#include "device_config.hpp"
#include "domain/servo_command.hpp"
#include "domain/digital_inputs.hpp"
#include "main.h"
#include "sts3215.hpp"

#include <cstdint>
#include <cstdio>
#include <cstring>

extern "C" {
extern UART_HandleTypeDef huart1;  // STS3215 bus
extern UART_HandleTypeDef huart2;  // upstream USB serial
}

volatile Sts3215::Result g_servo_status = Sts3215::Result::Ok;
volatile uint8_t g_servo_id = 0;
volatile uint16_t g_servo_position = 0;
volatile uint8_t g_servo_error_flags = 0;

namespace {
enum class Mode : uint8_t { Safe, Run, Stop };

struct ServoState {
  uint8_t id = 0;
  bool used = false;
  bool enabled = false;
  bool has_target = false;
  Sts3215::Target target = {};
};

Sts3215 bus(&huart1, config::SERVO_TIMEOUT_MS, config::WAIT_FOR_WRITE_STATUS);
ServoState servos[config::MAX_SERVOS];
Mode mode = Mode::Safe;
bool protocol_ready = false;
uint32_t last_contact_ms = 0;
char line[config::LINE_CAPACITY] = {};
std::size_t line_length = 0;
bool line_overflow = false;
domain::DigitalInputs inputs(63);

void sampleInputs() {
  const uint16_t a = GPIOA->IDR;
  const uint16_t b = GPIOB->IDR;
  const uint8_t raw = (!(b & GPIO_PIN_1) ? 1 : 0) | (!(b & GPIO_PIN_0) ? 2 : 0) |
      (!(a & GPIO_PIN_7) ? 4 : 0) | (!(a & GPIO_PIN_6) ? 8 : 0) |
      (!(a & GPIO_PIN_5) ? 16 : 0) | (!(a & GPIO_PIN_4) ? 32 : 0);
  inputs.sample(raw, static_cast<uint8_t>((~b >> 4) & 15), HAL_GetTick());
}

void reply(const char* text) {
  HAL_UART_Transmit(&huart2, reinterpret_cast<const uint8_t*>(text),
                    static_cast<uint16_t>(std::strlen(text)), 20);
  static const uint8_t newline = '\n';
  HAL_UART_Transmit(&huart2, &newline, 1, 20);
}

ServoState* findServo(uint8_t id, bool create) {
  ServoState* free_entry = nullptr;
  for (auto& servo : servos) {
    if (servo.used && servo.id == id) return &servo;
    if (!servo.used && free_entry == nullptr) free_entry = &servo;
  }
  if (!create || free_entry == nullptr) return nullptr;
  free_entry->used = true;
  free_entry->id = id;
  return free_entry;
}

void disableAll() {
  for (auto& servo : servos) {
    if (servo.used) bus.setTorque(servo.id, false);
  }
}

void setMode(Mode next) {
  if (next != Mode::Run) {
    disableAll();
    // 再RUN時に前のセッションの出力を復帰させない。
    for (auto& servo : servos) {
      servo.enabled = false;
      servo.has_target = false;
    }
    mode = next;
    return;
  }
  if (!protocol_ready) {
    reply("ERR code=NOT_READY");
    return;
  }
  mode = Mode::Run;
  for (auto& servo : servos) {
    if (!servo.used || !servo.enabled) continue;
    g_servo_status = bus.setTorque(servo.id, true);
    if (g_servo_status == Sts3215::Result::Ok && servo.has_target) {
      g_servo_status = bus.setTarget(servo.target);
    }
  }
}

void reportPosition(uint8_t id, uint16_t position, bool enabled) {
  char output[80] = {};
  std::snprintf(output, sizeof(output), "SERVO_STATE id=%u position=%u enabled=%u error=%02X",
                static_cast<unsigned>(id), static_cast<unsigned>(position),
                static_cast<unsigned>(enabled), static_cast<unsigned>(bus.lastServoError()));
  reply(output);
}

void apply(const domain::ServoCommand& command) {
  switch (command.kind) {
    case domain::ServoCommandKind::Hello:
      protocol_ready = command.protocol_version == config::PROTOCOL_VERSION;
      reply(protocol_ready ? "DEVICE protocol=1 board=serial_svmd slots=16 watchdog_ms=250"
                           : "ERR code=BAD_VERSION");
      break;
    case domain::ServoCommandKind::Safe:
      setMode(Mode::Safe);
      break;
    case domain::ServoCommandKind::Run:
      setMode(Mode::Run);
      break;
    case domain::ServoCommandKind::Stop:
      setMode(Mode::Stop);
      break;
    case domain::ServoCommandKind::Heartbeat:
      break;
    case domain::ServoCommandKind::Enable: {
      ServoState* servo = findServo(command.id, true);
      if (servo == nullptr) {
        reply("ERR code=NO_SLOT");
        break;
      }
      servo->enabled = command.enabled;
      if (mode == Mode::Run) g_servo_status = bus.setTorque(command.id, command.enabled);
      break;
    }
    case domain::ServoCommandKind::Target: {
      ServoState* servo = findServo(command.id, true);
      if (servo == nullptr) {
        reply("ERR code=NO_SLOT");
        break;
      }
      servo->target = Sts3215::Target{command.id, command.acceleration, command.position, 0,
                                      command.speed};
      servo->has_target = true;
      if (mode == Mode::Run && servo->enabled) g_servo_status = bus.setTarget(servo->target);
      break;
    }
    case domain::ServoCommandKind::Read: {
      uint16_t position = 0;
      g_servo_status = bus.readPosition(command.id, position);
      g_servo_id = command.id;
      g_servo_error_flags = bus.lastServoError();
      if (g_servo_status == Sts3215::Result::Ok) {
        g_servo_position = position;
        const ServoState* servo = findServo(command.id, false);
        reportPosition(command.id, position,
                       mode == Mode::Run && servo != nullptr && servo->enabled);
      } else {
        reply("ERR code=SERVO_IO");
      }
      break;
    }
    case domain::ServoCommandKind::None:
      break;
    case domain::ServoCommandKind::InputRead: {
      char text[96];
      std::snprintf(text, sizeof(text), "INPUT_STATE raw=%u stable=%u dip=%u available=%u",
                    inputs.raw(), inputs.stable(), inputs.dip(), inputs.available());
      reply(text);
      break;
    }
  }
}

void consume(uint8_t byte) {
  if (byte == '\r') return;
  if (byte != '\n') {
    if (line_length + 1 < sizeof(line) && !line_overflow) line[line_length++] = byte;
    else line_overflow = true;
    return;
  }

  if (!line_overflow && line_length != 0) {
    const domain::ServoCommand command = domain::parseServoCommand(line, line_length);
    if (command.kind != domain::ServoCommandKind::None) {
      last_contact_ms = HAL_GetTick();
      apply(command);
    } else {
      reply("ERR code=BAD_COMMAND");
    }
  }
  line_length = 0;
  line_overflow = false;
}
}  // namespace

extern "C" void setup(void) {
  mode = Mode::Safe;
  protocol_ready = false;
  last_contact_ms = HAL_GetTick();
}

extern "C" void loop(void) {
  sampleInputs();
  for (uint8_t count = 0; count < 64; ++count) {
    uint8_t byte = 0;
    if (HAL_UART_Receive(&huart2, &byte, 1, 0) != HAL_OK) break;
    consume(byte);
  }

  const uint32_t now = HAL_GetTick();
  if (mode == Mode::Run && now - last_contact_ms > config::WATCHDOG_MS) {
    setMode(Mode::Stop);
    protocol_ready = false;
  }
}

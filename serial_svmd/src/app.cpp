#include "device_config.hpp"
#include "domain/servo_can.hpp"
#include "domain/servo_command.hpp"
#include "domain/digital_inputs.hpp"
#include "domain/parameters.hpp"
#include "main.h"
#include "sts3215.hpp"

#include <cstdint>
#include <cstdio>
#include <cstring>

extern "C" {
extern CAN_HandleTypeDef hcan;     // cctl FDCAN2
extern UART_HandleTypeDef huart1;  // STS3215 bus
extern UART_HandleTypeDef huart2;  // upstream USB serial
}

volatile Sts3215::Result g_servo_status = Sts3215::Result::Ok;
volatile uint8_t g_servo_id = 0;
volatile uint16_t g_servo_position = 0;
volatile uint8_t g_servo_error_flags = 0;

namespace {
enum class Mode : uint8_t { Safe, Run, Stop };

// 指令の届いた経路。応答は来た側へ返す。
enum class Link : uint8_t { Serial, Can };

struct ServoState {
  uint8_t id = 0;
  bool used = false;
  bool enabled = false;
  bool has_target = false;
  Sts3215::Target target = {};
};

domain::ServoParameters parameters;
Sts3215 bus(&huart1, config::SERVO_TIMEOUT_MS, config::WAIT_FOR_WRITE_STATUS);
ServoState servos[config::MAX_SERVOS];
Mode mode = Mode::Safe;
bool protocol_ready = false;
uint32_t last_contact_ms = 0;
char line[config::LINE_CAPACITY] = {};
std::size_t line_length = 0;
bool line_overflow = false;
domain::DigitalInputs inputs(63);
bool bus_ready = false;
Link source = Link::Serial;

void sampleInputs() {
  const uint16_t a = GPIOA->IDR;
  const uint16_t b = GPIOB->IDR;
  const uint8_t raw = (!(b & GPIO_PIN_1) ? 1 : 0) | (!(b & GPIO_PIN_0) ? 2 : 0) |
      (!(a & GPIO_PIN_7) ? 4 : 0) | (!(a & GPIO_PIN_6) ? 8 : 0) |
      (!(a & GPIO_PIN_5) ? 16 : 0) | (!(a & GPIO_PIN_4) ? 32 : 0);
  inputs.sample(raw, static_cast<uint8_t>((~b >> 4) & 15), HAL_GetTick());
}

void sendCan(uint16_t id, const uint8_t* data) {
  if (!bus_ready) return;
  CAN_TxHeaderTypeDef header = {};
  header.StdId = id;
  header.IDE = CAN_ID_STD;
  header.RTR = CAN_RTR_DATA;
  header.DLC = 8;
  uint32_t mailbox = 0;
  HAL_CAN_AddTxMessage(&hcan, &header, const_cast<uint8_t*>(data), &mailbox);
}

void sendStatus(domain::servo_can::Status status) {
  uint8_t count = 0;
  for (const auto& servo : servos) {
    if (servo.used) ++count;
  }
  uint8_t frame[8];
  domain::servo_can::encodeStatus(status, static_cast<uint8_t>(mode), count, frame);
  sendCan(domain::servo_can::STATUS_ID, frame);
}

// USART2へはASCIIの1行、CANへは拒否として返す。
void reply(const char* text) {
  if (source == Link::Can) {
    sendStatus(domain::servo_can::Status::Rejected);
    return;
  }
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
  if (source == Link::Can) {
    uint8_t frame[8];
    domain::servo_can::encodePosition(id, position, enabled, bus.lastServoError(), frame);
    sendCan(domain::servo_can::POSITION_ID, frame);
    return;
  }
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
      if (source == Link::Can) {
        sendStatus(protocol_ready ? domain::servo_can::Status::Ok
                                  : domain::servo_can::Status::Rejected);
      } else {
        reply(protocol_ready ? "DEVICE protocol=1 board=serial_svmd slots=16 watchdog_ms=250"
                             : "ERR code=BAD_VERSION");
      }
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
    case domain::ServoCommandKind::ParamSet: {
      if (!parameters.set(command.param_id, command.value)) {
        reply("ERR code=OUT_OF_RANGE");
        break;
      }
      bus.setTiming(parameters.timeoutMs(), parameters.waitForWriteStatus());
      if (command.param_id == static_cast<uint8_t>(domain::ServoParamId::ServoBaud)) {
        // サーボが1 Mbps出荷の個体だと、ここを変えられないと手が出ない。
        huart1.Init.BaudRate = parameters.baud();
        HAL_UART_Init(&huart1);
      }
      break;
    }
    case domain::ServoCommandKind::None:
      break;
    case domain::ServoCommandKind::InputRead: {
      if (source == Link::Can) {
        uint8_t frame[8];
        domain::servo_can::encodeInputs(inputs.raw(), inputs.stable(), inputs.dip(),
                                        inputs.available(), frame);
        sendCan(domain::servo_can::INPUT_ID, frame);
        break;
      }
      char text[96];
      std::snprintf(text, sizeof(text), "INPUT_STATE raw=%u stable=%u dip=%u available=%u",
                    inputs.raw(), inputs.stable(), inputs.dip(), inputs.available());
      reply(text);
      break;
    }
  }
}

// cctlのFDCAN2から届いた指令を、USART2と同じ状態機械へ入れる。
void pollCan(void) {
  for (uint8_t count = 0; count < 4 && HAL_CAN_GetRxFifoFillLevel(&hcan, CAN_RX_FIFO0); ++count) {
    CAN_RxHeaderTypeDef header = {};
    uint8_t data[8] = {};
    if (HAL_CAN_GetRxMessage(&hcan, CAN_RX_FIFO0, &header, data) != HAL_OK) break;
    if (header.IDE != CAN_ID_STD || header.RTR != CAN_RTR_DATA ||
        header.StdId != domain::servo_can::COMMAND_ID) {
      continue;
    }
    domain::ServoCommand command;
    source = Link::Can;
    if (domain::servo_can::parse(data, header.DLC, command)) {
      last_contact_ms = HAL_GetTick();
      apply(command);
      if (command.kind != domain::ServoCommandKind::Read &&
          command.kind != domain::ServoCommandKind::InputRead &&
          command.kind != domain::ServoCommandKind::Hello) {
        sendStatus(domain::servo_can::Status::Ok);
      }
    } else {
      sendStatus(domain::servo_can::Status::Rejected);
    }
    source = Link::Serial;
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

  CAN_FilterTypeDef filter = {};
  filter.FilterBank = 0;
  filter.FilterMode = CAN_FILTERMODE_IDMASK;
  filter.FilterScale = CAN_FILTERSCALE_32BIT;
  filter.FilterIdHigh = domain::servo_can::COMMAND_ID << 5;
  filter.FilterMaskIdHigh = 0x7FF << 5;
  filter.FilterFIFOAssignment = CAN_FILTER_FIFO0;
  filter.FilterActivation = ENABLE;
  bus_ready = HAL_CAN_ConfigFilter(&hcan, &filter) == HAL_OK &&
              HAL_CAN_Start(&hcan) == HAL_OK;
}

extern "C" void loop(void) {
  sampleInputs();
  pollCan();
  for (uint8_t count = 0; count < 64; ++count) {
    uint8_t byte = 0;
    if (HAL_UART_Receive(&huart2, &byte, 1, 0) != HAL_OK) break;
    consume(byte);
  }

  const uint32_t now = HAL_GetTick();
  if (mode == Mode::Run && now - last_contact_ms > parameters.watchdogMs()) {
    setMode(Mode::Stop);
    protocol_ready = false;
    sendStatus(domain::servo_can::Status::Timeout);
  }
}

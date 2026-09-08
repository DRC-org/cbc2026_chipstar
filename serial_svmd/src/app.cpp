#include "device_config.hpp"
#include "domain/servo_can.hpp"
#include "domain/servo_command.hpp"
#include "domain/digital_inputs.hpp"
#include "domain/parameters.hpp"
#include "domain/status_led.hpp"
#include "main.h"
#include "sts3215.hpp"
#include "servo_service.hpp"

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
DMA_HandleTypeDef servo_rx_dma{};
bool servo_uart_ready = false;
bool command_failed = false;
bool stop_retry = false;
uint32_t last_stop_retry_ms = 0;
Sts3215 bus(&huart1, config::SERVO_TIMEOUT_MS, config::WAIT_FOR_WRITE_STATUS);
void serviceReply(uint16_t id, const uint8_t* data);
bool serviceBaud(uint32_t baud);
ServoService servo_service(bus, serviceReply, serviceBaud);
ServoState servos[config::MAX_SERVOS];
Mode mode = Mode::Safe;
bool protocol_ready = false;
uint32_t last_contact_ms = 0;
char line[config::LINE_CAPACITY] = {};
std::size_t line_length = 0;
bool line_overflow = false;
domain::DigitalInputs inputs(63);
bool bus_ready = false;
uint8_t address = 0;
Link source = Link::Serial;

// LED1..6は基板の左から並ぶ。状態は点け方で表す。
domain::Status ledStatus() {
  if (!bus_ready || !servo_uart_ready) return domain::Status::Error;
  // 停止はBootより先に見る。通信断はprotocol_readyも落とすため。
  if (mode == Mode::Stop) return domain::Status::Stop;
  if (!protocol_ready) return domain::Status::Boot;
  return mode == Mode::Run ? domain::Status::Run : domain::Status::Safe;
}

void updateLeds(uint32_t now) {
  const uint8_t bits = domain::statusPattern(now, ledStatus(), 6);
  const auto level = [bits](uint8_t index) {
    return (bits & (1U << index)) != 0 ? GPIO_PIN_SET : GPIO_PIN_RESET;
  };
  HAL_GPIO_WritePin(LED1_GPIO_Port, LED1_Pin, level(0));
  HAL_GPIO_WritePin(LED2_GPIO_Port, LED2_Pin, level(1));
  HAL_GPIO_WritePin(LED3_GPIO_Port, LED3_Pin, level(2));
  HAL_GPIO_WritePin(LED4_GPIO_Port, LED4_Pin, level(3));
  HAL_GPIO_WritePin(LED5_GPIO_Port, LED5_Pin, level(4));
  HAL_GPIO_WritePin(LED6_GPIO_Port, LED6_Pin, level(5));
}

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
  const uint32_t start = HAL_GetTick();
  while (HAL_CAN_GetTxMailboxesFreeLevel(&hcan) == 0) {
    if (HAL_GetTick() - start >= 5) {
      mode = Mode::Stop;
      stop_retry = true;
      command_failed = true;
      return;
    }
  }
  if (HAL_CAN_AddTxMessage(&hcan, &header, const_cast<uint8_t*>(data), &mailbox) != HAL_OK) {
    mode = Mode::Stop;
    stop_retry = true;
    command_failed = true;
  }
}

void sendStatus(domain::servo_can::Status status) {
  uint8_t count = 0;
  for (const auto& servo : servos) {
    if (servo.used) ++count;
  }
  uint8_t frame[8];
  domain::servo_can::encodeStatus(status, static_cast<uint8_t>(mode), count, frame);
  sendCan(domain::servo_can::canId(domain::servo_can::STATUS_ID, address), frame);
}

// USART2へはASCIIの1行、CANへは拒否として返す。
void reply(const char* text) {
  if (std::strncmp(text, "ERR", 3) == 0) command_failed = true;
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

bool configureServoUart() {
  servo_uart_ready = false;
  bus.stopReceiver();
  huart1.Init.BaudRate = parameters.baud();
  // 8MHzで1Mbpsには8倍サンプリングが必要。16倍ではBRRの下限を満たさない。
  huart1.Init.OverSampling = UART_OVERSAMPLING_8;
  if (HAL_UART_Init(&huart1) != HAL_OK) return false;
  servo_uart_ready = bus.startReceiver() == Sts3215::Result::Ok;
  return servo_uart_ready;
}

bool serviceBaud(uint32_t baud) {
  if (!parameters.set(0, static_cast<float>(baud))) return false;
  return configureServoUart();
}

void serviceReply(uint16_t id, const uint8_t* data) {
  if (id == 0x326 && data[1] == 21 && data[4] == 0 && data[5] == 5) {
    for (auto& servo : servos) if (servo.used && servo.id == data[2]) servo = {};
  }
  // ACKのIDは計測フレームより小さいため、CAN優先順位で追い越さないよう待つ。
  if (id == 0x326 && bus_ready) {
    const uint32_t started = HAL_GetTick();
    while (HAL_CAN_GetTxMailboxesFreeLevel(&hcan) != 3) {
      if (HAL_GetTick() - started >= 5) {
        mode = Mode::Stop;
        stop_retry = true;
        return;
      }
    }
  }
  sendCan(domain::servo_can::canId(id, address), data);
  // 一括監視の連射で、受信側の3要素FIFOが満杯になるのを避ける。
  // 分割データ間で受信側に処理時間を与える。
  if (id == 0x327) HAL_Delay(2);
  if (id == 0x326 && data[4] != 0 && mode == Mode::Run) {
    mode = Mode::Stop;
    stop_retry = true;
  }
}

void disableAll() {
  // 登録されていない個体も停止対象。broadcastにはACKは返らない。
  stop_retry = bus.setTorque(Sts3215::BROADCAST_ID, false) != Sts3215::Result::Ok;
  if (!servo_service.stop()) stop_retry = true;
  for (auto& servo : servos) {
    if (servo.used) {
      const auto result = bus.setTorque(servo.id, false);
      if (result != Sts3215::Result::Ok) { command_failed = true; stop_retry = true; }
    }
    servo.enabled = false;
    servo.has_target = false;
  }
}

bool checkServoIo(Sts3215::Result result, uint8_t id) {
  g_servo_status = result;
  g_servo_id = id;
  g_servo_error_flags = bus.lastServoError();
  if (result == Sts3215::Result::Ok) return true;
  const auto hal = bus.lastHalStatus();
  const auto flags = bus.lastServoError();
  disableAll();
  mode = Mode::Stop;
  command_failed = true;
  if (source == Link::Can) {
    uint8_t detail[8] = {1, id, static_cast<uint8_t>(result), static_cast<uint8_t>(hal), flags, 0, 0, 0};
    sendCan(domain::servo_can::canId(0x325, address), detail);
    sendStatus(domain::servo_can::Status::Rejected);
  } else {
    char text[96];
    std::snprintf(text, sizeof(text), "ERR code=SERVO_IO id=%u result=%u hal=%u flags=%u", id,
                  static_cast<unsigned>(result), static_cast<unsigned>(hal), flags);
    reply(text);
  }
  return false;
}

void setMode(Mode next) {
  if (next != Mode::Run) {
    disableAll();
    mode = next;
    if (command_failed) reply("ERR code=SERVO_STOP_UNCONFIRMED");
    return;
  }
  if (!protocol_ready) { reply("ERR code=NOT_READY"); return; }
  if (!servo_uart_ready) { reply("ERR code=SERVO_UART"); return; }
  if (stop_retry) { reply("ERR code=STOP_PENDING"); return; }
  if (mode == Mode::Run) return;
  mode = Mode::Run;
  for (auto& servo : servos) {
    if (!servo.used || !servo.enabled) continue;
    if (!checkServoIo(bus.preparePosition(servo.id, servo.has_target ? &servo.target : nullptr), servo.id)) return;
  }
}

void reportPosition(uint8_t id, uint16_t position, bool enabled) {
  if (source == Link::Can) {
    uint8_t frame[8];
    domain::servo_can::encodePosition(id, position, enabled, bus.lastServoError(), frame);
    sendCan(domain::servo_can::canId(domain::servo_can::POSITION_ID, address), frame);
    return;
  }
  char output[80] = {};
  std::snprintf(output, sizeof(output), "SERVO_STATE id=%u position=%u enabled=%u error=%02X",
                static_cast<unsigned>(id), static_cast<unsigned>(position),
                static_cast<unsigned>(enabled), static_cast<unsigned>(bus.lastServoError()));
  reply(output);
}

void apply(const domain::ServoCommand& command) {
  command_failed = false;
  switch (command.kind) {
    case domain::ServoCommandKind::Hello:
      protocol_ready = command.protocol_version == config::PROTOCOL_VERSION;
      if (source == Link::Can) {
        sendStatus(protocol_ready ? domain::servo_can::Status::Ok
                                  : domain::servo_can::Status::Rejected);
      } else {
        if (!protocol_ready) {
          reply("ERR code=BAD_VERSION");
        } else {
          char text[80];
          std::snprintf(text, sizeof(text),
                        "DEVICE protocol=1 board=serial_svmd slots=16 watchdog_ms=%lu",
                        static_cast<unsigned long>(parameters.watchdogMs()));
          reply(text);
        }
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
      if (mode == Mode::Run && command.enabled && !servo->enabled) {
        if (!checkServoIo(bus.preparePosition(command.id, servo->has_target ? &servo->target : nullptr), command.id)) break;
      } else if (!command.enabled) {
        if (!checkServoIo(bus.setTorque(command.id, false), command.id)) break;
      }
      servo->enabled = command.enabled;
      if (!command.enabled) servo->has_target = false;
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
      if (mode == Mode::Run && servo->enabled) checkServoIo(bus.setTarget(servo->target), command.id);
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
        checkServoIo(g_servo_status, command.id);
      }
      break;
    }
    case domain::ServoCommandKind::ParamSet: {
      if (mode == Mode::Run) { reply("ERR code=BUSY"); break; }
      const auto previous = parameters;
      if (!parameters.set(command.param_id, command.value)) {
        reply("ERR code=OUT_OF_RANGE");
        break;
      }
      bus.setTiming(parameters.timeoutMs(), parameters.waitForWriteStatus());
      if (command.param_id == static_cast<uint8_t>(domain::ServoParamId::ServoBaud)) {
        // サーボが1 Mbps出荷の個体だと、ここを変えられないと手が出ない。
        if (!configureServoUart()) {
          parameters = previous;
          bus.setTiming(parameters.timeoutMs(), parameters.waitForWriteStatus());
          configureServoUart();
          reply("ERR code=SERVO_UART"); break;
        }
      }
      if (source == Link::Can) {
        const float value = parameters.get(static_cast<domain::ServoParamId>(command.param_id));
        uint32_t bits; std::memcpy(&bits, &value, sizeof(bits));
        uint8_t report[8] = {1, command.param_id, 0, 0,
            static_cast<uint8_t>(bits >> 24), static_cast<uint8_t>(bits >> 16),
            static_cast<uint8_t>(bits >> 8), static_cast<uint8_t>(bits)};
        sendCan(domain::servo_can::canId(0x324, address), report);
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
        sendCan(domain::servo_can::canId(domain::servo_can::INPUT_ID, address), frame);
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
        header.StdId != domain::servo_can::canId(domain::servo_can::COMMAND_ID, address)) {
      continue;
    }
    domain::ServoCommand command;
    source = Link::Can;
    if (header.DLC == 8 && data[1] >= 20 && data[1] <= 27) {
      if (protocol_ready && servo_uart_ready) {
        servo_service.handle(data, mode == Mode::Run);
      } else {
        uint8_t rejected[8] = {1, data[1], data[2], data[3], 2, 0, 0, 0};
        serviceReply(0x326, rejected);
      }
      source = Link::Serial;
      continue;
    }
    if (domain::servo_can::parse(data, header.DLC, command)) {
      if (command.kind == domain::ServoCommandKind::Run ||
          command.kind == domain::ServoCommandKind::Target ||
          command.kind == domain::ServoCommandKind::Heartbeat) last_contact_ms = HAL_GetTick();
      apply(command);
      if (command.kind != domain::ServoCommandKind::Read &&
          command.kind != domain::ServoCommandKind::InputRead &&
          command.kind != domain::ServoCommandKind::Hello && !command_failed) {
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
      if (command.kind == domain::ServoCommandKind::Run ||
          command.kind == domain::ServoCommandKind::Target ||
          command.kind == domain::ServoCommandKind::Heartbeat) last_contact_ms = HAL_GetTick();
      apply(command);
    } else {
      reply("ERR code=BAD_COMMAND");
    }
  }
  line_length = 0;
  line_overflow = false;
}
// USART2はポーリングで受ける。サーボバス(USART1)の送受信でループが数十ms止まる
// ため、その間に届いたバイトは取りこぼす。問題は取りこぼしそのものではなく、
// オーバーラン(ORE)を放置するとRXNEが二度と立たず受信が永久に止まることで、
// HAL_UART_Receiveはこのフラグを落とさない。RDRを直接読み、明示的に落とす。
void pollSerial() {
  for (uint8_t count = 0; count < 64; ++count) {
    const uint32_t status = huart2.Instance->ISR;
    if (status & (USART_ISR_ORE | USART_ISR_FE | USART_ISR_NE)) {
      huart2.Instance->ICR = USART_ICR_ORECF | USART_ICR_FECF | USART_ICR_NCF;
      // 途中まで受けた行は欠けているので捨てる。
      line_length = 0;
      line_overflow = false;
    }
    if ((status & USART_ISR_RXNE) == 0) break;
    consume(static_cast<uint8_t>(huart2.Instance->RDR & 0xFF));
  }
}

}  // namespace

extern "C" void setup(void) {
  __HAL_RCC_DMA1_CLK_ENABLE();
  servo_rx_dma.Instance = DMA1_Channel5; // STM32F303 USART1_RXの既定マッピング
  servo_rx_dma.Init.Direction = DMA_PERIPH_TO_MEMORY;
  servo_rx_dma.Init.PeriphInc = DMA_PINC_DISABLE;
  servo_rx_dma.Init.MemInc = DMA_MINC_ENABLE;
  servo_rx_dma.Init.PeriphDataAlignment = DMA_PDATAALIGN_BYTE;
  servo_rx_dma.Init.MemDataAlignment = DMA_MDATAALIGN_BYTE;
  servo_rx_dma.Init.Mode = DMA_CIRCULAR;
  servo_rx_dma.Init.Priority = DMA_PRIORITY_HIGH;
  __HAL_LINKDMA(&huart1, hdmarx, servo_rx_dma);
  if (HAL_DMA_Init(&servo_rx_dma) != HAL_OK || !configureServoUart()) {
    g_servo_status = Sts3215::Result::HalError;
  }
  // アドレスは起動時に一度だけ読む。
  sampleInputs();
  address = static_cast<uint8_t>(inputs.dip() & domain::servo_can::MAX_ADDRESS);
  mode = Mode::Safe;
  protocol_ready = false;
  last_contact_ms = HAL_GetTick();

  CAN_FilterTypeDef filter = {};
  filter.FilterBank = 0;
  filter.FilterMode = CAN_FILTERMODE_IDMASK;
  filter.FilterScale = CAN_FILTERSCALE_32BIT;
  filter.FilterIdHigh = domain::servo_can::canId(domain::servo_can::COMMAND_ID, address) << 5;
  filter.FilterMaskIdHigh = 0x7FF << 5;
  filter.FilterFIFOAssignment = CAN_FILTER_FIFO0;
  filter.FilterActivation = ENABLE;
  // 調停負け・一時的な送信エラーで確認応答を失わないようにする。
  hcan.Init.AutoRetransmission = ENABLE;
  bus_ready = HAL_CAN_Init(&hcan) == HAL_OK &&
              HAL_CAN_ConfigFilter(&hcan, &filter) == HAL_OK &&
              HAL_CAN_Start(&hcan) == HAL_OK;
}

extern "C" void loop(void) {
  sampleInputs();
  pollCan();
  pollSerial();

  const uint32_t now = HAL_GetTick();
  updateLeds(now);
  if (mode != Mode::Run && stop_retry && now - last_stop_retry_ms >= 50) {
    last_stop_retry_ms = now;
    disableAll();
  }
  if (mode == Mode::Run && now - last_contact_ms > parameters.watchdogMs()) {
    setMode(Mode::Stop);
    protocol_ready = false;
    sendStatus(domain::servo_can::Status::Timeout);
  }
}

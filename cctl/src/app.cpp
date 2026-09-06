#include "actuator_controller.hpp"
#include "can_bus.hpp"
#include "device_config.hpp"
#include "domain/can_frame.hpp"
#include "domain/command.hpp"
#include "domain/command_queue.hpp"
#include "domain/line_reader.hpp"
#include "domain/parameters.hpp"
#include "domain/status_led.hpp"
#include "domain/telemetry.hpp"
#include "domain/digital_inputs.hpp"
#include "main.h"
#include "ui.hpp"
#include "usbd_cdc_if.h"

#include <cstdint>
#include <cstdio>
#include <cstring>

extern "C" {
extern FDCAN_HandleTypeDef hfdcan1;
extern FDCAN_HandleTypeDef hfdcan2;
extern I2C_HandleTypeDef hi2c1;
extern TIM_HandleTypeDef htim15;
extern TIM_HandleTypeDef htim2;
extern USBD_HandleTypeDef hUsbDeviceFS;
}

namespace {
Ui ui(&hi2c1, &htim15);
CanBus motor_bus(&hfdcan1);
CanBus peripheral_bus(&hfdcan2);
ActuatorController controller(motor_bus);
domain::LineReader usb_line;
domain::CommandQueue commands;
volatile uint32_t last_contact_ms = 0;
bool protocol_ready = false;
bool peripheral_bus_ready = false;
bool motor_bus_ready = false;
domain::DigitalInputs inputs(7);

void sampleInputs() {
    const uint8_t raw = (HAL_GPIO_ReadPin(SW1_GPIO_Port, SW1_Pin) == GPIO_PIN_RESET ? 1 : 0) |
        (HAL_GPIO_ReadPin(SW2_GPIO_Port, SW2_Pin) == GPIO_PIN_RESET ? 2 : 0) |
        (HAL_GPIO_ReadPin(SW3_GPIO_Port, SW3_Pin) == GPIO_PIN_RESET ? 4 : 0);
    const uint8_t dip = (HAL_GPIO_ReadPin(DIP1_GPIO_Port, DIP1_Pin) == GPIO_PIN_RESET ? 1 : 0) |
        (HAL_GPIO_ReadPin(DIP2_GPIO_Port, DIP2_Pin) == GPIO_PIN_RESET ? 2 : 0) |
        (HAL_GPIO_ReadPin(DIP3_GPIO_Port, DIP3_Pin) == GPIO_PIN_RESET ? 4 : 0) |
        (HAL_GPIO_ReadPin(DIP4_GPIO_Port, DIP4_Pin) == GPIO_PIN_RESET ? 8 : 0);
    inputs.sample(raw, dip, HAL_GetTick());
}

bool usbReady() {
    const auto* cdc = static_cast<const USBD_CDC_HandleTypeDef*>(hUsbDeviceFS.pClassData);
    return hUsbDeviceFS.dev_state == USBD_STATE_CONFIGURED && cdc != nullptr &&
           cdc->TxState == 0;
}

// USB CDCへの送信は前の転送が終わるまで受け付けられない。直接叩くと、
// 50ms周期のSTATEと衝突した応答が黙って捨てられる。いったん積んでおき、
// 送れるときにまとめて出す。
constexpr std::size_t TX_CAPACITY = 2048;
uint8_t tx_ring[TX_CAPACITY];
std::size_t tx_head = 0;
std::size_t tx_tail = 0;

std::size_t txUsed() {
    return tx_head >= tx_tail ? tx_head - tx_tail : TX_CAPACITY - tx_tail + tx_head;
}

// 積めない分は捨てる。溢れるのは受け手が読んでいないときなので、
// 古い行を消して新しい行を残すより、送信済みの並びを保つ方を優先する。
void enqueue(const uint8_t* data, std::size_t length) {
    if (length > TX_CAPACITY - 1 - txUsed()) return;
    for (std::size_t i = 0; i < length; ++i) {
        tx_ring[tx_head] = data[i];
        tx_head = (tx_head + 1) % TX_CAPACITY;
    }
}

void pumpUsb() {
    if (!usbReady() || txUsed() == 0) return;
    // 転送完了までバッファが生きている必要があるので、静的な領域へ移す。
    static uint8_t chunk[64];
    std::size_t length = 0;
    while (length < sizeof(chunk) && txUsed() != 0) {
        chunk[length++] = tx_ring[tx_tail];
        tx_tail = (tx_tail + 1) % TX_CAPACITY;
    }
    CDC_Transmit_FS(chunk, static_cast<uint16_t>(length));
}

void sendText(const char* text) {
    const std::size_t length = std::strlen(text);
    enqueue(reinterpret_cast<const uint8_t*>(text), length);
    const uint8_t newline = '\n';
    enqueue(&newline, 1);
}

void sendParameter(uint8_t id) {
    char text[64];
    char value[32];
    domain::formatFixed3(controller.parameters().get(id), value, sizeof(value));
    std::snprintf(text, sizeof(text), "PARAM %u %s", static_cast<unsigned>(id), value);
    sendText(text);
}

// CANの診断。バスオフか、送受信のエラーカウンタがどう動いているかを返す。
// tec が増え rec が 0 のままなら、送っているが誰も応答していない。
void sendCanStat(uint8_t bus, const CanBus& can, bool started) {
    const uint32_t psr = can.protocolStatus();
    const uint32_t ecr = can.errorCounters();
    char text[112];
    std::snprintf(text, sizeof(text),
                  "CANSTAT bus=%u started=%u busoff=%u lec=%lu tec=%lu rec=%lu cel=%lu",
                  static_cast<unsigned>(bus), static_cast<unsigned>(started ? 1 : 0),
                  static_cast<unsigned>((psr & FDCAN_PSR_BO) ? 1 : 0),
                  static_cast<unsigned long>(psr & 0x7),
                  static_cast<unsigned long>(ecr & 0xFF),
                  static_cast<unsigned long>((ecr >> 8) & 0x7F),
                  static_cast<unsigned long>((ecr >> 16) & 0xFF));
    sendText(text);
}

void applyCommand(const domain::Command& command) {
    switch (command.kind) {
        case domain::CommandKind::Hello:
            protocol_ready = command.protocol_version == 1;
            if (!protocol_ready) {
                sendText("ERR code=BAD_VERSION");
                break;
            } else {
                char text[80];
                std::snprintf(text, sizeof(text),
                              "DEVICE protocol=1 board=cctl slots=3 can=2 watchdog_ms=%lu",
                              static_cast<unsigned long>(
                                  controller.parameters().getMs(domain::ParamId::WatchdogMs)));
                sendText(text);
            }
            break;
        case domain::CommandKind::Stop:
            controller.setMode(domain::RunMode::Stop);
            break;
        case domain::CommandKind::Run:
            if (protocol_ready) controller.setMode(domain::RunMode::Run);
            else sendText("ERR code=NOT_READY");
            break;
        case domain::CommandKind::Safe:
            controller.setMode(domain::RunMode::Safe);
            break;
        case domain::CommandKind::Heartbeat:
            break;
        case domain::CommandKind::Enable:
            controller.setSlotsEnabled(command.mask, command.value);
            break;
        case domain::CommandKind::Home:
            controller.home(command.mask);
            break;
        case domain::CommandKind::Target:
            if (!controller.setTarget(command.slot, command.target)) {
                sendText("ERR code=OUT_OF_RANGE");
            }
            break;
        case domain::CommandKind::CanTx:
            if (!protocol_ready) {
                sendText("ERR code=NOT_READY");
            } else if (!peripheral_bus_ready) {
                sendText("ERR code=CAN_UNAVAILABLE");
            } else if (!peripheral_bus.sendStd(command.can_id, command.can_data,
                                               command.can_length)) {
                sendText("ERR code=CAN_TX");
            }
            break;
        case domain::CommandKind::ParamSet:
            if (!controller.setParameter(command.param_id, command.target)) {
                sendText(domain::requiresSafe(command.param_id) &&
                                 controller.mode() != domain::RunMode::Safe
                             ? "ERR code=BUSY"
                             : "ERR code=OUT_OF_RANGE");
            } else {
                sendParameter(command.param_id);
            }
            break;
        case domain::CommandKind::ParamGet:
            if (command.param_id >= domain::PARAM_COUNT) sendText("ERR code=OUT_OF_RANGE");
            else sendParameter(command.param_id);
            break;
        // モード違反とCAN送信失敗を分ける。混ぜると
        // 「SAFEにし忘れ」と「モータが繋がっていない」を切り分けられない。
        case domain::CommandKind::DmRegRead:
            if (controller.mode() != domain::RunMode::Safe) sendText("ERR code=BUSY");
            else if (!controller.readDmRegister(command.param_id)) sendText("ERR code=CAN_TX");
            break;
        case domain::CommandKind::DmRegWrite:
            if (controller.mode() != domain::RunMode::Safe) sendText("ERR code=BUSY");
            else if (!controller.writeDmRegister(command.param_id, command.raw_value)) {
                sendText("ERR code=CAN_TX");
            }
            break;
        case domain::CommandKind::CanStat:
            sendCanStat(1, motor_bus, motor_bus_ready);
            sendCanStat(2, peripheral_bus, peripheral_bus_ready);
            break;
        case domain::CommandKind::None:
            break;
    }
}

void sendCanFrame(const domain::CanFrame& frame) {
    static char line[96];
    if (frame.extended || frame.length > 8) return;

    const int prefix = std::snprintf(line, sizeof(line), "CAN_RX bus=2 id=%lu data=",
                                     static_cast<unsigned long>(frame.id));
    if (prefix < 0 || static_cast<std::size_t>(prefix) >= sizeof(line)) return;
    std::size_t length = static_cast<std::size_t>(prefix);
    if (frame.length == 0) {
        line[length++] = '-';
    } else {
        constexpr char HEX[] = "0123456789ABCDEF";
        for (uint8_t i = 0; i < frame.length; ++i) {
            line[length++] = HEX[frame.data[i] >> 4];
            line[length++] = HEX[frame.data[i] & 0x0F];
        }
    }
    line[length] = '\0';
    sendText(line);
}

void sendTelemetry() {
    static uint8_t line[domain::TELEMETRY_LINE_CAPACITY + 1];
    domain::Telemetry telemetry;
    telemetry.uptime_ms = HAL_GetTick();
    for (uint8_t slot = 0; slot < domain::SLOT_COUNT; ++slot) {
        telemetry.targets[slot] = controller.target(slot);
        telemetry.measured[slot] = controller.measured(slot);
    }
    telemetry.enabled_slots = controller.enabledSlots();
    telemetry.mode = controller.mode();
    for (uint8_t slot = 0; slot < domain::SLOT_COUNT; ++slot) {
        telemetry.error_bits[slot] = controller.errorBits(slot);
    }
    telemetry.contacts = inputs.stable();
    telemetry.stale_slots = controller.staleSlots();
    telemetry.buses = static_cast<uint8_t>(
        (motor_bus_ready && !motor_bus.busOff() ? 1 : 0) |
        (peripheral_bus_ready && !peripheral_bus.busOff() ? 2 : 0));

    const std::size_t length = domain::formatTelemetry(
        telemetry, reinterpret_cast<char*>(line), domain::TELEMETRY_LINE_CAPACITY);
    if (length == 0) return;
    line[length] = '\n';
    enqueue(line, length + 1);
}

// 基板の状態をLEDの点け方へ落とす。異常が最優先。
domain::Status ledStatus() {
    for (uint8_t slot = 0; slot < domain::SLOT_COUNT; ++slot) {
        if (controller.errorBits(slot) != 0) return domain::Status::Error;
    }
    // 停止はBootより先に見る。通信断はprotocol_readyも落とすため、
    // 順序を逆にすると「止まった」が「起動直後」に見えてしまう。
    if (controller.mode() == domain::RunMode::Stop) return domain::Status::Stop;
    if (!protocol_ready) return domain::Status::Boot;
    return controller.mode() == domain::RunMode::Run ? domain::Status::Run
                                                     : domain::Status::Safe;
}
}  // namespace

extern "C" void setup(void) {
    ui.begin();
    HAL_TIM_Base_Start_IT(&htim2);
    motor_bus_ready = motor_bus.begin();
    peripheral_bus_ready = peripheral_bus.begin();
    controller.begin();
    last_contact_ms = HAL_GetTick();
}

extern "C" void loop(void) {
    sampleInputs();
    domain::CanFrame frame;
    while (motor_bus.receive(frame)) controller.dispatchRx(frame);
    while (peripheral_bus.receive(frame)) sendCanFrame(frame);

    domain::Command command;
    while (commands.pop(command)) applyCommand(command);

    const uint32_t now = HAL_GetTick();
    if (controller.mode() == domain::RunMode::Run &&
        now - last_contact_ms > controller.parameters().getMs(domain::ParamId::WatchdogMs)) {
        controller.setMode(domain::RunMode::Stop);
        protocol_ready = false;
    }

    controller.update();

    // バスオフからの自動復帰。放置すると電源を入れ直すまでCANが死ぬ。
    static uint32_t last_recover_ms = 0;
    if (now - last_recover_ms >= 100) {
        last_recover_ms = now;
        motor_bus.recover();
        peripheral_bus.recover();
    }

    static uint32_t last_lcd_ms = 0;
    if (now - last_lcd_ms >= config::period::LCD_MS) {
        last_lcd_ms = now;
        // LCDは1行に収めるため、slotの異常をORして「どこかで異常」として出す。
        ui.showStatus(controller.target(0), controller.target(1), controller.target(2),
                      static_cast<uint8_t>(controller.errorBits(0) | controller.errorBits(1) |
                                           controller.errorBits(2)));
    }

    // DMのレジスタ応答は非同期に届く。届いた時点で1行返す。
    uint8_t reg_id = 0;
    uint32_t reg_raw = 0;
    if (controller.takeDmRegisterReply(reg_id, reg_raw)) {
        float as_float = 0.0f;
        std::memcpy(&as_float, &reg_raw, sizeof(as_float));
        char value[32];
        domain::formatFixed3(as_float, value, sizeof(value));
        char text[80];
        std::snprintf(text, sizeof(text), "DMREG rid=%u raw=%08lX f=%s",
                      static_cast<unsigned>(reg_id), static_cast<unsigned long>(reg_raw), value);
        sendText(text);
    }

    static uint32_t last_telemetry_ms = 0;
    if (now - last_telemetry_ms >= controller.parameters().getMs(domain::ParamId::TelemetryPeriodMs)) {
        last_telemetry_ms = now;
        sendTelemetry();
    }
    pumpUsb();
}

extern "C" void cctl_usbcdc_receive(const uint8_t* data, uint32_t length) {
    for (uint32_t i = 0; i < length; ++i) {
        if (!usb_line.push(static_cast<char>(data[i]))) continue;
        const domain::Command command = domain::parseCommand(usb_line.line(), usb_line.length());
        if (command.kind != domain::CommandKind::None && commands.push(command) &&
            domain::extendsDeadline(command.kind)) {
            last_contact_ms = HAL_GetTick();
        }
    }
}

extern "C" void HAL_TIM_PeriodElapsedCallback(TIM_HandleTypeDef* htim) {
    if (htim->Instance == TIM2) {
        static uint32_t tick_ms = 0;
        ++tick_ms;
        if (tick_ms % 20U == 0U) {
            ui.updateLeds(tick_ms, ledStatus());
        }
    }
}

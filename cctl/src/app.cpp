#include "actuator_controller.hpp"
#include "can_bus.hpp"
#include "device_config.hpp"
#include "domain/can_frame.hpp"
#include "domain/motor_discovery.hpp"
#include "domain/command.hpp"
#include "domain/command_queue.hpp"
#include "domain/line_reader.hpp"
#include "domain/parameters.hpp"
#include "param_store.hpp"
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
domain::c620::Discovery c620_discovery;
domain::MotorDiscovery motor_discovery;
uint32_t motor_scan_tx_failed = 0;
uint32_t motor_scan_el05_completed = 0;
uint32_t motor_scan_dm_completed = 0;
uint32_t motor_standard_count = 0;
uint32_t motor_extended_count = 0;
uint32_t motor_last_standard = 0;
uint32_t motor_last_extended = 0;

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
    domain::formatFixed5(controller.parameters().get(id), value, sizeof(value));
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
                  "CANSTAT bus=%u started=%u busoff=%u lec=%lu tec=%lu rec=%lu cel=%lu tx_failed=%lu",
                  static_cast<unsigned>(bus), static_cast<unsigned>(started ? 1 : 0),
                  static_cast<unsigned>((psr & FDCAN_PSR_BO) ? 1 : 0),
                  static_cast<unsigned long>(psr & 0x7),
                  static_cast<unsigned long>(ecr & 0xFF),
                  static_cast<unsigned long>((ecr >> 8) & 0x7F),
                  static_cast<unsigned long>((ecr >> 16) & 0xFF),
                  static_cast<unsigned long>(can.txFailures()));
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
                char text[96];
                std::snprintf(text, sizeof(text),
                              "DEVICE protocol=1 board=cctl slots=3 can=2 watchdog_ms=%lu "
                              "params=%s jog=1",
                              static_cast<unsigned long>(
                                  controller.parameters().getMs(domain::ParamId::WatchdogMs)),
                              param_store::present() ? "stored" : "default");
                sendText(text);
            }
            break;
        case domain::CommandKind::Stop:
            if (!controller.setMode(domain::RunMode::Stop)) sendText("ERR code=CAN_TX");
            break;
        case domain::CommandKind::Run:
            if (protocol_ready) {
                if (!controller.setMode(domain::RunMode::Run)) sendText("ERR code=CAN_TX");
            }
            else sendText("ERR code=NOT_READY");
            break;
        case domain::CommandKind::Safe:
            if (!controller.setMode(domain::RunMode::Safe)) sendText("ERR code=CAN_TX");
            break;
        case domain::CommandKind::Heartbeat:
            break;
        case domain::CommandKind::Enable:
            if (!controller.setSlotsEnabled(command.mask, command.value)) sendText("ERR code=CAN_TX");
            break;
        case domain::CommandKind::Home:
            controller.home(command.mask);
            break;
        case domain::CommandKind::Target:
            if (!controller.setTarget(command.slot, command.target)) {
                sendText("ERR code=OUT_OF_RANGE");
            }
            break;
        case domain::CommandKind::Jog:
            if (!controller.setJog(command.slot, command.target)) {
                char text[128];
                std::snprintf(text, sizeof(text),
                              "ERR code=JOG_REJECTED slot=%u mode=%u enabled=%u stale=%u",
                              static_cast<unsigned>(command.slot),
                              static_cast<unsigned>(controller.mode()),
                              static_cast<unsigned>(controller.enabledSlots()),
                              static_cast<unsigned>(controller.staleSlots()));
                sendText(text);
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
        case domain::CommandKind::ParamSet: {
            const uint32_t failures = motor_bus.txFailures();
            if (!controller.setParameter(command.param_id, command.target)) {
                if (motor_bus.txFailures() != failures) sendText("ERR code=CAN_TX");
                else sendText(domain::requiresSafe(command.param_id) &&
                                 controller.mode() != domain::RunMode::Safe
                             ? "ERR code=BUSY"
                             : "ERR code=OUT_OF_RANGE");
            } else {
                sendParameter(command.param_id);
            }
            break;
        }
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
        case domain::CommandKind::DmStore:
            if (!controller.storeDmParameters()) sendText("ERR code=DMSTORE_REJECTED");
            break;
        case domain::CommandKind::DmRegWrite:
            if (controller.mode() != domain::RunMode::Safe) sendText("ERR code=BUSY");
            else if (!controller.writeDmRegister(command.param_id, command.raw_value)) {
                sendText("ERR code=CAN_TX");
            }
            break;
        case domain::CommandKind::Reinit: {
            const bool safe = controller.mode() == domain::RunMode::Safe;
            if (!controller.reinitialize(command.mask)) sendText(safe ? "ERR code=CAN_TX" : "ERR code=BUSY");
            else sendText("OK");
            break;
        }
        case domain::CommandKind::ParamDefault: {
            const uint32_t failures = motor_bus.txFailures();
            if (!controller.resetParameters()) {
                sendText(motor_bus.txFailures() != failures ? "ERR code=CAN_TX" : "ERR code=BUSY");
            }
            else sendText("OK");
            break;
        }
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
    uint32_t completed_id = 0;
    bool completed_extended = false;
    while (motor_bus.receiveTxCompletion(completed_id, completed_extended)) {
        if (completed_extended) ++motor_scan_el05_completed;
        else if (completed_id == domain::dm::CONFIG_ID) ++motor_scan_dm_completed;
    }
    domain::CanFrame frame;
    while (motor_bus.receive(frame)) {
        if (!frame.extended && frame.length == 8) c620_discovery.observe(frame.id, HAL_GetTick());
        if (frame.extended) {
            ++motor_extended_count;
            motor_last_extended = frame.id;
            if (frame.length == 8 && domain::el05::commType(frame.id) == 0 &&
                domain::el05::targetId(frame.id) == 0xFE) {
                char text[80];
                std::snprintf(text, sizeof(text), "MOTOR_ID kind=el05 id=%u",
                    static_cast<unsigned>(domain::el05::dataArea2(frame.id)));
                sendText(text);
            }
        } else {
            ++motor_standard_count;
            motor_last_standard = frame.id;
            if (frame.length == 4 && frame.id == controller.parameters().getU16(domain::ParamId::DmMstId) &&
                (frame.data[0] | (static_cast<uint16_t>(frame.data[1]) << 8)) ==
                    controller.parameters().getU16(domain::ParamId::DmCanId) &&
                frame.data[2] == domain::dm::CONFIG_STORE && frame.data[3] == 1) {
                sendText("DMSTORE result=stored");
                continue;
            }
            static uint32_t last_dm_raw_ms = 0;
            if (frame.length == 8 && frame.id == controller.parameters().getU16(domain::ParamId::DmMstId) &&
                HAL_GetTick() - last_dm_raw_ms >= 100) {
                last_dm_raw_ms = HAL_GetTick();
                char raw[100];
                std::snprintf(raw, sizeof(raw), "DM_RX id=%lu data=%02X%02X%02X%02X%02X%02X%02X%02X",
                    static_cast<unsigned long>(frame.id), frame.data[0], frame.data[1], frame.data[2], frame.data[3],
                    frame.data[4], frame.data[5], frame.data[6], frame.data[7]);
                sendText(raw);
            }
            if (frame.length == 8 && frame.data[2] == domain::dm::CONFIG_READ &&
                frame.data[3] == domain::dm::reg::ESC_ID) {
                const uint16_t id = frame.data[0] | (static_cast<uint16_t>(frame.data[1]) << 8);
                if (domain::dm::configReplyValue(frame.data) == id) {
                    char text[80];
                    std::snprintf(text, sizeof(text), "MOTOR_ID kind=dm id=%u feedback_id=%lu",
                        static_cast<unsigned>(id), static_cast<unsigned long>(frame.id));
                    sendText(text);
                    // 識別応答を位置フィードバックとして数えない。
                    continue;
                }
            }
        }
        controller.dispatchRx(frame);
    }
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
    domain::CanFrame query;
    if (motor_discovery.next(now, controller.mode() != domain::RunMode::Run,
            controller.parameters().getU8(domain::ParamId::El05HostId), query)) {
        const bool sent = query.extended
            ? motor_bus.sendExt(query.id, query.data, query.length, true)
            : motor_bus.sendStd(static_cast<uint16_t>(query.id), query.data, query.length, true);
        if (!sent) ++motor_scan_tx_failed;
    }
    controller.flushParameters();

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

    static uint32_t last_current_diagnostic_ms = 0;
    if (now - last_current_diagnostic_ms >= 100) {
        last_current_diagnostic_ms = now;
        char text[96];
        std::snprintf(text, sizeof(text), "C620_DIAG cmd_ma=%d actual_ma=%ld rpm=%d",
            static_cast<int>(controller.c620CommandMilliAmp()),
            static_cast<long>(controller.c620CurrentMilliAmp()),
            static_cast<int>(controller.c620Rpm()));
        sendText(text);
    }

    static uint32_t last_discovery_ms = 0;
    if (now - last_discovery_ms >= 1000) {
        last_discovery_ms = now;
        char text[64];
        std::snprintf(text, sizeof(text), "C620_SCAN mask=%u configured_id=%u",
                      static_cast<unsigned>(c620_discovery.mask(now)),
                      static_cast<unsigned>(controller.parameters().getU8(domain::ParamId::C620EscId)));
        sendText(text);
        sendCanStat(1, motor_bus, motor_bus_ready);
        char rx_text[128];
        std::snprintf(rx_text, sizeof(rx_text),
            "MOTOR_RX std=%lu ext=%lu last_std=%lu last_ext=%08lX scan_done=%u scan_cancel=%u tx_failed=%lu",
            static_cast<unsigned long>(motor_standard_count),
            static_cast<unsigned long>(motor_extended_count),
            static_cast<unsigned long>(motor_last_standard),
            static_cast<unsigned long>(motor_last_extended), motor_discovery.done() ? 1U : 0U,
            motor_discovery.cancelled() ? 1U : 0U,
            static_cast<unsigned long>(motor_scan_tx_failed));
        sendText(rx_text);
        char scan_text[96];
        std::snprintf(scan_text, sizeof(scan_text), "MOTOR_SCAN_TX el05_done=%lu dm_done=%lu",
            static_cast<unsigned long>(motor_scan_el05_completed),
            static_cast<unsigned long>(motor_scan_dm_completed));
        sendText(scan_text);
    }

    uint16_t el05_index = 0;
    uint32_t el05_raw = 0;
    if (controller.takeEl05ParameterReply(el05_index, el05_raw)) {
        char text[96];
        std::snprintf(text, sizeof(text),
                      "EL05_PARAM index=%04X raw=%08lX state=%u fault=%u",
                      static_cast<unsigned>(el05_index), static_cast<unsigned long>(el05_raw),
                      static_cast<unsigned>(controller.el05State()),
                      static_cast<unsigned>(controller.errorBits(0)));
        sendText(text);
    }

    // DMのレジスタ応答は非同期に届く。届いた時点で1行返す。
    uint8_t reg_id = 0;
    uint32_t reg_raw = 0;
    if (controller.takeDmRegisterReply(reg_id, reg_raw)) {
        float as_float = 0.0f;
        std::memcpy(&as_float, &reg_raw, sizeof(as_float));
        char value[32];
        domain::formatFixed3(as_float, value, sizeof(value));
        char text[96];
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

#include "actuator_controller.hpp"
#include "can_bus.hpp"
#include "device_config.hpp"
#include "domain/can_frame.hpp"
#include "domain/command.hpp"
#include "domain/command_queue.hpp"
#include "domain/line_reader.hpp"
#include "domain/telemetry.hpp"
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

bool usbReady() {
    const auto* cdc = static_cast<const USBD_CDC_HandleTypeDef*>(hUsbDeviceFS.pClassData);
    return hUsbDeviceFS.dev_state == USBD_STATE_CONFIGURED && cdc != nullptr &&
           cdc->TxState == 0;
}

void sendText(const char* text) {
    if (!usbReady()) return;
    static uint8_t buffer[96];
    const std::size_t length = std::strlen(text);
    if (length + 1 > sizeof(buffer)) return;
    std::memcpy(buffer, text, length);
    buffer[length] = '\n';
    CDC_Transmit_FS(buffer, static_cast<uint16_t>(length + 1));
}

void applyCommand(const domain::Command& command) {
    switch (command.kind) {
        case domain::CommandKind::Hello:
            protocol_ready = command.protocol_version == 1;
            sendText(protocol_ready ? "DEVICE protocol=1 board=cctl slots=3 can=2 watchdog_ms=250"
                                    : "ERR code=BAD_VERSION");
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
    if (!usbReady()) return;
    static uint8_t line[domain::TELEMETRY_LINE_CAPACITY + 1];
    domain::Telemetry telemetry;
    telemetry.uptime_ms = HAL_GetTick();
    for (uint8_t slot = 0; slot < domain::SLOT_COUNT; ++slot) {
        telemetry.targets[slot] = controller.target(slot);
        telemetry.measured[slot] = controller.measured(slot);
    }
    telemetry.enabled_slots = controller.enabledSlots();
    telemetry.mode = controller.mode();
    telemetry.error_bits = controller.errorBits();

    const std::size_t length = domain::formatTelemetry(
        telemetry, reinterpret_cast<char*>(line), domain::TELEMETRY_LINE_CAPACITY);
    if (length == 0) return;
    line[length] = '\n';
    CDC_Transmit_FS(line, static_cast<uint16_t>(length + 1));
}
}  // namespace

extern "C" void setup(void) {
    ui.begin();
    HAL_TIM_Base_Start_IT(&htim2);
    motor_bus.begin();
    peripheral_bus_ready = peripheral_bus.begin();
    controller.begin();
    last_contact_ms = HAL_GetTick();
}

extern "C" void loop(void) {
    domain::CanFrame frame;
    while (motor_bus.receive(frame)) controller.dispatchRx(frame);
    while (peripheral_bus.receive(frame)) sendCanFrame(frame);

    domain::Command command;
    while (commands.pop(command)) applyCommand(command);

    const uint32_t now = HAL_GetTick();
    if (controller.mode() == domain::RunMode::Run &&
        now - last_contact_ms > config::period::WATCHDOG_MS) {
        controller.setMode(domain::RunMode::Stop);
        protocol_ready = false;
    }

    controller.update();

    static uint32_t last_lcd_ms = 0;
    if (now - last_lcd_ms >= config::period::LCD_MS) {
        last_lcd_ms = now;
        ui.showStatus(controller.target(0), controller.target(1), controller.target(2),
                      controller.errorBits());
    }

    static uint32_t last_telemetry_ms = 0;
    if (now - last_telemetry_ms >= config::period::TELEMETRY_MS) {
        last_telemetry_ms = now;
        sendTelemetry();
    }
}

extern "C" void cctl_usbcdc_receive(const uint8_t* data, uint32_t length) {
    for (uint32_t i = 0; i < length; ++i) {
        if (!usb_line.push(static_cast<char>(data[i]))) continue;
        const domain::Command command = domain::parseCommand(usb_line.line(), usb_line.length());
        if (command.kind != domain::CommandKind::None && commands.push(command)) {
            last_contact_ms = HAL_GetTick();
        }
    }
}

extern "C" void HAL_TIM_PeriodElapsedCallback(TIM_HandleTypeDef* htim) {
    if (htim->Instance == TIM2) {
        static uint32_t tick_ms = 0;
        ++tick_ms;
        if (tick_ms % 20U == 0U) {
            ui.updateLeds(tick_ms, controller.mode(), controller.enabledSlots());
        }
    }
}

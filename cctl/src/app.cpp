#include "can_bus.hpp"
#include "domain/can_frame.hpp"
#include "domain/command.hpp"
#include "domain/command_queue.hpp"
#include "domain/line_reader.hpp"
#include "domain/stick_command.hpp"
#include "domain/telemetry.hpp"
#include "main.h"
#include "robot_config.hpp"
#include "rthetaz_controller.hpp"
#include "ui.hpp"
#include "usbd_cdc_if.h"

#include <algorithm>
#include <cstdint>

extern "C" {
extern FDCAN_HandleTypeDef hfdcan1;  // モータ用バス
extern I2C_HandleTypeDef hi2c1;
extern TIM_HandleTypeDef htim15;
extern TIM_HandleTypeDef htim2;
}

namespace {
Ui ui(&hi2c1, &htim15);
CanBus motor_bus(&hfdcan1);
RThetaZController controller(motor_bus);

domain::LineReader usb_line;
domain::CommandQueue commands;

// USB 受信は割込みコンテキストなので、指令はここで受け渡す。
// 連番を進めることで、loop 側は未処理の指令だけを取り込める。
volatile int8_t pending_lx_percent = 0;
volatile int8_t pending_ly_percent = 0;
volatile uint32_t pending_input_sequence = 0;

void updateManualControl() {
    static uint32_t consumed_sequence = 0;
    static uint32_t last_input_ms = 0;

    const uint32_t sequence = pending_input_sequence;
    if (sequence == consumed_sequence) {
        return;
    }

    const domain::StickCommand command = {pending_lx_percent, pending_ly_percent};
    consumed_sequence = sequence;

    const uint32_t now = HAL_GetTick();
    if (last_input_ms == 0) {
        last_input_ms = now;
        return;
    }

    // 通信が途切れていた時間分を一度に移動しないよう、間隔に上限を設ける。
    const uint32_t interval_ms = std::min(now - last_input_ms,
        config::manual_control::MAX_INPUT_INTERVAL_MS);
    last_input_ms = now;

    // 運転中以外は目標値を動かさない。復帰した瞬間に飛ぶのを避ける。
    if (controller.mode() != domain::RunMode::Run) {
        return;
    }

    const domain::ManualDelta delta = domain::computeManualDelta(
        command, static_cast<float>(interval_ms) / 1000.0f,
        config::manual_control::STICK_DEADZONE,
        config::manual_control::R_SPEED_MM_S,
        config::manual_control::THETA_SPEED_DEG_S);

    controller.setTarget(controller.targetR() + delta.r_mm,
                         controller.targetTheta() + delta.theta_deg,
                         controller.targetZ());
}

void applyCommand(const domain::Command& command) {
    switch (command.kind) {
        case domain::CommandKind::Stop:
            controller.setMode(domain::RunMode::Stop);
            break;
        case domain::CommandKind::Run:
            controller.setMode(domain::RunMode::Run);
            break;
        case domain::CommandKind::Safe:
            controller.setMode(domain::RunMode::Safe);
            break;
        case domain::CommandKind::Enable:
            controller.setAxesEnabled(command.axes, command.value);
            break;
        case domain::CommandKind::Home:
            controller.home();
            break;
        case domain::CommandKind::Test:
            controller.enableTestSequence(command.value);
            break;
        case domain::CommandKind::None:
            break;
    }
}

void sendTelemetry() {
    // CDC_Transmit_FS は渡したバッファをそのまま参照する。
    // 送信完了まで生きている必要があるので static で持つ。
    static uint8_t line[domain::TELEMETRY_LINE_CAPACITY + 1];

    domain::Telemetry telemetry;
    telemetry.uptime_ms = HAL_GetTick();
    telemetry.r_target_mm = controller.targetR();
    telemetry.r_measured_mm = controller.measuredR();
    telemetry.theta_target_deg = controller.targetTheta();
    telemetry.theta_measured_deg = controller.measuredTheta();
    telemetry.z_target_mm = controller.targetZ();
    telemetry.z_measured_mm = controller.measuredZ();
    telemetry.enabled_axes = controller.enabledAxes();
    telemetry.mode = controller.mode();
    telemetry.error_bits = controller.errorBits();

    const std::size_t length = domain::formatTelemetry(
        telemetry, reinterpret_cast<char*>(line), domain::TELEMETRY_LINE_CAPACITY);
    if (length == 0) {
        return;
    }
    line[length] = '\n';

    // 前の送信が終わっていなければ今回は諦める。制御を待たせない。
    CDC_Transmit_FS(line, static_cast<uint16_t>(length + 1));
}
}  // namespace

extern "C" void setup(void) {
    ui.begin();

    HAL_TIM_Base_Start_IT(&htim2);

    motor_bus.begin();
    controller.begin();

    // DIP1 が ON のときだけ、電源投入で運転状態に入る。
    // 未設定（プルアップで HIGH）なら Safe のまま動かない。
    if (HAL_GPIO_ReadPin(DIP1_GPIO_Port, DIP1_Pin) == GPIO_PIN_RESET) {
        controller.setAxesEnabled(domain::axis_bit::ALL, true);
        controller.setMode(domain::RunMode::Run);
    }
}

extern "C" void loop(void) {
    // 受信フレームを全て取り込んで各モータへ振り分ける。
    domain::CanFrame frame;
    while (motor_bus.receive(frame)) {
        controller.dispatchRx(frame);
    }

    // 割込みで積まれた指令を処理する。CAN 送信を伴うのでここで実行する。
    domain::Command command;
    while (commands.pop(command)) {
        applyCommand(command);
    }

    updateManualControl();

    // 各軸の指令送信。
    controller.update();

    const uint32_t now = HAL_GetTick();

    // 状態表示（200ms 間隔）。
    static uint32_t last_lcd_ms = 0;
    if (now - last_lcd_ms >= config::period::LCD_MS) {
        last_lcd_ms = now;
        ui.showStatus(controller.targetR(), controller.targetTheta(), controller.targetZ(),
                      controller.errorBits());
    }

    // テレメトリ送信。
    static uint32_t last_telemetry_ms = 0;
    if (now - last_telemetry_ms >= config::period::TELEMETRY_MS) {
        last_telemetry_ms = now;
        sendTelemetry();
    }
}

extern "C" void cctl_usbcdc_receive(const uint8_t *data, uint32_t length) {
    for (uint32_t i = 0; i < length; ++i) {
        if (!usb_line.push(static_cast<char>(data[i]))) {
            continue;
        }

        // 指令と操作入力が同じリンクを流れる。先に指令として解釈を試みる。
        const domain::Command command =
            domain::parseCommand(usb_line.line(), usb_line.length());
        if (command.kind != domain::CommandKind::None) {
            commands.push(command);
            continue;
        }

        domain::StickCommand stick = {};
        if (domain::parseStickCommand(usb_line.line(), usb_line.length(), stick)) {
            pending_lx_percent = stick.lx_percent;
            pending_ly_percent = stick.ly_percent;
            ++pending_input_sequence;
        }
    }
}

extern "C" void HAL_TIM_PeriodElapsedCallback(TIM_HandleTypeDef *htim) {
    if (htim->Instance == TIM2) {
        static uint32_t tick_ms = 0;

        tick_ms++;
        if (tick_ms % 20U == 0U) {
            ui.updateLeds(tick_ms, controller.mode(), controller.enabledAxes());
        }
    }
}

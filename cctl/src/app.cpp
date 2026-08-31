#include "can_bus.hpp"
#include "domain/can_frame.hpp"
#include "domain/line_reader.hpp"
#include "domain/stick_command.hpp"
#include "main.h"
#include "robot_config.hpp"
#include "rthetaz_controller.hpp"
#include "ui.hpp"

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

    const domain::ManualDelta delta = domain::computeManualDelta(
        command, static_cast<float>(interval_ms) / 1000.0f,
        config::manual_control::STICK_DEADZONE,
        config::manual_control::R_SPEED_MM_S,
        config::manual_control::THETA_SPEED_DEG_S);

    controller.setTarget(controller.targetR() + delta.r_mm,
                         controller.targetTheta() + delta.theta_deg,
                         controller.targetZ());
}
}  // namespace

extern "C" void setup(void) {
    ui.begin();

    HAL_TIM_Base_Start_IT(&htim2);

    motor_bus.begin();
    controller.begin();

    controller.enableTestSequence(false);
}

extern "C" void loop(void) {
    // 受信フレームを全て取り込んで各モータへ振り分ける。
    domain::CanFrame frame;
    while (motor_bus.receive(frame)) {
        controller.dispatchRx(frame);
    }

    updateManualControl();

    // 各軸の指令送信。
    controller.update();

    // 状態表示（200ms 間隔）。
    static uint32_t last_lcd_ms = 0;
    const uint32_t now = HAL_GetTick();
    if (now - last_lcd_ms >= config::period::LCD_MS) {
        last_lcd_ms = now;
        ui.showStatus(controller.targetR(), controller.targetTheta(), controller.targetZ(),
                      controller.z().errorState());
    }
}

extern "C" void cctl_usbcdc_receive(const uint8_t *data, uint32_t length) {
    for (uint32_t i = 0; i < length; ++i) {
        if (!usb_line.push(static_cast<char>(data[i]))) {
            continue;
        }

        domain::StickCommand command = {};
        if (domain::parseStickCommand(usb_line.line(), usb_line.length(), command)) {
            pending_lx_percent = command.lx_percent;
            pending_ly_percent = command.ly_percent;
            ++pending_input_sequence;
        }
    }
}

extern "C" void HAL_TIM_PeriodElapsedCallback(TIM_HandleTypeDef *htim) {
    if (htim->Instance == TIM2) {
        static uint32_t tick_ms = 0;

        tick_ms++;
        if (tick_ms % 100U == 0U) {
            ui.updateLeds(tick_ms);
        }
    }
}

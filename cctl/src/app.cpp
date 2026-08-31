#include "can_bus.hpp"
#include "lcd_aqm1602.h"
#include "main.h"
#include "robot_config.hpp"
#include "rthetaz_controller.hpp"

#include <algorithm>
#include <cstdint>
#include <cstdio>

extern "C" {
extern FDCAN_HandleTypeDef hfdcan1;  // モータ用バス
extern I2C_HandleTypeDef hi2c1;
extern TIM_HandleTypeDef htim15;
extern TIM_HandleTypeDef htim2;
}

namespace {
Aqm1602 lcd(&hi2c1);
CanBus motor_bus(&hfdcan1);
RThetaZController controller(motor_bus);

constexpr uint32_t kUsbLineCapacity = 96;
char usb_line[kUsbLineCapacity] = {};
uint32_t usb_line_length = 0;
bool usb_line_overflow = false;

volatile int8_t pending_lx_percent = 0;
volatile int8_t pending_ly_percent = 0;
volatile uint32_t pending_input_sequence = 0;

bool parsePercent(const char *text, int8_t &value) {
    if ((text[0] != '+' && text[0] != '-') ||
        text[1] < '0' || text[1] > '9' ||
        text[2] < '0' || text[2] > '9' ||
        text[3] < '0' || text[3] > '9') {
        return false;
    }

    int parsed = (text[1] - '0') * 100 + (text[2] - '0') * 10 + (text[3] - '0');
    if (text[0] == '-') {
        parsed = -parsed;
    }
    if (parsed < -100 || parsed > 100) {
        return false;
    }

    value = static_cast<int8_t>(parsed);
    return true;
}

void publishControllerLine(const char *line, uint32_t length) {
    // host の形式: "LX+000 LY+000 ..."。手動操作に必要な2軸だけを取り込む。
    if (length < 13 || line[0] != 'L' || line[1] != 'X' ||
        line[6] != ' ' || line[7] != 'L' || line[8] != 'Y') {
        return;
    }

    int8_t lx = 0;
    int8_t ly = 0;
    if (!parsePercent(&line[2], lx) || !parsePercent(&line[9], ly)) {
        return;
    }

    pending_lx_percent = lx;
    pending_ly_percent = ly;
    ++pending_input_sequence;
}

float applyDeadzone(float value) {
    const float magnitude = value < 0.0f ? -value : value;
    if (magnitude <= config::manual_control::STICK_DEADZONE) {
        return 0.0f;
    }

    const float scaled = (magnitude - config::manual_control::STICK_DEADZONE) /
                         (1.0f - config::manual_control::STICK_DEADZONE);
    return value < 0.0f ? -scaled : scaled;
}

void updateManualControl() {
    static uint32_t consumed_sequence = 0;
    static uint32_t last_input_ms = 0;

    const uint32_t sequence = pending_input_sequence;
    if (sequence == consumed_sequence) {
        return;
    }

    const float lx = static_cast<float>(pending_lx_percent) / 100.0f;
    const float ly = static_cast<float>(pending_ly_percent) / 100.0f;
    consumed_sequence = sequence;

    const uint32_t now = HAL_GetTick();
    if (last_input_ms == 0) {
        last_input_ms = now;
        return;
    }

    const uint32_t interval_ms = std::min(now - last_input_ms,
        config::manual_control::MAX_INPUT_INTERVAL_MS);
    last_input_ms = now;
    const float interval_s = static_cast<float>(interval_ms) / 1000.0f;

    const float r_delta = applyDeadzone(ly) *
        config::manual_control::R_SPEED_MM_S * interval_s;
    const float theta_delta = applyDeadzone(lx) *
        config::manual_control::THETA_SPEED_DEG_S * interval_s;
    controller.setTarget(controller.targetR() + r_delta,
                         controller.targetTheta() + theta_delta,
                         controller.targetZ());
}

void playTone(uint32_t frequency_hz, uint32_t duration_ms) {
    const uint32_t period = (1000000U / frequency_hz) - 1U;

    __HAL_TIM_SET_AUTORELOAD(&htim15, period);
    __HAL_TIM_SET_COMPARE(&htim15, TIM_CHANNEL_2, (period + 1U) / 2U);

    HAL_TIM_PWM_Start(&htim15, TIM_CHANNEL_2);
    HAL_Delay(duration_ms);
    HAL_TIM_PWM_Stop(&htim15, TIM_CHANNEL_2);
}

void showStartupLcd() {
    lcd.clear();
    lcd.setCursor(0, 0);
    lcd.print("DRC-CCTL2026");
    lcd.setCursor(0, 1);
    lcd.print("rtheta-z ctrl");
}

void updateStatusLcd() {
    char line0[17] = {};
    char line1[17] = {};

    std::snprintf(line0, sizeof(line0), "r%-4d th%-4d",
                  static_cast<int>(controller.targetR()),
                  static_cast<int>(controller.targetTheta()));
    std::snprintf(line1, sizeof(line1), "z%-4d dmE%X",
                  static_cast<int>(controller.targetZ()),
                  controller.z().errorState() & 0x0F);

    lcd.clear();
    lcd.setCursor(0, 0);
    lcd.print(line0);
    lcd.setCursor(0, 1);
    lcd.print(line1);
}

void updateLedPattern(uint32_t tick_ms) {
    const auto cycle_ms = tick_ms % 300U;
    const bool led1_on = cycle_ms < 100U || cycle_ms >= 285U;
    const bool led2_on = cycle_ms >= 85U && cycle_ms < 200U;
    const bool led3_on = cycle_ms >= 185U && cycle_ms < 300U;

    HAL_GPIO_WritePin(LED1_GPIO_Port, LED1_Pin, led1_on ? GPIO_PIN_SET : GPIO_PIN_RESET);
    HAL_GPIO_WritePin(LED2_GPIO_Port, LED2_Pin, led2_on ? GPIO_PIN_SET : GPIO_PIN_RESET);
    HAL_GPIO_WritePin(LED3_GPIO_Port, LED3_Pin, led3_on ? GPIO_PIN_SET : GPIO_PIN_RESET);
}
}  // namespace

extern "C" void setup(void) {
    lcd.begin();
    showStartupLcd();

    playTone(988, 80);
    playTone(1319, 120);

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
        updateStatusLcd();
    }
}

extern "C" void cctl_usbcdc_receive(const uint8_t *data, uint32_t length) {
    for (uint32_t i = 0; i < length; ++i) {
        const char ch = static_cast<char>(data[i]);
        if (ch == '\n') {
            if (!usb_line_overflow) {
                uint32_t line_length = usb_line_length;
                if (line_length > 0 && usb_line[line_length - 1] == '\r') {
                    --line_length;
                }
                publishControllerLine(usb_line, line_length);
            }
            usb_line_length = 0;
            usb_line_overflow = false;
        } else if (!usb_line_overflow) {
            if (usb_line_length < kUsbLineCapacity) {
                usb_line[usb_line_length++] = ch;
            } else {
                usb_line_overflow = true;
            }
        }
    }
}

extern "C" void HAL_TIM_PeriodElapsedCallback(TIM_HandleTypeDef *htim) {
    if (htim->Instance == TIM2) {
        static uint32_t tick_ms = 0;

        tick_ms++;
        if (tick_ms % 100U == 0U) {
            updateLedPattern(tick_ms);
        }
    }
}

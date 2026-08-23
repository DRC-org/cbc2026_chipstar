#include "can_bus.hpp"
#include "lcd_aqm1602.h"
#include "main.h"
#include "robot_config.hpp"
#include "rthetaz_controller.hpp"

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

    // 指令I/F 未接続のため、単体動作確認用の内蔵テストシーケンスを有効化する。
    controller.enableTestSequence(true);
}

extern "C" void loop(void) {
    // 受信フレームを全て取り込んで各モータへ振り分ける。
    FDCAN_RxHeaderTypeDef rx_header = {};
    uint8_t rx_data[8] = {};
    while (motor_bus.receive(rx_header, rx_data)) {
        controller.dispatchRx(rx_header, rx_data);
    }

    // 各軸の指令送信・テスト動作。
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
    // 上位指令I/F は未定。将来ここで r/θ/z 目標値を受信して controller.setTarget を呼ぶ。
    (void)data;
    (void)length;
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

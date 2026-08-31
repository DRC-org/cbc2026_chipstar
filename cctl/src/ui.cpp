#include "ui.hpp"

#include <cstdio>

void Ui::playTone(uint32_t frequency_hz, uint32_t duration_ms) {
  // TIM15 のカウントは 1MHz。周期から分周後のカウント値を求める。
  const uint32_t period = (1000000U / frequency_hz) - 1U;

  __HAL_TIM_SET_AUTORELOAD(buzzer_tim_, period);
  __HAL_TIM_SET_COMPARE(buzzer_tim_, TIM_CHANNEL_2, (period + 1U) / 2U);

  HAL_TIM_PWM_Start(buzzer_tim_, TIM_CHANNEL_2);
  HAL_Delay(duration_ms);
  HAL_TIM_PWM_Stop(buzzer_tim_, TIM_CHANNEL_2);
}

void Ui::begin() {
  lcd_.begin();
  lcd_.clear();
  lcd_.setCursor(0, 0);
  lcd_.print("DRC-CCTL2026");
  lcd_.setCursor(0, 1);
  lcd_.print("rtheta-z ctrl");

  playTone(988, 80);
  playTone(1319, 120);
}

void Ui::showStatus(float r_mm, float theta_deg, float z_mm, uint8_t z_error) {
  char line0[17] = {};
  char line1[17] = {};

  std::snprintf(line0, sizeof(line0), "r%-4d th%-4d", static_cast<int>(r_mm),
                static_cast<int>(theta_deg));
  std::snprintf(line1, sizeof(line1), "z%-4d dmE%X", static_cast<int>(z_mm),
                z_error & 0x0F);

  lcd_.clear();
  lcd_.setCursor(0, 0);
  lcd_.print(line0);
  lcd_.setCursor(0, 1);
  lcd_.print(line1);
}

void Ui::updateLeds(uint32_t tick_ms) {
  const auto cycle_ms = tick_ms % 300U;
  const bool led1_on = cycle_ms < 100U || cycle_ms >= 285U;
  const bool led2_on = cycle_ms >= 85U && cycle_ms < 200U;
  const bool led3_on = cycle_ms >= 185U && cycle_ms < 300U;

  HAL_GPIO_WritePin(LED1_GPIO_Port, LED1_Pin, led1_on ? GPIO_PIN_SET : GPIO_PIN_RESET);
  HAL_GPIO_WritePin(LED2_GPIO_Port, LED2_Pin, led2_on ? GPIO_PIN_SET : GPIO_PIN_RESET);
  HAL_GPIO_WritePin(LED3_GPIO_Port, LED3_Pin, led3_on ? GPIO_PIN_SET : GPIO_PIN_RESET);
}

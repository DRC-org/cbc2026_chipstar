#include "ui.hpp"

#include "domain/led_pattern.hpp"

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
  lcd_.print("actuator device");

  playTone(988, 80);
  playTone(1319, 120);
}

void Ui::showStatus(float slot0, float slot1, float slot2, uint8_t error) {
  char line0[17] = {};
  char line1[17] = {};

  std::snprintf(line0, sizeof(line0), "0:%-4d 1:%-4d", static_cast<int>(slot0),
                static_cast<int>(slot1));
  std::snprintf(line1, sizeof(line1), "2:%-4d err:%X", static_cast<int>(slot2), error & 0x0F);

  lcd_.clear();
  lcd_.setCursor(0, 0);
  lcd_.print(line0);
  lcd_.setCursor(0, 1);
  lcd_.print(line1);
}

void Ui::updateLeds(uint32_t tick_ms, domain::RunMode mode, uint8_t enabled_slots) {
  const uint8_t bits = domain::ledPattern(tick_ms, mode, enabled_slots);

  const auto state = [bits](uint8_t bit) {
    return (bits & bit) != 0 ? GPIO_PIN_SET : GPIO_PIN_RESET;
  };

  HAL_GPIO_WritePin(LED1_GPIO_Port, LED1_Pin, state(domain::slot_bit::SLOT0));
  HAL_GPIO_WritePin(LED2_GPIO_Port, LED2_Pin, state(domain::slot_bit::SLOT1));
  HAL_GPIO_WritePin(LED3_GPIO_Port, LED3_Pin, state(domain::slot_bit::SLOT2));
}

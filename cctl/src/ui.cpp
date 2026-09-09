#include "ui.hpp"


#include <cstring>

void Ui::begin() {
  lcd_.begin();
}

void Ui::update(uint32_t now, const domain::IndicatorState& state) {
  const auto frame = domain::indicatorFrame(state);
  const uint32_t frequency = tone_.update(now, state, frame.alarm);
  if (frequency != frequency_) {
    HAL_TIM_PWM_Stop(buzzer_tim_, TIM_CHANNEL_2);
    if (frequency) {
      const uint32_t period = 1000000U / frequency - 1U;
      __HAL_TIM_SET_AUTORELOAD(buzzer_tim_, period);
      __HAL_TIM_SET_COUNTER(buzzer_tim_, 0);
      __HAL_TIM_SET_COMPARE(buzzer_tim_, TIM_CHANNEL_2, (period + 1U) / 2U);
      HAL_TIM_PWM_Start(buzzer_tim_, TIM_CHANNEL_2);
    }
    frequency_ = frequency;
  }

  if (lcd_failed_ && now - lcd_retry_ms_ < 1000) return;
  // 一度に1文字だけ更新。長い行の送信で制御周期を占有しない。
  for (uint8_t count = 0; count < 32; ++count) {
    const uint8_t row = cursor_ / 16;
    const uint8_t col = cursor_ % 16;
    cursor_ = (cursor_ + 1) % 32;
    if (displayed_[row][col] == frame.lines[row][col]) continue;
    lcd_.setCursor(col, row);
    if (lcd_.healthy()) lcd_.write(frame.lines[row][col]);
    if (!lcd_.healthy()) {
      std::memset(displayed_, 0, sizeof(displayed_));
      lcd_failed_ = true;
      lcd_retry_ms_ = now;
      return;
    }
    lcd_failed_ = false;
    displayed_[row][col] = frame.lines[row][col];
    break;
  }
}

void Ui::updateLeds(uint32_t tick_ms, domain::Status status) {
  const uint8_t bits = domain::statusPattern(tick_ms, status, 3);

  const auto state = [bits](uint8_t index) {
    return (bits & (1U << index)) != 0 ? GPIO_PIN_SET : GPIO_PIN_RESET;
  };

  HAL_GPIO_WritePin(LED1_GPIO_Port, LED1_Pin, state(0));
  HAL_GPIO_WritePin(LED2_GPIO_Port, LED2_Pin, state(1));
  HAL_GPIO_WritePin(LED3_GPIO_Port, LED3_Pin, state(2));
}

#include "main.h"

#include <cstdint>

extern "C" {
extern TIM_HandleTypeDef htim15;
}

namespace {
void playTone(uint32_t frequency_hz, uint32_t duration_ms) {
  const uint32_t period = (1000000U / frequency_hz) - 1U;

  __HAL_TIM_SET_AUTORELOAD(&htim15, period);
  __HAL_TIM_SET_COMPARE(&htim15, TIM_CHANNEL_2, (period + 1U) / 2U);

  HAL_TIM_PWM_Start(&htim15, TIM_CHANNEL_2);
  HAL_Delay(duration_ms);
  HAL_TIM_PWM_Stop(&htim15, TIM_CHANNEL_2);
}
}

extern "C" void setup(void) {
  playTone(988, 80);
  playTone(1319, 120);
}

extern "C" void loop(void) {}

extern "C" void cctl_usbcdc_receive(const uint8_t *data, uint32_t length) {
  (void)data;
  (void)length;
}

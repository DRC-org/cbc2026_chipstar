#include "main.h"

#include <cstdint>

namespace {
constexpr uint32_t ON_TIME_MS = 400;
constexpr uint32_t OFF_GAP_MS = 100;
constexpr uint32_t ALL_ON_TIME_MS = 600;
constexpr uint32_t CYCLE_GAP_MS = 1000;

struct LedOutput {
  GPIO_TypeDef* port;
  uint16_t pin;
};

constexpr LedOutput LEDS[] = {
    {LED1_GPIO_Port, LED1_Pin}, {LED2_GPIO_Port, LED2_Pin}, {LED3_GPIO_Port, LED3_Pin},
    {LED4_GPIO_Port, LED4_Pin}, {LED5_GPIO_Port, LED5_Pin}, {LED6_GPIO_Port, LED6_Pin},
};

void setAll(GPIO_PinState state) {
  HAL_GPIO_WritePin(GPIOA, LED1_Pin | LED2_Pin | LED3_Pin | LED4_Pin, state);
  HAL_GPIO_WritePin(GPIOF, LED5_Pin | LED6_Pin, state);
}
}  // namespace

extern "C" void setup(void) {
  setAll(GPIO_PIN_RESET);
}

extern "C" void loop(void) {
  for (const LedOutput& led : LEDS) {
    HAL_GPIO_WritePin(led.port, led.pin, GPIO_PIN_SET);
    HAL_Delay(ON_TIME_MS);
    HAL_GPIO_WritePin(led.port, led.pin, GPIO_PIN_RESET);
    HAL_Delay(OFF_GAP_MS);
  }

  setAll(GPIO_PIN_SET);
  HAL_Delay(ALL_ON_TIME_MS);
  setAll(GPIO_PIN_RESET);
  HAL_Delay(CYCLE_GAP_MS);
}

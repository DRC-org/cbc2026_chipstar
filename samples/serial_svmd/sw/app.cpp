#include "main.h"

#include <cstdint>

namespace {
constexpr uint32_t POLL_INTERVAL_MS = 1;

struct SwitchLedPair {
  GPIO_TypeDef* switch_port;
  uint16_t switch_pin;
  GPIO_TypeDef* led_port;
  uint16_t led_pin;
};

constexpr SwitchLedPair PAIRS[] = {
    {SW1_GPIO_Port, SW1_Pin, LED1_GPIO_Port, LED1_Pin},
    {SW2_GPIO_Port, SW2_Pin, LED2_GPIO_Port, LED2_Pin},
    {SW3_GPIO_Port, SW3_Pin, LED3_GPIO_Port, LED3_Pin},
    {SW4_GPIO_Port, SW4_Pin, LED4_GPIO_Port, LED4_Pin},
    {SW5_GPIO_Port, SW5_Pin, LED5_GPIO_Port, LED5_Pin},
    {SW6_GPIO_Port, SW6_Pin, LED6_GPIO_Port, LED6_Pin},
};
}  // namespace

extern "C" void setup(void) {
  HAL_GPIO_WritePin(GPIOA, LED1_Pin | LED2_Pin | LED3_Pin | LED4_Pin, GPIO_PIN_RESET);
  HAL_GPIO_WritePin(GPIOF, LED5_Pin | LED6_Pin, GPIO_PIN_RESET);
}

extern "C" void loop(void) {
  // スイッチはプルアップ入力。押下で LOW になる。
  for (const SwitchLedPair& pair : PAIRS) {
    const bool pressed =
        HAL_GPIO_ReadPin(pair.switch_port, pair.switch_pin) == GPIO_PIN_RESET;
    HAL_GPIO_WritePin(pair.led_port, pair.led_pin,
                      pressed ? GPIO_PIN_SET : GPIO_PIN_RESET);
  }

  HAL_Delay(POLL_INTERVAL_MS);
}

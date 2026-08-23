#include "main.h"

#include <cstdint>

namespace {
bool sw1_state = false;
bool sw2_state = false;
bool sw3_state = false;
}

extern "C" void setup(void) {}

extern "C" void loop(void) {
  sw1_state = HAL_GPIO_ReadPin(SW1_GPIO_Port, SW1_Pin) == GPIO_PIN_RESET;
  sw2_state = HAL_GPIO_ReadPin(SW2_GPIO_Port, SW2_Pin) == GPIO_PIN_RESET;
  sw3_state = HAL_GPIO_ReadPin(SW3_GPIO_Port, SW3_Pin) == GPIO_PIN_RESET;
  HAL_Delay(200);
}

extern "C" void cctl_usbcdc_receive(const uint8_t *data, uint32_t length) {
  (void)data;
  (void)length;
}

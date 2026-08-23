#include "main.h"

#include <cstdint>

namespace {
uint8_t dip_value = 0;
}

extern "C" void setup(void) {}

extern "C" void loop(void) {
  dip_value = ((HAL_GPIO_ReadPin(DIP1_GPIO_Port, DIP1_Pin) == GPIO_PIN_RESET) ? 0x01 : 0x00) |
              ((HAL_GPIO_ReadPin(DIP2_GPIO_Port, DIP2_Pin) == GPIO_PIN_RESET) ? 0x02 : 0x00) |
              ((HAL_GPIO_ReadPin(DIP3_GPIO_Port, DIP3_Pin) == GPIO_PIN_RESET) ? 0x04 : 0x00) |
              ((HAL_GPIO_ReadPin(DIP4_GPIO_Port, DIP4_Pin) == GPIO_PIN_RESET) ? 0x08 : 0x00);
  HAL_Delay(200);
}

extern "C" void cctl_usbcdc_receive(const uint8_t *data, uint32_t length) {
  (void)data;
  (void)length;
}

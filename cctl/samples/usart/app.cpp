#include "main.h"

#include <cstdint>
#include <cstring>

extern "C" {
extern UART_HandleTypeDef huart3;
}

extern "C" void setup(void) {
  static const char banner[] = "USART3 echo\r\n";
  HAL_UART_Transmit(&huart3, reinterpret_cast<const uint8_t*>(banner), sizeof(banner) - 1U,
                    HAL_MAX_DELAY);
}

extern "C" void loop(void) {
  uint8_t ch = 0;
  if (HAL_UART_Receive(&huart3, &ch, 1, 10) == HAL_OK) {
    HAL_UART_Transmit(&huart3, &ch, 1, HAL_MAX_DELAY);
  }
}

extern "C" void cctl_usbcdc_receive(const uint8_t *data, uint32_t length) {
  (void)data;
  (void)length;
}

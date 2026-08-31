#include "main.h"

#include <cstdint>

extern "C" {
extern I2C_HandleTypeDef hi2c4;
}

namespace {
uint8_t detected_addresses[112] = {};
uint8_t detected_count = 0;
}

extern "C" void setup(void) {
  detected_count = 0;
  for (uint8_t address = 0x08; address <= 0x77; ++address) {
    if (HAL_I2C_IsDeviceReady(&hi2c4, static_cast<uint16_t>(address << 1), 1, 10) == HAL_OK) {
      detected_addresses[detected_count++] = address;
    }
  }
}

extern "C" void loop(void) {}

extern "C" void cctl_usbcdc_receive(const uint8_t *data, uint32_t length) {
  (void)data;
  (void)length;
}

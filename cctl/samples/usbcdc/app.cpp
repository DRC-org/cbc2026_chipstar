#include "main.h"
#include "usbd_cdc_if.h"

#include <cstddef>
#include <cstdint>
#include <cstring>

extern "C" void setup(void) {}

extern "C" void loop(void) {}

extern "C" void cctl_usbcdc_receive(const uint8_t *data, uint32_t length) {
  if (data == nullptr || length == 0U) {
    return;
  }

  static uint8_t tx_buffer[64];
  const uint16_t tx_length = static_cast<uint16_t>(
      length < sizeof(tx_buffer) ? length : sizeof(tx_buffer));
  std::memcpy(tx_buffer, data, tx_length);
  (void)CDC_Transmit_FS(tx_buffer, tx_length);
}

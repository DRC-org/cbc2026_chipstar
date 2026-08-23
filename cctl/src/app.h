#pragma once

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

void setup(void);

void loop(void);

void cctl_usbcdc_receive(const uint8_t *data, uint32_t length);

#ifdef __cplusplus
}
#endif

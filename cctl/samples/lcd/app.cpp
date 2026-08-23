#include "main.h"
#include "lcd_aqm1602.h"

#include <cstdint>

extern "C" {
extern I2C_HandleTypeDef hi2c1;
}

namespace {
Aqm1602 lcd(&hi2c1);
}

extern "C" void setup(void) {
  lcd.begin();
  lcd.clear();
  lcd.setCursor(0, 0);
  lcd.print("DRC-CCTL2026");
  lcd.setCursor(0, 1);
  lcd.print("LCD example");
}

extern "C" void loop(void) {}

extern "C" void cctl_usbcdc_receive(const uint8_t *data, uint32_t length) {
  (void)data;
  (void)length;
}

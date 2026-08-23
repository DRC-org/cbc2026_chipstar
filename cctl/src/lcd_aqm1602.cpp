#include "lcd_aqm1602.h"

Aqm1602::Aqm1602(I2C_HandleTypeDef* i2c, uint8_t address) : i2c_(i2c), address_(address) {}

bool Aqm1602::begin() {
  HAL_Delay(40);

  sendCommand(0x38);
  sendCommand(0x39);
  sendCommand(0x14);
  sendCommand(0x7F);
  sendCommand(0x5E);
  sendCommand(0x6C);
  HAL_Delay(200);
  sendCommand(0x38);
  sendCommand(0x0C);
  sendCommand(0x01);
  HAL_Delay(2);
  sendCommand(0x06);
  return true;
}

void Aqm1602::clear() {
  sendCommand(0x01);
  HAL_Delay(2);
}

void Aqm1602::home() {
  sendCommand(0x02);
  HAL_Delay(2);
}

void Aqm1602::setCursor(uint8_t col, uint8_t row) {
  static constexpr uint8_t kRowOffsets[] = {0x00, 0x40};
  const uint8_t row_index = row > 1 ? 1 : row;
  sendCommand(static_cast<uint8_t>(0x80 | (kRowOffsets[row_index] + col)));
}

void Aqm1602::print(const char* text) {
  while (*text != '\0') {
    sendData(static_cast<uint8_t>(*text));
    ++text;
  }
}

void Aqm1602::sendCommand(uint8_t value) {
  writeByte(0x00, value);
}

void Aqm1602::sendData(uint8_t value) {
  writeByte(0x40, value);
}

void Aqm1602::writeByte(uint8_t control, uint8_t value) {
  uint8_t packet[2] = {control, value};
  HAL_I2C_Master_Transmit(i2c_, static_cast<uint16_t>(address_ << 1), packet, 2, HAL_MAX_DELAY);
}

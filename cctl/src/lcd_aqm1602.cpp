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

void Aqm1602::write(char value) {
  sendData(static_cast<uint8_t>(value));
}

void Aqm1602::setDisplay(bool display_on, bool cursor_on, bool blink_on) {
  // Display ON/OFF control: 0x08 | D<<2 | C<<1 | B
  uint8_t command = 0x08;
  if (display_on) {
    command |= 0x04;
  }
  if (cursor_on) {
    command |= 0x02;
  }
  if (blink_on) {
    command |= 0x01;
  }
  sendCommand(command);
}

void Aqm1602::setEntryMode(bool increment, bool shift) {
  // Entry mode set: 0x04 | I/D<<1 | S
  uint8_t command = 0x04;
  if (increment) {
    command |= 0x02;
  }
  if (shift) {
    command |= 0x01;
  }
  sendCommand(command);
}

void Aqm1602::setContrast(uint8_t contrast) {
  // コントラストは拡張命令(IS=1)で、下位4bitと上位2bitに分けて書く。
  // 上位側の命令には昇圧回路(Bon)とアイコン表示(Ion)のビットが同居するので、
  // begin() の初期化と同じ 0x08|0x04 を保ったまま書き換える。
  const uint8_t value = contrast > 63 ? 63 : contrast;

  sendCommand(0x39);  // Function set: IS = 1
  sendCommand(static_cast<uint8_t>(0x70 | (value & 0x0F)));
  sendCommand(static_cast<uint8_t>(0x50 | 0x08 | 0x04 | ((value >> 4) & 0x03)));
  sendCommand(0x38);  // Function set: IS = 0
}

void Aqm1602::createChar(uint8_t index, const uint8_t rows[8]) {
  // CGRAM は 8 文字分。1 文字あたり 8 行 × 5bit。
  sendCommand(static_cast<uint8_t>(0x40 | ((index & 0x07) << 3)));
  for (uint8_t i = 0; i < 8; ++i) {
    sendData(static_cast<uint8_t>(rows[i] & 0x1F));
  }
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

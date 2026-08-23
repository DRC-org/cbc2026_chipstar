#pragma once

#include "main.h"

class Aqm1602 {
 public:
  explicit Aqm1602(I2C_HandleTypeDef* i2c, uint8_t address = 0x3E);

  bool begin();
  void clear();
  void home();
  void setCursor(uint8_t col, uint8_t row);
  void print(const char* text);

 private:
  void sendCommand(uint8_t value);
  void sendData(uint8_t value);
  void writeByte(uint8_t control, uint8_t value);

  I2C_HandleTypeDef* i2c_;
  uint8_t address_;
};

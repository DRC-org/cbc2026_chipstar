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
  void write(char value);

  // 表示・カーソル・ブリンクの on/off。
  void setDisplay(bool display_on, bool cursor_on, bool blink_on);

  // 文字を書いた後にカーソルを進めるか、画面をずらすか。
  void setEntryMode(bool increment, bool shift);

  // コントラスト 0..63。視認性は電源電圧と個体で変わるので現場で追い込む。
  void setContrast(uint8_t contrast);

  // 外字を登録する。index は 0..7、rows は各 5bit の 8 行。
  // 登録後に setCursor でカーソル位置を戻すこと。
  void createChar(uint8_t index, const uint8_t rows[8]);

 private:
  void sendCommand(uint8_t value);
  void sendData(uint8_t value);
  void writeByte(uint8_t control, uint8_t value);

  I2C_HandleTypeDef* i2c_;
  uint8_t address_;
};

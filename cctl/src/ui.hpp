#pragma once

#include "lcd_aqm1602.h"
#include "main.h"

#include <cstdint>

// LCD・ブザー・LED をまとめた表示系。
// 制御ロジックから表示の都合を切り離すために独立させている。
class Ui {
 public:
  Ui(I2C_HandleTypeDef* i2c, TIM_HandleTypeDef* buzzer_tim)
      : lcd_(i2c), buzzer_tim_(buzzer_tim) {}

  // LCD の初期化、起動画面の表示、起動音。
  void begin();

  // 目標値と z 軸のエラー状態を 2 行で表示する。
  void showStatus(float r_mm, float theta_deg, float z_mm, uint8_t z_error);

  // 3 つの LED を巡回させる。tick_ms は 1ms 周期の通し番号。
  void updateLeds(uint32_t tick_ms);

 private:
  void playTone(uint32_t frequency_hz, uint32_t duration_ms);

  Aqm1602 lcd_;
  TIM_HandleTypeDef* buzzer_tim_;
};

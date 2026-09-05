#pragma once

#include "domain/run_state.hpp"
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

  // slot 0..2 の目標値とエラー状態を2行で表示する。
  void showStatus(float slot0, float slot1, float slot2, uint8_t error);

  // 運転状態と有効なslotをLED1..3に映す。tick_msは1ms周期の通し番号。
  void updateLeds(uint32_t tick_ms, domain::RunMode mode, uint8_t enabled_slots);

 private:
  void playTone(uint32_t frequency_hz, uint32_t duration_ms);

  Aqm1602 lcd_;
  TIM_HandleTypeDef* buzzer_tim_;
};

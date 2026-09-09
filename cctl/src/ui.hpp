#pragma once

#include "domain/run_state.hpp"
#include "domain/indicator.hpp"
#include "domain/status_led.hpp"
#include "lcd_aqm1602.h"
#include "main.h"

#include <cstdint>

// LCD・ブザー・LED をまとめた表示系。
// 制御ロジックから表示の都合を切り離すために独立させている。
class Ui {
 public:
  Ui(I2C_HandleTypeDef* i2c, TIM_HandleTypeDef* buzzer_tim)
      : lcd_(i2c), buzzer_tim_(buzzer_tim) {}

  // LCD の初期化。表示と起動音はupdateから開始する。
  void begin();

  // 毎ループ呼び出す。LCDは1文字ずつ転送し、鳴動中も制御を継続する。
  void update(uint32_t now, const domain::IndicatorState& state);

  // 運転状態と有効なslotをLED1..3に映す。tick_msは1ms周期の通し番号。
  void updateLeds(uint32_t tick_ms, domain::Status status);

 private:
  domain::IndicatorTone tone_;
  uint32_t frequency_ = 0;
  uint32_t lcd_retry_ms_ = 0;
  bool lcd_failed_ = false;
  uint8_t cursor_ = 0;
  char displayed_[2][17] = {};

  Aqm1602 lcd_;
  TIM_HandleTypeDef* buzzer_tim_;
};

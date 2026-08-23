#pragma once

#include <cstdint>

// 汎用 PID 制御器。HAL_GetTick() を用いて経過時間 dt を自動計測する。
// 出力は ±out_limit にクランプし、飽和時は積分器を anti-windup する。
class Pid {
 public:
  Pid(float kp, float ki, float kd, float out_limit)
      : kp_(kp), ki_(ki), kd_(kd), out_limit_(out_limit) {}

  void setGains(float kp, float ki, float kd) {
    kp_ = kp;
    ki_ = ki;
    kd_ = kd;
  }

  void reset();

  // 目標値と計測値から出力を計算し、内部状態を更新する。
  float update(float target, float measured);

 private:
  float kp_;
  float ki_;
  float kd_;
  float out_limit_;

  float integral_ = 0.0f;
  float prev_error_ = 0.0f;
  uint32_t prev_tick_ = 0;
  bool first_ = true;
};

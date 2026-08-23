#include "pid.hpp"

#include "main.h"

#include <algorithm>

void Pid::reset() {
  integral_ = 0.0f;
  prev_error_ = 0.0f;
  first_ = true;
}

float Pid::update(float target, float measured) {
  const uint32_t now = HAL_GetTick();
  const float error = target - measured;

  // 初回、または dt=0 の場合は微分・積分を進めず P 項のみ。
  float dt = 0.0f;
  if (!first_) {
    dt = static_cast<float>(now - prev_tick_) / 1000.0f;
  }
  prev_tick_ = now;

  const float proportional = kp_ * error;

  float derivative = 0.0f;
  if (dt > 0.0f) {
    integral_ += ki_ * error * dt;
    derivative = kd_ * (error - prev_error_) / dt;
  }
  first_ = false;
  prev_error_ = error;

  const float raw = proportional + integral_ + derivative;
  const float clamped = std::clamp(raw, -out_limit_, out_limit_);

  // anti-windup: 出力飽和かつ誤差が同方向なら積分を戻す。
  if (raw != clamped && (raw * error > 0.0f)) {
    integral_ = 0.0f;
  }

  return clamped;
}

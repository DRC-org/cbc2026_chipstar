#include "m3508_motor.hpp"

#include "main.h"

#include <algorithm>

M3508Motor::M3508Motor(CanBus& bus, uint8_t esc_id, uint16_t feedback_base,
                       float pos_kp, float pos_ki, float pos_kd, float max_rpm,
                       float vel_kp, float vel_ki, float vel_kd, float max_current_ma)
    : bus_(bus),
      esc_id_(esc_id),
      feedback_base_(feedback_base),
      pos_pid_(pos_kp, pos_ki, pos_kd, max_rpm),
      vel_pid_(vel_kp, vel_ki, vel_kd, max_current_ma),
      max_rpm_(max_rpm),
      max_current_ma_(max_current_ma) {}

void M3508Motor::onFeedback(uint16_t rx_id, const uint8_t data[8]) {
  if (rx_id != feedbackId()) {
    return;
  }

  last_feedback_ = domain::c620::decodeFeedback(data);
  angle_.update(last_feedback_.raw_angle);
}

void M3508Motor::setEnabled(bool enabled) {
  if (enabled_ == enabled) {
    return;
  }
  enabled_ = enabled;
  // 止めている間に積分が伸びると、再開した瞬間に飛び出す。
  pos_pid_.reset();
  vel_pid_.reset();
  timer_.reset();
}

void M3508Motor::resetOrigin() {
  angle_.reset();
  pos_pid_.reset();
  vel_pid_.reset();
  timer_.reset();
}

void M3508Motor::setDirectCurrent(float milli_amp) {
  direct_current_ = true;
  direct_current_ma_ = std::clamp(milli_amp, -max_current_ma_, max_current_ma_);
}

void M3508Motor::clearDirectCurrent() {
  direct_current_ = false;
  pos_pid_.reset();
  vel_pid_.reset();
  timer_.reset();
}

int32_t M3508Motor::currentMilliAmp() const {
  return domain::c620::rawToCurrent(last_feedback_.current_raw);
}

int16_t M3508Motor::computeCurrentMilliAmp() {
  if (!enabled_) {
    return 0;
  }
  if (direct_current_) {
    return static_cast<int16_t>(direct_current_ma_);
  }
  if (!hasFeedback()) {
    return 0;
  }
  // 2 つのループは同じ周期で回るので、経過時間は 1 回だけ測って共有する。
  const float dt_s = timer_.update(HAL_GetTick());

  // 外側: 位置 → 目標rpm
  const float target_rpm = pos_pid_.update(target_motor_deg_, angle_.degrees(), dt_s);
  // 内側: 速度 → 電流[mA]
  const float current_ma =
      vel_pid_.update(target_rpm, static_cast<float>(last_feedback_.rpm), dt_s);
  const float clamped = std::clamp(current_ma, -max_current_ma_, max_current_ma_);
  return static_cast<int16_t>(clamped);
}

int16_t M3508Motor::commandRaw() {
  return domain::c620::currentToRaw(computeCurrentMilliAmp());
}

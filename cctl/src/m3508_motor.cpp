#include "m3508_motor.hpp"

#include <algorithm>

namespace {
constexpr int32_t kCountsPerRev = 8192;  // C620 角度分解能
constexpr int32_t kHalfRev = kCountsPerRev / 2;
// C620 電流スケール: ±20000mA ↔ ±16384
constexpr int32_t kCurrentScaleNum = 16384;
constexpr int32_t kCurrentScaleDen = 20000;
}  // namespace

M3508Motor::M3508Motor(CanBus& bus, uint8_t esc_id, uint16_t command_id,
                       float pos_kp, float pos_ki, float pos_kd, float max_rpm,
                       float vel_kp, float vel_ki, float vel_kd, float max_current_ma)
    : bus_(bus),
      esc_id_(esc_id),
      command_id_(command_id),
      pos_pid_(pos_kp, pos_ki, pos_kd, max_rpm),
      vel_pid_(vel_kp, vel_ki, vel_kd, max_current_ma),
      max_rpm_(max_rpm),
      max_current_ma_(max_current_ma) {}

void M3508Motor::onFeedback(uint16_t rx_id, const uint8_t data[8]) {
  if (rx_id != feedbackId()) {
    return;
  }

  const uint16_t raw_angle = (static_cast<uint16_t>(data[0]) << 8) | data[1];
  rpm_ = static_cast<int16_t>((data[2] << 8) | data[3]);
  amp_ = static_cast<int16_t>((data[4] << 8) | data[5]);
  temp_ = data[6];

  if (!has_feedback_) {
    // 初回受信角度を原点に採用（起動位置=原点方針）。
    last_raw_angle_ = raw_angle;
    total_counts_ = 0;
    origin_counts_ = 0;
    has_feedback_ = true;
  } else {
    int32_t delta = static_cast<int32_t>(raw_angle) - static_cast<int32_t>(last_raw_angle_);
    if (delta > kHalfRev) {
      delta -= kCountsPerRev;
    } else if (delta < -kHalfRev) {
      delta += kCountsPerRev;
    }
    total_counts_ += delta;
    last_raw_angle_ = raw_angle;
  }

  motor_deg_ = static_cast<float>(total_counts_ - origin_counts_) * 360.0f /
               static_cast<float>(kCountsPerRev);
}

int16_t M3508Motor::computeCurrentMilliAmp() {
  if (!has_feedback_) {
    return 0;
  }
  // 外側: 位置 → 目標rpm
  const float target_rpm = pos_pid_.update(target_motor_deg_, motor_deg_);
  // 内側: 速度 → 電流[mA]
  const float current_ma = vel_pid_.update(target_rpm, static_cast<float>(rpm_));
  const float clamped = std::clamp(current_ma, -max_current_ma_, max_current_ma_);
  return static_cast<int16_t>(clamped);
}

bool M3508Motor::sendCurrentCommand() {
  const int32_t milli_amp = computeCurrentMilliAmp();
  const int32_t scaled = milli_amp * kCurrentScaleNum / kCurrentScaleDen;

  uint8_t tx[8] = {};
  const uint8_t slot = static_cast<uint8_t>((esc_id_ - 1) * 2);
  tx[slot] = static_cast<uint8_t>((scaled >> 8) & 0xFF);
  tx[slot + 1] = static_cast<uint8_t>(scaled & 0xFF);
  return bus_.sendStd(command_id_, tx, 8);
}

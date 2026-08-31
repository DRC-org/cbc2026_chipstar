#include "m3508_motor.hpp"

#include "main.h"

#include <algorithm>

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

  last_feedback_ = domain::c620::decodeFeedback(data);
  angle_.update(last_feedback_.raw_angle);
}

int16_t M3508Motor::computeCurrentMilliAmp() {
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

bool M3508Motor::sendCurrentCommand() {
  const int16_t raw = domain::c620::currentToRaw(computeCurrentMilliAmp());

  uint8_t tx[8] = {};
  domain::c620::writeCommandSlot(tx, esc_id_, raw);
  return bus_.sendStd(command_id_, tx, 8);
}

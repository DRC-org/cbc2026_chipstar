#include "rthetaz_controller.hpp"

#include "main.h"
#include "robot_config.hpp"

#include <algorithm>

namespace {
constexpr float kTwoPi = 6.28318530718f;

float clampf(float v, float lo, float hi) { return std::clamp(v, lo, hi); }

// mm → 出力回転[rad]（送りねじ/ベルト/ラック共通: 1回転 = mm_per_rev）
float mmToRad(float mm, float mm_per_rev, float sign) {
  return sign * mm / mm_per_rev * kTwoPi;
}
}  // namespace

RThetaZController::RThetaZController(CanBus& bus)
    : bus_(bus),
      z_(bus, config::can_id::DM_CAN_ID, config::can_id::DM_MST_ID,
         config::dm::P_MAX, config::dm::V_MAX, config::dm::T_MAX),
      r_(bus, config::can_id::EL05_MOTOR_ID, config::can_id::EL05_HOST_ID),
      theta_(bus, config::can_id::C620_ESC_ID, config::can_id::C620_COMMAND,
             config::m3508::POS_KP, config::m3508::POS_KI, config::m3508::POS_KD,
             config::m3508::MAX_RPM, config::m3508::VEL_KP, config::m3508::VEL_KI,
             config::m3508::VEL_KD, config::m3508::MAX_CURRENT_MA) {}

void RThetaZController::begin() {
  // z: DM を Position-Velocity モードに設定してイネーブル。
  z_.disable();
  HAL_Delay(50);
  z_.setModePositionVelocity();
  HAL_Delay(50);
  z_.enable();
  HAL_Delay(50);

  // r: EL05 を Position モードに設定し、リミット/ゲイン、原点、イネーブル。
  r_.disable(true);
  HAL_Delay(50);
  r_.setRunModePosition();
  HAL_Delay(20);
  r_.writeParamFloat(0x7017 /*LIMIT_SPD*/, config::el05::LIMIT_SPD);
  HAL_Delay(20);
  r_.writeParamFloat(0x7018 /*LIMIT_CUR*/, config::el05::LIMIT_CUR);
  HAL_Delay(20);
  r_.writeParamFloat(0x701E /*LOC_KP*/, config::el05::LOC_KP);
  HAL_Delay(20);
  r_.setZero();
  HAL_Delay(200);
  r_.enable();
  HAL_Delay(50);

  // θ: M3508/C620 は電流指令のみ。原点は初回フィードバックで採用する。
  setTarget(0.0f, 0.0f, 0.0f);

  const uint32_t now = HAL_GetTick();
  last_m3508_ms_ = now;
  last_dm_ms_ = now;
  last_el05_ms_ = now;
  test_start_ms_ = now;
}

void RThetaZController::setTarget(float r_mm, float theta_deg, float z_mm) {
  target_r_mm_ = clampf(r_mm, config::mech::R_MIN_MM, config::mech::R_MAX_MM);
  target_theta_deg_ = clampf(theta_deg, config::mech::THETA_MIN_DEG, config::mech::THETA_MAX_DEG);
  target_z_mm_ = clampf(z_mm, config::mech::Z_MIN_MM, config::mech::Z_MAX_MM);

  target_r_rad_ = mmToRad(target_r_mm_, config::mech::R_MM_PER_OUTPUT_REV, config::mech::R_SIGN);
  target_z_rad_ = mmToRad(target_z_mm_, config::mech::Z_MM_PER_OUTPUT_REV, config::mech::Z_SIGN);

  const float motor_deg =
      config::mech::THETA_SIGN * target_theta_deg_ * config::mech::THETA_MOTOR_DEG_PER_OUTPUT_DEG;
  theta_.setTargetMotorDeg(motor_deg);
}

void RThetaZController::dispatchRx(const domain::CanFrame& frame) {
  switch (domain::classifyFeedback(frame, theta_.feedbackId(), z_.feedbackId())) {
    case domain::Axis::R:
      r_.onFeedback(frame.id, frame.data);
      break;
    case domain::Axis::Theta:
      theta_.onFeedback(static_cast<uint16_t>(frame.id), frame.data);
      break;
    case domain::Axis::Z:
      z_.onFeedback(frame.data);
      break;
    case domain::Axis::None:
      break;
  }
}

void RThetaZController::update() {
  if (test_enabled_) {
    applyTestSequence();
  }

  const uint32_t now = HAL_GetTick();

  if (now - last_m3508_ms_ >= config::period::M3508_MS) {
    last_m3508_ms_ = now;
    theta_.sendCurrentCommand();
  }
  if (now - last_dm_ms_ >= config::period::DM_MS) {
    last_dm_ms_ = now;
    z_.sendPositionVelocity(target_z_rad_, config::dm::POS_VEL_LIMIT);
  }
  if (now - last_el05_ms_ >= config::period::EL05_MS) {
    last_el05_ms_ = now;
    r_.setLocRef(target_r_rad_);
  }
}

void RThetaZController::applyTestSequence() {
  // 各軸を安全な範囲で往復させる単体確認用シーケンス（5秒刻み）。
  const uint32_t elapsed = HAL_GetTick() - test_start_ms_;
  const uint32_t phase = (elapsed / 5000U) % 4U;

  switch (phase) {
    case 0: setTarget(0.0f, 0.0f, 0.0f); break;
    case 1: setTarget(50.0f, 45.0f, 100.0f); break;
    case 2: setTarget(0.0f, 0.0f, 0.0f); break;
    case 3: setTarget(50.0f, -45.0f, 50.0f); break;
    default: break;
  }
}

#include "rthetaz_controller.hpp"

#include "domain/kinematics.hpp"
#include "main.h"
#include "robot_config.hpp"

#include <algorithm>

namespace {
namespace kin = domain::kinematics;

float clampf(float v, float lo, float hi) { return std::clamp(v, lo, hi); }
}  // namespace

RThetaZController::RThetaZController(CanBus& bus)
    : bus_(bus),
      z_(bus, config::can_id::DM_CAN_ID, config::can_id::DM_MST_ID,
         config::dm::P_MAX, config::dm::V_MAX, config::dm::T_MAX),
      r_(bus, config::can_id::EL05_MOTOR_ID, config::can_id::EL05_HOST_ID),
      theta_(bus, config::can_id::C620_ESC_ID, config::can_id::C620_COMMAND,
             config::m3508::POS_KP, config::m3508::POS_KI, config::m3508::POS_KD,
             config::m3508::MAX_RPM, config::m3508::VEL_KP, config::m3508::VEL_KI,
             config::m3508::VEL_KD, config::m3508::MAX_CURRENT_MA),
      theta_group_(bus, domain::c620::groupCommandId(config::can_id::C620_ESC_ID)) {
  theta_group_.add(theta_);
}

void RThetaZController::begin() {
  // z: DM を Position-Velocity モードに設定する。トルクはまだ入れない。
  z_.disable();
  HAL_Delay(50);
  z_.setModePositionVelocity();
  HAL_Delay(50);

  // r: EL05 を Position モードに設定し、リミット/ゲイン、原点まで。
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

  // θ: M3508/C620 は電流指令のみ。原点は初回フィードバックで採用する。
  setTarget(0.0f, 0.0f, 0.0f);

  // 立ち上げ事故を避けるため、初期状態は Safe・全軸無効。
  mode_ = domain::RunMode::Safe;
  enabled_axes_ = 0;
  applyAxisStates();

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

  target_r_rad_ = kin::mmToRad(target_r_mm_, config::mech::R_MM_PER_OUTPUT_REV,
                               config::mech::R_SIGN);
  target_z_rad_ = kin::mmToRad(target_z_mm_, config::mech::Z_MM_PER_OUTPUT_REV,
                               config::mech::Z_SIGN);

  theta_.setTargetMotorDeg(kin::outputDegToMotorDeg(
      target_theta_deg_, config::mech::THETA_MOTOR_DEG_PER_OUTPUT_DEG,
      config::mech::THETA_SIGN));
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

bool RThetaZController::axisActive(uint8_t axis) const {
  return mode_ == domain::RunMode::Run && (enabled_axes_ & axis) != 0;
}

void RThetaZController::applyAxisStates() {
  // θ は電流指令のみなので、無効化は 0 電流の送出で表現する。
  theta_.setEnabled(axisActive(domain::axis_bit::THETA));

  if (axisActive(domain::axis_bit::R)) {
    r_.enable();
  } else {
    r_.disable(false);
  }

  if (axisActive(domain::axis_bit::Z)) {
    z_.enable();
  } else {
    z_.disable();
  }
}

void RThetaZController::setMode(domain::RunMode mode) {
  if (mode_ == mode) {
    return;
  }
  mode_ = mode;
  applyAxisStates();
}

void RThetaZController::setAxesEnabled(uint8_t axes, bool enabled) {
  const uint8_t masked = axes & domain::axis_bit::ALL;
  const uint8_t updated =
      enabled ? static_cast<uint8_t>(enabled_axes_ | masked)
              : static_cast<uint8_t>(enabled_axes_ & ~masked);
  if (updated == enabled_axes_) {
    return;
  }
  enabled_axes_ = updated;
  applyAxisStates();
}

void RThetaZController::home() {
  theta_.resetOrigin();
  r_.setZero();
  z_.setZero();
  setTarget(0.0f, 0.0f, 0.0f);
}

float RThetaZController::measuredR() const {
  return kin::radToMm(r_.position(), config::mech::R_MM_PER_OUTPUT_REV, config::mech::R_SIGN);
}

float RThetaZController::measuredTheta() const {
  return kin::motorDegToOutputDeg(theta_.motorDeg(),
                                  config::mech::THETA_MOTOR_DEG_PER_OUTPUT_DEG,
                                  config::mech::THETA_SIGN);
}

float RThetaZController::measuredZ() const {
  return kin::radToMm(z_.position(), config::mech::Z_MM_PER_OUTPUT_REV, config::mech::Z_SIGN);
}

void RThetaZController::update() {
  if (test_enabled_ && mode_ == domain::RunMode::Run) {
    applyTestSequence();
  }

  const uint32_t now = HAL_GetTick();

  // θ は無効時も 0 電流を送り続ける。送信を止めると ESC 側の状態が読めない。
  if (now - last_m3508_ms_ >= config::period::M3508_MS) {
    last_m3508_ms_ = now;
    theta_group_.send();
  }
  if (axisActive(domain::axis_bit::Z) && now - last_dm_ms_ >= config::period::DM_MS) {
    last_dm_ms_ = now;
    z_.sendPositionVelocity(target_z_rad_, config::dm::POS_VEL_LIMIT);
  }
  if (axisActive(domain::axis_bit::R) && now - last_el05_ms_ >= config::period::EL05_MS) {
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

#include "actuator_controller.hpp"

#include "device_config.hpp"

#include <cmath>

ActuatorController::ActuatorController(CanBus& bus)
    : slot0_(bus, config::can_id::EL05_MOTOR_ID, config::can_id::EL05_HOST_ID),
      slot1_(bus, config::can_id::C620_ESC_ID, config::can_id::C620_COMMAND,
             config::m3508::POS_KP, config::m3508::POS_KI, config::m3508::POS_KD,
             config::m3508::MAX_RPM, config::m3508::VEL_KP, config::m3508::VEL_KI,
             config::m3508::VEL_KD, config::m3508::MAX_CURRENT_MA),
      slot2_(bus, config::can_id::DM_CAN_ID, config::can_id::DM_MST_ID,
             config::dm::P_MAX, config::dm::V_MAX, config::dm::T_MAX),
      c620_group_(bus, domain::c620::groupCommandId(config::can_id::C620_ESC_ID)) {
  c620_group_.add(slot1_);
}

void ActuatorController::begin() {
  slot2_.disable();
  HAL_Delay(50);
  slot2_.setControlMode(DmMotor::ControlMode::PositionVelocity);
  HAL_Delay(50);

  slot0_.disable(true);
  HAL_Delay(50);
  slot0_.setRunMode(El05Motor::RunMode::Position);
  HAL_Delay(20);
  slot0_.writeParamFloat(domain::el05::param::LIMIT_SPD, config::el05::LIMIT_SPD);
  HAL_Delay(20);
  slot0_.writeParamFloat(domain::el05::param::LIMIT_CUR, config::el05::LIMIT_CUR);
  HAL_Delay(20);
  slot0_.writeParamFloat(domain::el05::param::LOC_KP, config::el05::LOC_KP);
  HAL_Delay(20);

  slot1_.setTargetMotorDeg(0.0f);
  mode_ = domain::RunMode::Safe;
  enabled_slots_ = 0;
  applySlotStates();

  const uint32_t now = HAL_GetTick();
  last_m3508_ms_ = now;
  last_dm_ms_ = now;
  last_el05_ms_ = now;
}

bool ActuatorController::setTarget(uint8_t slot, float value) {
  if (!std::isfinite(value)) return false;
  switch (slot) {
    case 0:
      if (value < config::limit::SLOT0_MIN || value > config::limit::SLOT0_MAX) return false;
      targets_[0] = value;
      return true;
    case 1:
      if (value < config::limit::SLOT1_MIN || value > config::limit::SLOT1_MAX) return false;
      targets_[1] = value;
      slot1_.setTargetMotorDeg(value);
      return true;
    case 2:
      if (value < config::limit::SLOT2_MIN || value > config::limit::SLOT2_MAX) return false;
      targets_[2] = value;
      return true;
    default:
      return false;
  }
}

float ActuatorController::target(uint8_t slot) const {
  return slot < domain::SLOT_COUNT ? targets_[slot] : 0.0f;
}

float ActuatorController::measured(uint8_t slot) const {
  switch (slot) {
    case 0: return slot0_.position();
    case 1: return slot1_.motorDeg();
    case 2: return slot2_.position();
    default: return 0.0f;
  }
}

uint8_t ActuatorController::errorBits() const {
  return static_cast<uint8_t>(slot0_.faultBits() | slot2_.errorState());
}

void ActuatorController::dispatchRx(const domain::CanFrame& frame) {
  if (frame.extended) {
    slot0_.onFeedback(frame.id, frame.data);
  } else if (frame.id == slot1_.feedbackId()) {
    slot1_.onFeedback(static_cast<uint16_t>(frame.id), frame.data);
  } else if (frame.id == slot2_.feedbackId()) {
    slot2_.onFeedback(frame.data);
  }
}

bool ActuatorController::slotActive(uint8_t bit) const {
  return mode_ == domain::RunMode::Run && (enabled_slots_ & bit) != 0;
}

void ActuatorController::applySlotStates() {
  slot1_.setEnabled(slotActive(domain::slot_bit::SLOT1));
  if (slotActive(domain::slot_bit::SLOT0)) slot0_.enable();
  else slot0_.disable(false);
  if (slotActive(domain::slot_bit::SLOT2)) slot2_.enable();
  else slot2_.disable();
}

void ActuatorController::setMode(domain::RunMode mode) {
  if (mode_ == mode) return;
  mode_ = mode;
  applySlotStates();
}

void ActuatorController::setSlotsEnabled(uint8_t slots, bool enabled) {
  const uint8_t masked = slots & domain::slot_bit::ALL;
  enabled_slots_ = enabled ? static_cast<uint8_t>(enabled_slots_ | masked)
                           : static_cast<uint8_t>(enabled_slots_ & ~masked);
  applySlotStates();
}

void ActuatorController::home(uint8_t slots) {
  setMode(domain::RunMode::Safe);
  if ((slots & domain::slot_bit::SLOT0) != 0) slot0_.setZero();
  if ((slots & domain::slot_bit::SLOT1) != 0) slot1_.resetOrigin();
  if ((slots & domain::slot_bit::SLOT2) != 0) slot2_.setZero();
  for (uint8_t slot = 0; slot < domain::SLOT_COUNT; ++slot) {
    if ((slots & (1U << slot)) != 0) setTarget(slot, 0.0f);
  }
}

void ActuatorController::update() {
  const uint32_t now = HAL_GetTick();
  if (now - last_m3508_ms_ >= config::period::M3508_MS) {
    last_m3508_ms_ = now;
    c620_group_.send();
  }
  if (slotActive(domain::slot_bit::SLOT2) && now - last_dm_ms_ >= config::period::DM_MS) {
    last_dm_ms_ = now;
    slot2_.sendPositionVelocity(targets_[2], config::dm::POS_VEL_LIMIT);
  }
  if (slotActive(domain::slot_bit::SLOT0) && now - last_el05_ms_ >= config::period::EL05_MS) {
    last_el05_ms_ = now;
    slot0_.setLocRef(targets_[0]);
  }
}

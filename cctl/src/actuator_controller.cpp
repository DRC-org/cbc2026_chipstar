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
  slot0_.writeParamFloat(domain::el05::param::LIMIT_SPD,
                         parameters_.get(domain::ParamId::El05LimitSpd));
  HAL_Delay(20);
  slot0_.writeParamFloat(domain::el05::param::LIMIT_CUR,
                         parameters_.get(domain::ParamId::El05LimitCur));
  HAL_Delay(20);
  slot0_.writeParamFloat(domain::el05::param::LOC_KP,
                         parameters_.get(domain::ParamId::El05LocKp));
  HAL_Delay(20);

  slot1_.setTargetMotorDeg(0.0f);
  mode_ = domain::RunMode::Safe;
  enabled_slots_ = 0;
  applySlotStates();

  const uint32_t now = HAL_GetTick();
  last_m3508_ms_ = now;
  last_dm_ms_ = now;
  last_el05_ms_ = now;
  for (auto& stamp : last_rx_ms_) stamp = now;
  stale_slots_ = 0;
}

bool ActuatorController::setTarget(uint8_t slot, float value) {
  if (!std::isfinite(value)) return false;
  switch (slot) {
    case 0:
      if (value < parameters_.get(domain::ParamId::Slot0Min) ||
          value > parameters_.get(domain::ParamId::Slot0Max)) {
        return false;
      }
      targets_[0] = value;
      return true;
    case 1:
      if (value < parameters_.get(domain::ParamId::Slot1Min) ||
          value > parameters_.get(domain::ParamId::Slot1Max)) {
        return false;
      }
      targets_[1] = value;
      slot1_.setTargetMotorDeg(value);
      return true;
    case 2:
      if (value < parameters_.get(domain::ParamId::Slot2Min) ||
          value > parameters_.get(domain::ParamId::Slot2Max)) {
        return false;
      }
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
  const uint32_t now = HAL_GetTick();
  if (frame.extended) {
    slot0_.onFeedback(frame.id, frame.data);
    last_rx_ms_[0] = now;
  } else if (frame.id == slot1_.feedbackId()) {
    slot1_.onFeedback(static_cast<uint16_t>(frame.id), frame.data);
    last_rx_ms_[1] = now;
  } else if (frame.id == slot2_.feedbackId()) {
    slot2_.onFeedback(frame.data);
    last_rx_ms_[2] = now;
  }
}

// モータからの応答が途切れたslotを落とす。
//
// hostとの通信だけを見ていると、モータ側のCANが抜けても気づけない。
// 位置ループは凍った実測値との差を見続け、電流上限のまま押し続ける。
void ActuatorController::checkFeedback(uint32_t now) {
  const uint32_t limit = parameters_.getMs(domain::ParamId::FeedbackTimeoutMs);
  for (uint8_t slot = 0; slot < domain::SLOT_COUNT; ++slot) {
    const uint8_t bit = static_cast<uint8_t>(1U << slot);
    if (mode_ != domain::RunMode::Run || (enabled_slots_ & bit) == 0) {
      stale_slots_ = static_cast<uint8_t>(stale_slots_ & ~bit);
      continue;
    }
    if (now - last_rx_ms_[slot] <= limit) {
      stale_slots_ = static_cast<uint8_t>(stale_slots_ & ~bit);
      continue;
    }
    // 出力を切ってから印を立てる。復帰にはhostからの再有効化を要求する。
    stale_slots_ = static_cast<uint8_t>(stale_slots_ | bit);
    setSlotsEnabled(bit, false);
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
  checkFeedback(HAL_GetTick());
  const uint32_t now = HAL_GetTick();
  if (now - last_m3508_ms_ >= parameters_.getMs(domain::ParamId::M3508PeriodMs)) {
    last_m3508_ms_ = now;
    c620_group_.send();
  }
  if (slotActive(domain::slot_bit::SLOT2) &&
      now - last_dm_ms_ >= parameters_.getMs(domain::ParamId::DmPeriodMs)) {
    last_dm_ms_ = now;
    slot2_.sendPositionVelocity(targets_[2], parameters_.get(domain::ParamId::DmPosVelLimit));
  }
  if (slotActive(domain::slot_bit::SLOT0) &&
      now - last_el05_ms_ >= parameters_.getMs(domain::ParamId::El05PeriodMs)) {
    last_el05_ms_ = now;
    slot0_.setLocRef(targets_[0]);
  }
}

bool ActuatorController::setParameter(uint8_t id, float value) {
  if (domain::requiresSafe(id) && mode_ != domain::RunMode::Safe) return false;
  if (!parameters_.set(id, value)) return false;
  applyParameter(id);
  return true;
}

// 変更をデバイスへ反映する。可動域と周期は参照側が毎回読むため何もしない。
void ActuatorController::applyParameter(uint8_t id) {
  using domain::ParamId;
  switch (static_cast<ParamId>(id)) {
    case ParamId::M3508PosKp:
    case ParamId::M3508PosKi:
    case ParamId::M3508PosKd:
    case ParamId::M3508VelKp:
    case ParamId::M3508VelKi:
    case ParamId::M3508VelKd:
      slot1_.setGains(parameters_.get(ParamId::M3508PosKp), parameters_.get(ParamId::M3508PosKi),
                      parameters_.get(ParamId::M3508PosKd), parameters_.get(ParamId::M3508VelKp),
                      parameters_.get(ParamId::M3508VelKi), parameters_.get(ParamId::M3508VelKd));
      break;
    case ParamId::M3508MaxRpm:
      slot1_.setMaxRpm(parameters_.get(ParamId::M3508MaxRpm));
      break;
    case ParamId::M3508MaxCurrentMa:
      slot1_.setMaxCurrentMilliAmp(parameters_.get(ParamId::M3508MaxCurrentMa));
      break;
    case ParamId::El05LocKp:
      slot0_.writeParamFloat(domain::el05::param::LOC_KP, parameters_.get(ParamId::El05LocKp));
      break;
    case ParamId::El05LimitSpd:
      slot0_.writeParamFloat(domain::el05::param::LIMIT_SPD,
                             parameters_.get(ParamId::El05LimitSpd));
      break;
    case ParamId::El05LimitCur:
      slot0_.writeParamFloat(domain::el05::param::LIMIT_CUR,
                             parameters_.get(ParamId::El05LimitCur));
      break;
    case ParamId::DmPMax:
    case ParamId::DmVMax:
    case ParamId::DmTMax:
      slot2_.setRange(parameters_.get(ParamId::DmPMax), parameters_.get(ParamId::DmVMax),
                      parameters_.get(ParamId::DmTMax));
      break;
    case ParamId::C620EscId:
      slot1_.setEscId(parameters_.getU8(ParamId::C620EscId));
      break;
    case ParamId::DmCanId:
    case ParamId::DmMstId:
      slot2_.setIds(parameters_.getU16(ParamId::DmCanId), parameters_.getU16(ParamId::DmMstId));
      break;
    case ParamId::El05MotorId:
    case ParamId::El05HostId:
      slot0_.setIds(parameters_.getU8(ParamId::El05MotorId),
                    parameters_.getU8(ParamId::El05HostId));
      break;
    default:
      break;
  }
}

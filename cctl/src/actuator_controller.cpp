#include "actuator_controller.hpp"

#include "param_store.hpp"

#include "device_config.hpp"
#include "domain/dm_codec.hpp"

#include <cmath>

ActuatorController::ActuatorController(CanBus& bus)
    : bus_(bus), slot0_(bus, config::can_id::EL05_MOTOR_ID, config::can_id::EL05_HOST_ID),
      slot1_(bus, config::can_id::C620_ESC_ID, config::can_id::C620_COMMAND,
             config::m3508::POS_KP, config::m3508::POS_KI, config::m3508::POS_KD,
             config::m3508::MAX_RPM, config::m3508::VEL_KP, config::m3508::VEL_KI,
             config::m3508::VEL_KD, config::m3508::MAX_CURRENT_MA),
      slot2_(bus, config::can_id::DM_CAN_ID, config::can_id::DM_MST_ID,
             config::dm::P_MAX, config::dm::V_MAX, config::dm::T_MAX),
      c620_group_(bus, domain::c620::groupCommandId(config::can_id::C620_ESC_ID)) {
  c620_group_.add(slot1_);
}

// モータ側の設定を書き込む。電源投入直後は応答が遅いので間を空ける。
void ActuatorController::initMotor(uint8_t slot) {
  switch (slot) {
    case 0:
      slot0_.disable(true);
      HAL_Delay(50);
      slot0_.setRunMode(El05Motor::RunMode::Position);
      HAL_Delay(20);
      slot0_.writeParamFloat(domain::el05::param::VEL_MAX,
                             parameters_.get(domain::ParamId::El05LimitSpd));
      HAL_Delay(20);
      slot0_.writeParamFloat(domain::el05::param::LIMIT_CUR,
                             parameters_.get(domain::ParamId::El05LimitCur));
      HAL_Delay(20);
      slot0_.writeParamFloat(domain::el05::param::LOC_KP,
                             parameters_.get(domain::ParamId::El05LocKp));
      HAL_Delay(20);
      break;
    case 1:
      // C620に設定はない。積算角の目標だけ現在位置に置き直す。
      slot1_.setTargetMotorDeg(slot1_.motorDeg());
      break;
    default:
      slot2_.disable();
      HAL_Delay(50);
      slot2_.setControlMode(DmMotor::ControlMode::PositionVelocity);
      HAL_Delay(50);
      break;
  }
  targets_[slot] = measured(slot);
}

void ActuatorController::applyAllParameters() {
  for (uint8_t id = 0; id < domain::PARAM_COUNT; ++id) applyParameter(id);
}

// ページ消去でCPUが数十ms止まる。SAFE中に、変更が落ち着いてから1回だけ書く。
void ActuatorController::flushParameters() {
  constexpr uint32_t QUIET_MS = 1000;
  if (!param_dirty_ || mode_ != domain::RunMode::Safe) return;
  if (HAL_GetTick() - param_dirty_ms_ < QUIET_MS) return;
  if (param_store::save(parameters_)) param_dirty_ = false;
  else param_dirty_ms_ = HAL_GetTick();
}

bool ActuatorController::resetParameters() {
  if (mode_ != domain::RunMode::Safe) return false;
  parameters_.reset();
  const uint32_t failures = bus_.txFailures();
  applyAllParameters();
  if (bus_.txFailures() != failures) {
    stopAfterTxFailure();
    return false;
  }
  param_dirty_ = false;
  return param_store::clear();
}

bool ActuatorController::reinitialize(uint8_t slots) {
  if (mode_ != domain::RunMode::Safe) return false;
  const uint32_t failures = bus_.txFailures();
  for (uint8_t slot = 0; slot < domain::SLOT_COUNT; ++slot) {
    if ((slots & (1U << slot)) != 0) initMotor(slot);
  }
  if (bus_.txFailures() != failures) {
    stopAfterTxFailure();
    return false;
  }
  feedback_.reset(HAL_GetTick());
  return true;
}

void ActuatorController::begin() {
  const uint32_t failures = bus_.txFailures();
  // 実機で詰めた値は保存されている。モータへ書き込む前に取り込む。
  param_store::load(parameters_);
  applyAllParameters();

  initMotor(2);
  initMotor(0);
  initMotor(1);
  mode_ = domain::RunMode::Safe;
  enabled_slots_ = 0;
  applySlotStates();

  const uint32_t now = HAL_GetTick();
  last_m3508_ms_ = now;
  last_dm_ms_ = now;
  last_el05_ms_ = now;
  feedback_.reset(now);
  if (bus_.txFailures() != failures) stopAfterTxFailure();
}

bool ActuatorController::setTarget(uint8_t slot, float value) {
  if (!std::isfinite(value)) return false;
  switch (slot) {
    case 0:
      if (value < parameters_.get(domain::ParamId::Slot0Min) ||
          value > parameters_.get(domain::ParamId::Slot0Max)) {
        return false;
      }
      jog_[0].reset(measured(0));
      targets_[0] = value;
      return true;
    case 1:
      if (value < parameters_.get(domain::ParamId::Slot1Min) ||
          value > parameters_.get(domain::ParamId::Slot1Max)) {
        return false;
      }
      jog_[1].reset(measured(1));
      targets_[1] = value;
      slot1_.setTargetMotorDeg(value);
      return true;
    case 2:
      if (value < parameters_.get(domain::ParamId::Slot2Min) ||
          value > parameters_.get(domain::ParamId::Slot2Max)) {
        return false;
      }
      jog_[2].reset(measured(2));
      targets_[2] = value;
      return true;
    default:
      return false;
  }
}

bool ActuatorController::setJog(uint8_t slot, float velocity) {
  if (slot >= domain::SLOT_COUNT || !std::isfinite(velocity) ||
      !slotActive(static_cast<uint8_t>(1U << slot))) return false;
  const float caps[] = {
      parameters_.get(domain::ParamId::El05LimitSpd),
      parameters_.get(domain::ParamId::M3508MaxRpm) * 6.0f,
      parameters_.get(domain::ParamId::DmPosVelLimit)};
  jog_[slot].command(std::clamp(velocity, -caps[slot], caps[slot]), measured(slot));
  return true;
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

uint8_t ActuatorController::errorBits(uint8_t slot) const {
  const uint8_t stale =
      (feedback_.stale() & (1U << slot)) ? domain::error_bit::FEEDBACK_LOST : 0;
  switch (slot) {
    case 0:
      return static_cast<uint8_t>(slot0_.faultBits() | stale);
    case 1: {
      // C620は異常フラグを返さないため、温度だけを異常として扱う。
      const bool hot = slot1_.temperature() >=
                       parameters_.get(domain::ParamId::M3508MaxTemperatureC);
      return static_cast<uint8_t>((hot ? domain::error_bit::OVER_TEMPERATURE : 0) | stale);
    }
    default: {
      // DMのERRは列挙値で、bit maskではない。そのまま載せると
      // 「過負荷(0x0E)」と他コードのbitが混ざって見えるため、
      // 異常かどうかだけを上位bitで示し、生の値は下位に残す。
      const uint8_t code = slot2_.errorState();
      const uint8_t fault = domain::dm::error_code::isFault(code) ? 0x10 : 0;
      return static_cast<uint8_t>(code | fault | stale);
    }
  }
}

void ActuatorController::dispatchRx(const domain::CanFrame& frame) {
  const uint32_t now = HAL_GetTick();
  if (frame.extended) {
    if (slot0_.onFeedback(frame.id, frame.data)) feedback_.markSeen(0, now);
  } else if (frame.id == slot1_.feedbackId()) {
    slot1_.onFeedback(static_cast<uint16_t>(frame.id), frame.data);
    feedback_.markSeen(1, now);
  } else if (frame.id == slot2_.feedbackId()) {
    slot2_.onFeedback(frame.data);
    feedback_.markSeen(2, now);
  }
}

// モータからの応答が途切れたslotを落とす。
//
// hostとの通信だけを見ていると、モータ側のCANが抜けても気づけない。
// 位置ループは凍った実測値との差を見続け、電流上限のまま押し続ける。
void ActuatorController::checkFeedback(uint32_t now) {
  // 判定は domain::FeedbackWatch に置いてある。指令を送っていないslotは
  // 応答も返らないので、有効になった時点から数え始める。
  const uint8_t active = mode_ == domain::RunMode::Run ? enabled_slots_ : 0;
  const uint8_t dropped =
      feedback_.update(now, active, parameters_.getMs(domain::ParamId::FeedbackTimeoutMs));
  // 出力を切る。復帰にはhostからの再有効化を要求する。
  if (dropped != 0) setSlotsEnabled(dropped, false);
}

bool ActuatorController::slotActive(uint8_t bit) const {
  return mode_ == domain::RunMode::Run && (enabled_slots_ & bit) != 0;
}

void ActuatorController::stopAfterTxFailure() {
  mode_ = domain::RunMode::Stop;
  enabled_slots_ = 0;
  slot1_.setEnabled(false);
  for (uint8_t slot = 0; slot < domain::SLOT_COUNT; ++slot) jog_[slot].reset(measured(slot));
  cancel_before_stop_ = !bus_.discardPending();
  stop_pending_ = domain::slot_bit::ALL;
  retry_stop_ = true;
}

bool ActuatorController::applySlotStates() {
  const bool el05 = slotActive(domain::slot_bit::SLOT0) ? slot0_.enable() : slot0_.disable(false);
  const bool dm = slotActive(domain::slot_bit::SLOT2) ? slot2_.enable() : slot2_.disable();
  if (!el05 || !dm) {
    stopAfterTxFailure();
    return false;
  }
  slot1_.setEnabled(slotActive(domain::slot_bit::SLOT1));
  return true;
}

bool ActuatorController::setMode(domain::RunMode mode) {
  if (retry_stop_) {
    if (mode != domain::RunMode::Run) mode_ = mode;
    return false;
  }
  if (mode_ == mode) return true;
  if (mode != domain::RunMode::Run && !bus_.discardPending()) {
    stopAfterTxFailure();
    return false;
  }
  for (uint8_t slot = 0; slot < domain::SLOT_COUNT; ++slot) {
    jog_[slot].reset(measured(slot));
    targets_[slot] = measured(slot);
  }
  slot1_.setTargetMotorDeg(targets_[1]);
  mode_ = mode;
  return applySlotStates();
}

bool ActuatorController::setSlotsEnabled(uint8_t slots, bool enabled) {
  if (retry_stop_) return false;
  if (!enabled && !bus_.discardPending()) {
    stopAfterTxFailure();
    return false;
  }
  const uint8_t masked = slots & domain::slot_bit::ALL;
  enabled_slots_ = enabled ? static_cast<uint8_t>(enabled_slots_ | masked)
                           : static_cast<uint8_t>(enabled_slots_ & ~masked);
  return applySlotStates();
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
  if (retry_stop_) {
    // 一度受け付けた停止フレームは取り消さず、未受付分だけを再送する。
    if (cancel_before_stop_) {
      cancel_before_stop_ = !bus_.discardPending();
      if (cancel_before_stop_) return;
    }
    if ((stop_pending_ & 1) && slot0_.disable(false)) stop_pending_ &= ~1U;
    if ((stop_pending_ & 2) && c620_group_.send()) stop_pending_ &= ~2U;
    if ((stop_pending_ & 4) && slot2_.disable()) stop_pending_ &= ~4U;
    retry_stop_ = stop_pending_ != 0;
    // 無効状態の応答監視と周期時計は進め、復旧直後の一斉送信を避ける。
    const uint32_t now = HAL_GetTick();
    checkFeedback(now);
    last_jog_ms_ = last_m3508_ms_ = last_dm_ms_ = last_el05_ms_ = now;
    return;
  }
  const uint32_t failures = bus_.txFailures();
  checkFeedback(HAL_GetTick());
  const uint32_t now = HAL_GetTick();

  const float jog_dt = static_cast<float>(now - last_jog_ms_) * 0.001f;
  last_jog_ms_ = now;
  for (uint8_t slot = 0; slot < domain::SLOT_COUNT; ++slot) {
    if (!slotActive(static_cast<uint8_t>(1U << slot))) {
      jog_[slot].reset(measured(slot));
    } else if (jog_[slot].active()) {
      const uint8_t min_id = static_cast<uint8_t>(domain::ParamId::Slot0Min) + slot * 2;
      targets_[slot] = jog_[slot].step(measured(slot), jog_dt,
          parameters_.get(static_cast<domain::ParamId>(min_id)),
          parameters_.get(static_cast<domain::ParamId>(min_id + 1)));
      if (slot == 1) slot1_.setTargetMotorDeg(targets_[slot]);
    }
  }

  // 有効でないslotの目標を実測へ追従させる。トルクが切れている間に手で
  // 動かしても、そこが次の保持点になる。非常停止して退避させたあとRUNへ
  // 戻したとき、hostからの最初のTARGETが届く前に元の位置へ戻り出すのを防ぐ。
  for (uint8_t slot = 0; slot < domain::SLOT_COUNT; ++slot) {
    if (slotActive(static_cast<uint8_t>(1U << slot))) continue;
    targets_[slot] = measured(slot);
    if (slot == 1) slot1_.setTargetMotorDeg(targets_[1]);
  }

  if (now - last_m3508_ms_ >= parameters_.getMs(domain::ParamId::M3508PeriodMs)) {
    last_m3508_ms_ = now;
    c620_group_.send();
  }
  // 無効なDMには停止を再送する。DM3520は停止指令にも位置・状態を返す。
  if (now - last_dm_ms_ >= parameters_.getMs(domain::ParamId::DmPeriodMs)) {
    last_dm_ms_ = now;
    if (slotActive(domain::slot_bit::SLOT2)) {
      slot2_.sendPositionVelocity(targets_[2], parameters_.get(domain::ParamId::DmPosVelLimit));
    } else {
      slot2_.disable();
    }
  }
  // 読取り専用の診断。モータのモード・目標・出力状態は変更しない。
  if (now - last_el05_diagnostic_ms_ >= 250) {
    last_el05_diagnostic_ms_ = now;
    namespace p = domain::el05::param;
    static constexpr uint16_t indices[] = {
        p::RUN_MODE, p::LIMIT_SPD, p::LIMIT_CUR, p::LOC_KP,
        p::LOC_REF, p::MECH_POS, p::MECH_VEL, p::IQF, p::VBUS,
        p::SPD_KP, p::SPD_KI, p::LIMIT_TORQUE,
    };
    slot0_.requestParam(indices[el05_diagnostic_index_]);
    el05_diagnostic_index_ = (el05_diagnostic_index_ + 1) %
                             (sizeof(indices) / sizeof(indices[0]));
  }
  if (now - last_el05_ms_ >= parameters_.getMs(domain::ParamId::El05PeriodMs)) {
    last_el05_ms_ = now;
    slot0_.setLocRef(targets_[0]);
  }
  if (bus_.txFailures() != failures) stopAfterTxFailure();
}

bool ActuatorController::setParameter(uint8_t id, float value) {
  if (domain::requiresSafe(id) && mode_ != domain::RunMode::Safe) return false;
  if (!domain::Parameters::valid(id, value)) return false;
  if (parameters_.get(id) == value) return true;
  const float previous = parameters_.get(id);
  if (!parameters_.set(id, value)) return false;
  const uint32_t failures = bus_.txFailures();
  applyParameter(id);
  if (bus_.txFailures() != failures) {
    parameters_.set(id, previous);
    stopAfterTxFailure();
    return false;
  }
  param_dirty_ = true;
  param_dirty_ms_ = HAL_GetTick();
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
      slot0_.writeParamFloat(domain::el05::param::VEL_MAX,
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
      c620_group_.reset(domain::c620::groupCommandId(slot1_.escId()));
      c620_group_.add(slot1_);
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

bool ActuatorController::readDmRegister(uint8_t rid) {
  if (mode_ != domain::RunMode::Safe) return false;
  return slot2_.requestRegister(rid);
}

bool ActuatorController::storeDmParameters() {
  if (mode_ != domain::RunMode::Safe || !slot2_.disabledRecently()) return false;
  return slot2_.storeParameters();
}

bool ActuatorController::writeDmRegister(uint8_t rid, uint32_t raw) {
  if (mode_ != domain::RunMode::Safe) return false;
  return slot2_.writeRegister(rid, raw);
}

bool ActuatorController::takeDmRegisterReply(uint8_t& rid, uint32_t& raw) {
  if (!slot2_.hasRegisterReply()) return false;
  rid = slot2_.lastRegisterId();
  raw = slot2_.lastRegisterRaw();
  slot2_.clearRegisterReply();
  return true;
}

#include "dm_motor.hpp"

#include <cstring>

namespace {
namespace codec = domain::dm;
}  // namespace

bool DmMotor::sendConfig(uint8_t command, uint8_t rid, uint32_t value) {
  uint8_t data[8] = {};
  codec::encodeConfig(can_id_, command, rid, value, data);
  return bus_.sendStd(codec::CONFIG_ID, data, 8);
}

bool DmMotor::sendSpecialCommand(uint8_t command) {
  uint8_t data[8] = {};
  codec::encodeSpecial(command, data);
  return bus_.sendStd(can_id_, data, 8);
}

bool DmMotor::setControlMode(ControlMode mode) {
  return sendConfig(codec::CONFIG_WRITE, codec::reg::CTRL_MODE, static_cast<uint32_t>(mode));
}

bool DmMotor::enable() { return sendSpecialCommand(codec::SPECIAL_ENABLE); }
bool DmMotor::disable() { return sendSpecialCommand(codec::SPECIAL_DISABLE); }
bool DmMotor::setZero() { return sendSpecialCommand(codec::SPECIAL_ZERO); }

bool DmMotor::sendPositionVelocity(float pos_rad, float vel_limit) {
  uint8_t data[8] = {};
  codec::encodePositionVelocity(pos_rad, vel_limit, data);
  return bus_.sendStd(static_cast<uint16_t>(codec::POS_VEL_CMD_BASE + can_id_), data, 8);
}

bool DmMotor::sendVelocity(float vel_rad_s) {
  uint8_t data[8] = {};
  codec::encodeVelocity(vel_rad_s, data);
  return bus_.sendStd(static_cast<uint16_t>(codec::VELOCITY_CMD_BASE + can_id_), data, 8);
}

bool DmMotor::sendMit(float pos_rad, float vel_rad_s, float kp, float kd, float torque_nm) {
  uint8_t data[8] = {};
  codec::encodeMit(pos_rad, vel_rad_s, kp, kd, torque_nm, range_, data);
  return bus_.sendStd(static_cast<uint16_t>(codec::MIT_CMD_BASE + can_id_), data, 8);
}

bool DmMotor::writeRegister(uint8_t rid, uint32_t value) {
  return sendConfig(codec::CONFIG_WRITE, rid, value);
}

bool DmMotor::writeRegisterFloat(uint8_t rid, float value) {
  uint32_t bits = 0;
  std::memcpy(&bits, &value, sizeof(bits));
  return writeRegister(rid, bits);
}

bool DmMotor::requestRegister(uint8_t rid) {
  return sendConfig(codec::CONFIG_READ, rid, 0);
}

bool DmMotor::storeParameters() {
  return sendConfig(codec::CONFIG_STORE, 0, 0);
}

void DmMotor::onFeedback(const uint8_t data[8]) {
  // レジスタ応答はフィードバックと同じ ID で届く。
  // 取り違えると位置や温度に設定値が化けて入る。
  if (codec::isConfigReply(can_id_, data)) {
    has_register_reply_ = true;
    last_register_id_ = codec::configReplyRegister(data);
    last_register_raw_ = codec::configReplyValue(data);
    return;
  }

  feedback_ = codec::decodeFeedback(data, range_);
}

float DmMotor::lastRegisterFloat() const {
  float value = 0.0f;
  std::memcpy(&value, &last_register_raw_, sizeof(value));
  return value;
}

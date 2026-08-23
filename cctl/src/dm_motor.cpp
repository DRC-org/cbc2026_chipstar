#include "dm_motor.hpp"

#include <cstring>

namespace {
constexpr uint16_t kConfigWriteId = 0x7FF;  // レジスタ設定フレームID
constexpr uint8_t kWriteCmd = 0x55;         // 書込コマンド
constexpr uint8_t kRidCtrlMode = 0x0A;      // 制御モードレジスタ
constexpr uint32_t kModePositionVelocity = 2;
constexpr uint16_t kPosVelCmdBase = 0x100;  // 位置速度指令 = 0x100 + can_id

// N bit 整数 → 対称レンジ float への線形マッピング（マニュアル準拠）
float rawToFloat(uint32_t raw, uint32_t full_scale, float range) {
  return static_cast<float>(raw) * (2.0f * range) / static_cast<float>(full_scale) - range;
}
}  // namespace

bool DmMotor::writeRegisterU32(uint8_t rid, uint32_t value) {
  uint8_t data[8] = {};
  data[0] = static_cast<uint8_t>(can_id_ & 0xFF);
  data[1] = static_cast<uint8_t>((can_id_ >> 8) & 0xFF);
  data[2] = kWriteCmd;
  data[3] = rid;
  data[4] = static_cast<uint8_t>(value & 0xFF);
  data[5] = static_cast<uint8_t>((value >> 8) & 0xFF);
  data[6] = static_cast<uint8_t>((value >> 16) & 0xFF);
  data[7] = static_cast<uint8_t>((value >> 24) & 0xFF);
  return bus_.sendStd(kConfigWriteId, data, 8);
}

bool DmMotor::sendSpecialCommand(uint8_t command) {
  // DM 特殊コマンド (0xFC:Enable / 0xFD:Disable / 0xFE:Zero)
  uint8_t data[8] = {0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, command};
  return bus_.sendStd(can_id_, data, 8);
}

bool DmMotor::setModePositionVelocity() {
  return writeRegisterU32(kRidCtrlMode, kModePositionVelocity);
}

bool DmMotor::enable() { return sendSpecialCommand(0xFC); }
bool DmMotor::disable() { return sendSpecialCommand(0xFD); }
bool DmMotor::setZero() { return sendSpecialCommand(0xFE); }

bool DmMotor::sendPositionVelocity(float pos_rad, float vel_limit) {
  uint8_t data[8] = {};
  std::memcpy(&data[0], &pos_rad, sizeof(float));
  std::memcpy(&data[4], &vel_limit, sizeof(float));
  return bus_.sendStd(static_cast<uint16_t>(kPosVelCmdBase + can_id_), data, 8);
}

void DmMotor::onFeedback(const uint8_t data[8]) {
  // D0: ID|ERR<<4, D1-2: POS(16), D3: VEL[11:4], D4: VEL[3:0]|T[11:8], D5: T[7:0]
  error_state_ = (data[0] >> 4) & 0x0F;

  const uint16_t pos_raw = (static_cast<uint16_t>(data[1]) << 8) | data[2];      // 16bit
  const uint16_t vel_raw = (static_cast<uint16_t>(data[3]) << 4) | (data[4] >> 4);  // 12bit
  const uint16_t tau_raw = (static_cast<uint16_t>(data[4] & 0x0F) << 8) | data[5];  // 12bit

  position_rad_ = rawToFloat(pos_raw, 65535u, p_max_);
  velocity_rad_s_ = rawToFloat(vel_raw, 4095u, v_max_);
  torque_nm_ = rawToFloat(tau_raw, 4095u, t_max_);
}

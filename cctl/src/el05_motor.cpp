#include "el05_motor.hpp"

#include <cstring>

namespace {
// 通信種別 (拡張ID bit28..24)
namespace comm {
constexpr uint8_t MOTION_CONTROL = 1;
constexpr uint8_t FEEDBACK = 2;
constexpr uint8_t ENABLE = 3;
constexpr uint8_t DISABLE = 4;
constexpr uint8_t SET_ZERO = 6;
constexpr uint8_t WRITE_PARAM = 18;
}  // namespace comm

// パラメータID
constexpr uint16_t PARAM_RUN_MODE = 0x7005;
constexpr uint16_t PARAM_LOC_REF = 0x7016;

constexpr uint8_t RUN_MODE_POSITION = 1;

// フィードバック復号レンジ（MITマッピング）
constexpr float P_MIN = -12.57f, P_MAX = 12.57f;
constexpr float V_MIN = -50.0f, V_MAX = 50.0f;
constexpr float T_MIN = -6.0f, T_MAX = 6.0f;

// 拡張ID 構築: [comm_type(5)][data_area_2(16)][target_id(8)]
uint32_t buildCanId(uint8_t comm_type, uint16_t data_area_2, uint8_t target_id) {
  return ((static_cast<uint32_t>(comm_type) & 0x1F) << 24) |
         ((static_cast<uint32_t>(data_area_2) & 0xFFFF) << 8) |
         static_cast<uint32_t>(target_id);
}

float uint16ToFloat(uint16_t raw, float min, float max) {
  return static_cast<float>(raw) * (max - min) / 65535.0f + min;
}
}  // namespace

bool El05Motor::enable() {
  uint8_t data[8] = {};
  return bus_.sendExt(buildCanId(comm::ENABLE, host_id_, motor_id_), data, 8);
}

bool El05Motor::disable(bool clear_fault) {
  uint8_t data[8] = {};
  data[0] = clear_fault ? 1 : 0;
  return bus_.sendExt(buildCanId(comm::DISABLE, host_id_, motor_id_), data, 8);
}

bool El05Motor::setZero() {
  uint8_t data[8] = {};
  data[0] = 1;
  return bus_.sendExt(buildCanId(comm::SET_ZERO, host_id_, motor_id_), data, 8);
}

bool El05Motor::writeParamFloat(uint16_t param, float value) {
  uint8_t data[8] = {};
  data[0] = static_cast<uint8_t>(param & 0xFF);
  data[1] = static_cast<uint8_t>((param >> 8) & 0xFF);
  std::memcpy(&data[4], &value, sizeof(float));
  return bus_.sendExt(buildCanId(comm::WRITE_PARAM, host_id_, motor_id_), data, 8);
}

bool El05Motor::writeParamU8(uint16_t param, uint8_t value) {
  uint8_t data[8] = {};
  data[0] = static_cast<uint8_t>(param & 0xFF);
  data[1] = static_cast<uint8_t>((param >> 8) & 0xFF);
  data[4] = value;
  return bus_.sendExt(buildCanId(comm::WRITE_PARAM, host_id_, motor_id_), data, 8);
}

bool El05Motor::setRunModePosition() { return writeParamU8(PARAM_RUN_MODE, RUN_MODE_POSITION); }

bool El05Motor::setLocRef(float pos_rad) { return writeParamFloat(PARAM_LOC_REF, pos_rad); }

bool El05Motor::onFeedback(uint32_t ext_id, const uint8_t data[8]) {
  const uint8_t ct = (ext_id >> 24) & 0x1F;
  if (ct != comm::FEEDBACK) {
    return false;
  }
  const uint8_t src_id = (ext_id >> 8) & 0xFF;
  if (src_id != motor_id_) {
    return false;
  }

  fault_bits_ = (ext_id >> 16) & 0x3F;

  const uint16_t pos_raw = (static_cast<uint16_t>(data[0]) << 8) | data[1];
  const uint16_t vel_raw = (static_cast<uint16_t>(data[2]) << 8) | data[3];
  const uint16_t tau_raw = (static_cast<uint16_t>(data[4]) << 8) | data[5];
  const uint16_t tmp_raw = (static_cast<uint16_t>(data[6]) << 8) | data[7];

  position_rad_ = uint16ToFloat(pos_raw, P_MIN, P_MAX);
  velocity_rad_s_ = uint16ToFloat(vel_raw, V_MIN, V_MAX);
  torque_nm_ = uint16ToFloat(tau_raw, T_MIN, T_MAX);
  temperature_c_ = static_cast<float>(tmp_raw) / 10.0f;
  return true;
}

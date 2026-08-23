#pragma once

#include "can_bus.hpp"

#include <cstdint>

// r軸: RobStride EL05 ドライバ（拡張ID 29bit）。
// Position モード(run_mode=1) を用い、LOC_REF に目標位置[rad]を書き込む。
class El05Motor {
 public:
  El05Motor(CanBus& bus, uint8_t motor_id, uint8_t host_id)
      : bus_(bus), motor_id_(motor_id), host_id_(host_id) {}

  bool enable();
  bool disable(bool clear_fault = true);
  bool setZero();
  bool setRunModePosition();
  bool writeParamFloat(uint16_t param, float value);
  bool writeParamU8(uint16_t param, uint8_t value);

  // 目標位置[rad]を設定（LOC_REF 書込）。
  bool setLocRef(float pos_rad);

  // 拡張フィードバックフレームか判定し、解析して内部状態を更新する。
  bool onFeedback(uint32_t ext_id, const uint8_t data[8]);

  float position() const { return position_rad_; }
  float velocity() const { return velocity_rad_s_; }
  uint8_t faultBits() const { return fault_bits_; }

 private:
  CanBus& bus_;
  uint8_t motor_id_;
  uint8_t host_id_;

  float position_rad_ = 0.0f;
  float velocity_rad_s_ = 0.0f;
  float torque_nm_ = 0.0f;
  float temperature_c_ = 0.0f;
  uint8_t fault_bits_ = 0;
};

#pragma once

#include "can_bus.hpp"

#include <cstdint>

// z軸: DM-S3519 (Damiao) ドライバ。
// Position-Velocity モード(mode 2) を用い、目標位置[rad]と速度上限[rad/s]を
// cmd ID = 0x100 + can_id へ float×2 で送る。電源投入時に位置=0.0rad。
class DmMotor {
 public:
  DmMotor(CanBus& bus, uint16_t can_id, uint16_t mst_id, float p_max, float v_max, float t_max)
      : bus_(bus), can_id_(can_id), mst_id_(mst_id), p_max_(p_max), v_max_(v_max), t_max_(t_max) {}

  // 制御モードを Position-Velocity に設定（レジスタ 0x0A = 2）。
  bool setModePositionVelocity();
  bool enable();
  bool disable();
  bool setZero();

  // 目標位置[rad]と速度上限[rad/s]を送信。
  bool sendPositionVelocity(float pos_rad, float vel_limit);

  // フィードバックID。RX 振り分けに使用。
  uint16_t feedbackId() const { return mst_id_; }
  // フィードバックフレーム(8byte)を解析して内部状態を更新。
  void onFeedback(const uint8_t data[8]);

  float position() const { return position_rad_; }
  float velocity() const { return velocity_rad_s_; }
  uint8_t errorState() const { return error_state_; }

 private:
  bool writeRegisterU32(uint8_t rid, uint32_t value);
  bool sendSpecialCommand(uint8_t command);

  CanBus& bus_;
  uint16_t can_id_;
  uint16_t mst_id_;
  float p_max_;
  float v_max_;
  float t_max_;

  float position_rad_ = 0.0f;
  float velocity_rad_s_ = 0.0f;
  float torque_nm_ = 0.0f;
  uint8_t error_state_ = 0;
};

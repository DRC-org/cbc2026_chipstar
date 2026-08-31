#pragma once

#include "can_bus.hpp"
#include "domain/dm_codec.hpp"

#include <cstdint>

// z軸: DM-S3519 (Damiao) ドライバ。
// フレームの組立・復号は domain::dm が担い、本クラスは CAN 送受信と
// 状態保持を受け持つ。電源投入時に位置 = 0.0rad。
//
// 制御モードは MIT / Position-Velocity / Velocity の 3 つを扱える。
// rθz では Position-Velocity を使うが、単体確認では Velocity が便利。
class DmMotor {
 public:
  using ControlMode = domain::dm::ControlMode;

  DmMotor(CanBus& bus, uint16_t can_id, uint16_t mst_id, float p_max, float v_max, float t_max)
      : bus_(bus), can_id_(can_id), mst_id_(mst_id), range_{p_max, v_max, t_max} {}

  // ---- 状態遷移 ---------------------------------------------------------
  bool setControlMode(ControlMode mode);
  bool enable();
  bool disable();
  bool setZero();

  // ---- 指令 -------------------------------------------------------------
  // Position-Velocity モード: 目標位置[rad] と速度上限[rad/s]。
  bool sendPositionVelocity(float pos_rad, float vel_limit);
  // Velocity モード: 目標速度[rad/s]。
  bool sendVelocity(float vel_rad_s);
  // MIT モード: 位置・速度・ゲイン・トルク前置。
  // kp は [0,500]、kd は [0,5]。位置制御では kd を 0 にしない（発振する）。
  bool sendMit(float pos_rad, float vel_rad_s, float kp, float kd, float torque_nm);

  // ---- レジスタ ---------------------------------------------------------
  bool writeRegister(uint8_t rid, uint32_t value);
  bool writeRegisterFloat(uint8_t rid, float value);
  // 読み出しを要求する。値は応答フレームとして後から届く。
  bool requestRegister(uint8_t rid);
  // レジスタ書込は電源断で消える。保存するとチップへ書き込まれる。
  bool storeParameters();

  // ---- 受信 -------------------------------------------------------------
  uint16_t feedbackId() const { return mst_id_; }
  // フィードバックフレーム(8byte)を解析して内部状態を更新。
  // レジスタ応答も同じ ID で届くため、ここで振り分ける。
  void onFeedback(const uint8_t data[8]);

  // ---- 状態 -------------------------------------------------------------
  float position() const { return feedback_.position_rad; }
  float velocity() const { return feedback_.velocity_rad_s; }
  float torque() const { return feedback_.torque_nm; }
  uint8_t errorState() const { return feedback_.error; }
  // ドライバ上側 MOS の温度[degC]。連続運転の監視に使う。
  uint8_t mosTemperature() const { return feedback_.mos_temperature_c; }
  // モータ内部コイルの温度[degC]。
  uint8_t rotorTemperature() const { return feedback_.rotor_temperature_c; }

  // 直近に届いたレジスタ応答。PMAX などを実機から読むのに使う。
  bool hasRegisterReply() const { return has_register_reply_; }
  uint8_t lastRegisterId() const { return last_register_id_; }
  uint32_t lastRegisterRaw() const { return last_register_raw_; }
  float lastRegisterFloat() const;

 private:
  bool sendSpecialCommand(uint8_t command);
  bool sendConfig(uint8_t command, uint8_t rid, uint32_t value);

  CanBus& bus_;
  uint16_t can_id_;
  uint16_t mst_id_;
  domain::dm::Range range_;

  domain::dm::Feedback feedback_ = {};

  bool has_register_reply_ = false;
  uint8_t last_register_id_ = 0;
  uint32_t last_register_raw_ = 0;
};

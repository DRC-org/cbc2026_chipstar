#pragma once

#include "can_bus.hpp"
#include "domain/el05_codec.hpp"

#include <cstdint>

// r軸: RobStride EL05 ドライバ（拡張ID 29bit）。
// フレームの組立・復号は domain::el05 が担う。減速比 9:1 内蔵。
//
// 位置・速度・電流の各モードを扱える。rθz では位置モードを使うが、
// 単体確認では速度モードのほうが機構を壊しにくい。
class El05Motor {
 public:
  using RunMode = domain::el05::RunMode;

  El05Motor(CanBus& bus, uint8_t motor_id, uint8_t host_id)
      : bus_(bus), motor_id_(motor_id), host_id_(host_id) {}

  // ---- 状態遷移 ---------------------------------------------------------
  bool enable();
  bool disable(bool clear_fault = true);
  bool setZero();
  bool setRunMode(RunMode mode);

  // ---- モードごとの目標値 -----------------------------------------------
  // 位置モード: 目標位置[rad]（LOC_REF）。
  bool setLocRef(float pos_rad);
  // 速度モード: 目標速度[rad/s]（SPD_REF）。
  bool setSpeedRef(float vel_rad_s);
  // 電流モード: 目標電流[A]（IQ_REF, ±11A）。
  bool setCurrentRef(float amp);

  // ---- パラメータ -------------------------------------------------------
  bool writeParamFloat(uint16_t param, float value);
  bool writeParamU8(uint16_t param, uint8_t value);
  // 読み出しを要求する。値は応答フレームとして後から届く。
  bool requestParam(uint16_t param);
  // パラメータ書込は電源断で消える。保存するとドライバへ書き込まれる。
  bool saveParams();

  // ---- 受信 -------------------------------------------------------------
  // 拡張フレームを解析して内部状態を更新する。自分宛でなければ false。
  bool onFeedback(uint32_t ext_id, const uint8_t data[8]);

  // ---- 状態 -------------------------------------------------------------
  float position() const { return feedback_.position_rad; }
  float velocity() const { return feedback_.velocity_rad_s; }
  float torque() const { return feedback_.torque_nm; }
  float temperature() const { return feedback_.temperature_c; }
  uint8_t faultBits() const { return feedback_.fault_bits; }

  // 直近に届いたパラメータ応答。バス電圧などを実機から読むのに使う。
  bool hasParamReply() const { return has_param_reply_; }
  uint16_t lastParamIndex() const { return last_param_index_; }
  float lastParamFloat() const { return last_param_value_; }

 private:
  bool sendFrame(uint8_t comm_type, const uint8_t data[8]);

  CanBus& bus_;
  uint8_t motor_id_;
  uint8_t host_id_;

  domain::el05::Feedback feedback_ = {};

  bool has_param_reply_ = false;
  uint16_t last_param_index_ = 0;
  float last_param_value_ = 0.0f;
};

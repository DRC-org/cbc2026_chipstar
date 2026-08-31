#pragma once

#include "can_bus.hpp"
#include "domain/c620_codec.hpp"
#include "pid.hpp"

#include <cstdint>

// θ軸: M3508 + C620。C620 は電流指令のみ受け付けるため、位置制御は本クラスで
// 位置PID→速度PID の 2 段カスケードにより電流指令を生成する。
// C620 の角度(0..8191)から多回転角を積算し、モータ多回転角[deg]を追従させる。
// フレームの符号化・復号と多回転の積算は domain::c620 が担う。
class M3508Motor {
 public:
  M3508Motor(CanBus& bus, uint8_t esc_id, uint16_t command_id,
             float pos_kp, float pos_ki, float pos_kd, float max_rpm,
             float vel_kp, float vel_ki, float vel_kd, float max_current_ma);

  // フィードバックフレーム解析（多回転角の積算含む）。
  // 初回受信でその角度を原点(0)に採用する。
  void onFeedback(uint16_t rx_id, const uint8_t data[8]);
  uint16_t feedbackId() const { return command_id_ + esc_id_; }

  // 目標: モータ多回転角[deg]。
  void setTargetMotorDeg(float deg) { target_motor_deg_ = deg; }

  // カスケードPIDで電流を計算し、C620 コマンドフレームを送信する。
  bool sendCurrentCommand();

  float motorDeg() const { return angle_.degrees(); }
  int16_t rpm() const { return last_feedback_.rpm; }
  bool hasFeedback() const { return angle_.hasReference(); }

 private:
  int16_t computeCurrentMilliAmp();

  CanBus& bus_;
  uint8_t esc_id_;       // 1..4
  uint16_t command_id_;  // 0x200

  Pid pos_pid_;
  Pid vel_pid_;
  float max_rpm_;
  float max_current_ma_;

  // フィードバック状態
  domain::c620::MultiTurnCounter angle_;
  domain::c620::Feedback last_feedback_ = {};

  float target_motor_deg_ = 0.0f;
};

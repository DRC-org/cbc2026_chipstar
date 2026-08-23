#pragma once

#include "can_bus.hpp"
#include "pid.hpp"

#include <cstdint>

// θ軸: M3508 + C620。C620 は電流指令のみ受け付けるため、位置制御は本クラスで
// 位置PID→速度PID の 2 段カスケードにより電流指令を生成する。
// C620 の角度(0..8191)から多回転角を積算し、モータ多回転角[deg]を追従させる。
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

  float motorDeg() const { return motor_deg_; }
  int16_t rpm() const { return rpm_; }
  bool hasFeedback() const { return has_feedback_; }

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
  bool has_feedback_ = false;
  uint16_t last_raw_angle_ = 0;
  int32_t total_counts_ = 0;  // 多回転カウント（原点補正後）
  int32_t origin_counts_ = 0;
  float motor_deg_ = 0.0f;
  int16_t rpm_ = 0;
  int16_t amp_ = 0;
  uint8_t temp_ = 0;

  float target_motor_deg_ = 0.0f;
};

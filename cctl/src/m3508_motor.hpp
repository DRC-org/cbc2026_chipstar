#pragma once

#include "can_bus.hpp"
#include "domain/c620_codec.hpp"
#include "domain/delta_timer.hpp"
#include "domain/pid.hpp"

#include <cstdint>

// θ軸: M3508 + C620。C620 は電流指令のみ受け付けるため、位置制御は本クラスで
// 位置PID→速度PID の 2 段カスケードにより電流指令を生成する。
// C620 の角度(0..8191)から多回転角を積算し、モータ多回転角[deg]を追従させる。
// フレームの符号化・復号と多回転の積算は domain::c620 が担う。
class M3508Motor {
 public:
  // feedback_base: フィードバックID の基点(0x200)。ID = feedback_base + esc_id。
  M3508Motor(CanBus& bus, uint8_t esc_id, uint16_t feedback_base,
             float pos_kp, float pos_ki, float pos_kd, float max_rpm,
             float vel_kp, float vel_ki, float vel_kd, float max_current_ma);

  // フィードバックフレーム解析（多回転角の積算含む）。
  // 初回受信でその角度を原点(0)に採用する。
  void onFeedback(uint16_t rx_id, const uint8_t data[8]);
  uint16_t feedbackId() const { return feedback_base_ + esc_id_; }
  uint8_t escId() const { return esc_id_; }

  // 目標: モータ多回転角[deg]。
  void setTargetMotorDeg(float deg) { target_motor_deg_ = deg; }

  // 無効にすると電流指令 0 を送り続ける。PID は積分を溜めない。
  void setEnabled(bool enabled);
  bool enabled() const { return enabled_; }

  // 現在位置を新しい原点にする。
  void resetOrigin();

  // カスケードPIDで電流を計算し、C620 の指令生値を返す。
  // 送信は C620Group がまとめて行う。1 本のフレームに 4 台分が載るため、
  // モータが個別に送ると互いの指令を打ち消してしまう。
  int16_t commandRaw();

  // PID を通さず電流[mA]を直接指令する。ゲイン調整前の素の確認に使う。
  void setDirectCurrent(float milli_amp);
  void clearDirectCurrent();

  float motorDeg() const { return angle_.degrees(); }
  int16_t rpm() const { return last_feedback_.rpm; }
  bool hasFeedback() const { return angle_.hasReference(); }

  // ESC が返す実トルク電流[mA]。
  int32_t currentMilliAmp() const;
  // ESC が返すモータ温度[degC]。
  uint8_t temperature() const { return last_feedback_.temperature; }

 private:
  int16_t computeCurrentMilliAmp();

  CanBus& bus_;
  uint8_t esc_id_;         // 1..8
  uint16_t feedback_base_;  // 0x200

  domain::Pid pos_pid_;
  domain::Pid vel_pid_;
  domain::DeltaTimer timer_;
  float max_rpm_;
  float max_current_ma_;

  // フィードバック状態
  domain::c620::MultiTurnCounter angle_;
  domain::c620::Feedback last_feedback_ = {};
  bool enabled_ = false;

  float target_motor_deg_ = 0.0f;
  // 設定されている間は PID を迂回して直接この電流を出す。
  bool direct_current_ = false;
  float direct_current_ma_ = 0.0f;
};

#pragma once

#include "c620_group.hpp"
#include "can_bus.hpp"
#include "dm_motor.hpp"
#include "domain/can_frame.hpp"
#include "domain/run_state.hpp"
#include "el05_motor.hpp"
#include "m3508_motor.hpp"

#include <cstdint>

// rθz 3軸の統合コントローラ。
// 目標 r[mm] / θ[deg] / z[mm] を各モータ指令へ換算し、周期的に送信する。
// r=EL05(位置モード), θ=M3508(カスケードPID), z=DM(Position-Velocityモード)。
//
// 立ち上げ時は Safe から始まり、指令があるまで動かない。軸ごとに有効・無効を
// 切り替えられるので、1 軸ずつ確認しながら立ち上げられる。
class RThetaZController {
 public:
  explicit RThetaZController(CanBus& bus);

  // モータの初期化シーケンス（モード設定・原点）。setup で1回呼ぶ。
  // 呼んだ直後は Safe で、どの軸も有効化されていない。
  void begin();

  // 目標位置を設定（ソフトリミットでクランプ）。
  void setTarget(float r_mm, float theta_deg, float z_mm);

  // 受信フレームを各モータへ振り分ける。loop で毎回呼ぶ。
  void dispatchRx(const domain::CanFrame& frame);

  // 周期処理（各軸の指令送信・内蔵テスト動作）。loop で毎回呼ぶ。
  void update();

  // ---- 立ち上げ・調整用の操作 -------------------------------------------

  // 運転状態を切り替える。Stop / Safe では有効な軸もトルクを切る。
  void setMode(domain::RunMode mode);
  domain::RunMode mode() const { return mode_; }

  // 軸ごとの有効・無効。axes は domain::axis_bit の論理和。
  void setAxesEnabled(uint8_t axes, bool enabled);
  uint8_t enabledAxes() const { return enabled_axes_; }

  // 現在位置を原点に取り直し、目標値を 0 に戻す。
  void home();

  // 内蔵テストシーケンスの有効化（指令I/F未接続時の単体動作確認用）。
  void enableTestSequence(bool enable) { test_enabled_ = enable; }
  bool testSequenceEnabled() const { return test_enabled_; }

  // ---- 目標値 -----------------------------------------------------------
  float targetR() const { return target_r_mm_; }
  float targetTheta() const { return target_theta_deg_; }
  float targetZ() const { return target_z_mm_; }

  // ---- 実測値（テレメトリ用。機体座標へ戻したもの）----------------------
  float measuredR() const;
  float measuredTheta() const;
  float measuredZ() const;

  // z 軸ドライバのエラービット。
  uint8_t errorBits() const { return z_.errorState(); }

  const DmMotor& z() const { return z_; }
  const El05Motor& r() const { return r_; }
  const M3508Motor& theta() const { return theta_; }

 private:
  void applyTestSequence();
  // mode_ と enabled_axes_ を各モータのトルク状態へ反映する。
  void applyAxisStates();
  bool axisActive(uint8_t axis) const;

  CanBus& bus_;
  DmMotor z_;
  El05Motor r_;
  M3508Motor theta_;
  // θ は 1 台だが、同じ指令フレームに他の C620 を足せるようにまとめて送る。
  C620Group theta_group_;

  // 目標値（軸座標）
  float target_r_mm_ = 0.0f;
  float target_theta_deg_ = 0.0f;
  float target_z_mm_ = 0.0f;

  // 換算後のモータ指令
  float target_r_rad_ = 0.0f;
  float target_z_rad_ = 0.0f;

  // 運転状態
  domain::RunMode mode_ = domain::RunMode::Safe;
  uint8_t enabled_axes_ = 0;

  // スケジューリング
  uint32_t last_m3508_ms_ = 0;
  uint32_t last_dm_ms_ = 0;
  uint32_t last_el05_ms_ = 0;

  // テストシーケンス
  bool test_enabled_ = false;
  uint32_t test_start_ms_ = 0;
};

#pragma once

#include "can_bus.hpp"
#include "dm_motor.hpp"
#include "el05_motor.hpp"
#include "m3508_motor.hpp"

#include <cstdint>

// rθz 3軸の統合コントローラ。
// 目標 r[mm] / θ[deg] / z[mm] を各モータ指令へ換算し、周期的に送信する。
// r=EL05(位置モード), θ=M3508(カスケードPID), z=DM(Position-Velocityモード)。
class RThetaZController {
 public:
  explicit RThetaZController(CanBus& bus);

  // モータの初期化シーケンス（モード設定・原点・イネーブル）。setup で1回呼ぶ。
  void begin();

  // 目標位置を設定（ソフトリミットでクランプ）。
  void setTarget(float r_mm, float theta_deg, float z_mm);

  // 受信フレームを各モータへ振り分ける。loop で毎回呼ぶ。
  void dispatchRx(const FDCAN_RxHeaderTypeDef& header, const uint8_t data[8]);

  // 周期処理（各軸の指令送信・内蔵テスト動作）。loop で毎回呼ぶ。
  void update();

  // 内蔵テストシーケンスの有効化（指令I/F未接続時の単体動作確認用）。
  void enableTestSequence(bool enable) { test_enabled_ = enable; }

  // 表示用アクセサ
  float targetR() const { return target_r_mm_; }
  float targetTheta() const { return target_theta_deg_; }
  float targetZ() const { return target_z_mm_; }
  const DmMotor& z() const { return z_; }
  const El05Motor& r() const { return r_; }
  const M3508Motor& theta() const { return theta_; }

 private:
  void applyTestSequence();

  CanBus& bus_;
  DmMotor z_;
  El05Motor r_;
  M3508Motor theta_;

  // 目標値（軸座標）
  float target_r_mm_ = 0.0f;
  float target_theta_deg_ = 0.0f;
  float target_z_mm_ = 0.0f;

  // 換算後のモータ指令
  float target_r_rad_ = 0.0f;
  float target_z_rad_ = 0.0f;

  // スケジューリング
  uint32_t last_m3508_ms_ = 0;
  uint32_t last_dm_ms_ = 0;
  uint32_t last_el05_ms_ = 0;

  // テストシーケンス
  bool test_enabled_ = false;
  uint32_t test_start_ms_ = 0;
};

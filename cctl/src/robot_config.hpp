#pragma once

#include <cstdint>

// rθz 機体の機構・通信・制御パラメータ集約ヘッダ。
//
// 実機で確定していない機械定数は TODO を付し、暫定既定値を置く。
// 実測が判明したら本ファイルの値のみ差し替えればよい。

namespace config {

// ---- CAN バス割当 -------------------------------------------------------
// モータは 1 本のバス(FDCAN1, 1Mbps)に集約する。周辺基板=FDCAN2, 予備=FDCAN3。

// ---- CAN ID 割当（同一バス上の標準ID衝突を回避）------------------------
// C620(θ) は標準 0x200(cmd)/0x201(fb)。DM(z) は C620 帯域外へ逃がす。
// EL05(r) は拡張IDのため標準IDと衝突しない。
namespace can_id {
// M3508 / C620 (θ)
constexpr uint16_t C620_COMMAND = 0x200;    // 4ch 一括電流指令
constexpr uint8_t C620_ESC_ID = 1;          // ESC ID (1..4)
constexpr uint16_t C620_FEEDBACK = 0x200 + C620_ESC_ID; // 0x201

// DM-S3519 (z)
constexpr uint16_t DM_CAN_ID = 0x09;        // 受信ID (ESC_ID)。cmd = 0x100 + DM_CAN_ID
constexpr uint16_t DM_MST_ID = 0x0A;        // フィードバックID (MST_ID)

// EL05 (r) : 拡張ID
constexpr uint8_t EL05_MOTOR_ID = 0x7F;     // モータ既定ID
constexpr uint8_t EL05_HOST_ID = 0xFD;      // ホストID
} // namespace can_id

// ---- 機構換算定数（要実測 / TODO）--------------------------------------
namespace mech {
// z軸: タイミングベルト。DM 出力 1 回転あたりのベルト送り量 [mm/rev]。
// TODO: プーリ歯数 × ベルトピッチ（or 有効径×π）に置換。
constexpr float Z_MM_PER_OUTPUT_REV = 40.0f;
// z 可動域 [mm]（ソフトリミット, 起動位置=0 基準）。TODO: 実ストロークに置換。
constexpr float Z_MIN_MM = 0.0f;
constexpr float Z_MAX_MM = 300.0f;
constexpr float Z_SIGN = 1.0f; // 正方向反転が必要なら -1.0

// r軸: ラックピニオン。EL05 出力(9:1後) 1 回転あたりのラック送り量 [mm/rev]。
// TODO: ピニオンのモジュール×歯数(有効径×π)に置換。
constexpr float R_MM_PER_OUTPUT_REV = 62.8f;
constexpr float R_MIN_MM = 0.0f;
constexpr float R_MAX_MM = 250.0f;
constexpr float R_SIGN = 1.0f;

// θ軸: 内歯車 + ピニオン。ターンテーブル(内歯車)を M3508 ピニオンで駆動。
// TODO: 実歯数に置換。
constexpr float RING_TEETH = 110.0f;    // 内歯車 歯数
constexpr float PINION_TEETH = 15.0f;   // ピニオン 歯数
constexpr float C620_REDUCTION = 3591.0f / 187.0f; // M3508 内蔵減速比 ≈ 19.2032
// θ出力[deg] → M3508 モータ角[deg] への換算係数
constexpr float THETA_MOTOR_DEG_PER_OUTPUT_DEG =
    (RING_TEETH / PINION_TEETH) * C620_REDUCTION;
constexpr float THETA_MIN_DEG = -180.0f;
constexpr float THETA_MAX_DEG = 180.0f;
constexpr float THETA_SIGN = 1.0f;
} // namespace mech

// ---- DM (z) フィードバック復号レンジ（マニュアル PMAX 等）--------------
namespace dm {
constexpr float P_MAX = 12.5f;   // 位置マッピング範囲 [rad] TODO: 実機 PMAX
constexpr float V_MAX = 45.0f;   // 速度マッピング範囲 [rad/s]
constexpr float T_MAX = 18.0f;   // トルクマッピング範囲 [N·m]
// Position-Velocity モードでの速度上限指令 [rad/s]
constexpr float POS_VEL_LIMIT = 8.0f;
} // namespace dm

// ---- EL05 (r) 位置モードパラメータ -------------------------------------
namespace el05 {
constexpr float LIMIT_SPD = 5.0f;  // [rad/s]
constexpr float LIMIT_CUR = 8.0f;  // [A]
constexpr float LOC_KP = 30.0f;
} // namespace el05

// ---- M3508 (θ) カスケード PID ------------------------------------------
namespace m3508 {
// 外側: 位置ループ (モータ角[deg]誤差 → 目標rpm)
constexpr float POS_KP = 8.0f;
constexpr float POS_KI = 0.0f;
constexpr float POS_KD = 0.0f;
constexpr float MAX_RPM = 4000.0f; // 目標rpmクランプ
// 内側: 速度ループ (rpm誤差 → 電流[mA])
constexpr float VEL_KP = 0.7f;
constexpr float VEL_KI = 0.0005f;
constexpr float VEL_KD = 50.0f;
constexpr float MAX_CURRENT_MA = 5000.0f;
} // namespace m3508

// ---- 制御周期 [ms] ------------------------------------------------------
namespace period {
constexpr uint32_t M3508_MS = 1;   // 電流ループ 1kHz
constexpr uint32_t DM_MS = 10;     // 位置速度指令 再送
constexpr uint32_t EL05_MS = 20;   // LOC_REF 更新
constexpr uint32_t LCD_MS = 200;   // 状態表示
} // namespace period

// ---- 手動操作 ----------------------------------------------------------
namespace manual_control {
constexpr float STICK_DEADZONE = 0.10f;
constexpr float R_SPEED_MM_S = 100.0f;
constexpr float THETA_SPEED_DEG_S = 90.0f;
// 通信再開時に、途切れていた時間分を一度に移動しないための上限。
constexpr uint32_t MAX_INPUT_INTERVAL_MS = 100;
} // namespace manual_control

} // namespace config

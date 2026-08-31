#pragma once

#include <cstdint>

// シリアルサーボ基板の通信・動作パラメータ集約ヘッダ。
//
// サーボ側の設定に合わせる値はここだけを差し替えればよい。

namespace config {

// ---- サーボ -------------------------------------------------------------
// USART1 は 115200 bps, 8-N-1。STS3215 は工場出荷時 1 Mbps の個体があるため、
// Feetech の設定ツールなどで事前に 115200 bps へ変更しておく。
// （STS プロトコルの Baud Rate レジスタはアドレス6、115200 bps の設定値は4）
namespace servo {
constexpr uint8_t ID = 1;
constexpr uint32_t TIMEOUT_MS = 20;  // 1 パケットあたりの通信タイムアウト
// 書き込み応答を待たない。往復待ちがなくなる代わりに書き込みの成否は検出できない。
constexpr bool WAIT_FOR_WRITE_STATUS = false;
constexpr uint32_t BOOT_DELAY_MS = 500;  // 電源投入からサーボが応答するまでの待ち

constexpr uint8_t ACCELERATION = 50;
constexpr uint16_t SPEED = 500;
constexpr uint16_t CENTER_POSITION = 2048;  // 可動域中央 (0-4095)
constexpr uint16_t SWEEP_MIN_POSITION = 1024;
constexpr uint16_t SWEEP_MAX_POSITION = 3072;
}  // namespace servo

// ---- 制御周期 [ms] ------------------------------------------------------
namespace period {
constexpr uint32_t SERVO_COMMAND_MS = 2000;  // 往復目標値の送信
constexpr uint32_t SERVO_FEEDBACK_MS = 100;  // 現在位置の読み取り
}  // namespace period

}  // namespace config

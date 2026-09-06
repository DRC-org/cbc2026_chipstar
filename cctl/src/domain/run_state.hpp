#pragma once

#include <cstdint>

namespace domain {

// slotごとの異常bitのうち、cctlが自分で立てるもの。
// 下位bitは各モータのドライバが返す値をそのまま載せる。
namespace error_bit {
constexpr uint8_t OVER_TEMPERATURE = 0x40;
constexpr uint8_t FEEDBACK_LOST = 0x80;
}  // namespace error_bit


// 機体の運転状態。立ち上げ時は指令を出さない Safe から始める。
enum class RunMode : uint8_t {
    Safe = 0,  // 初期化のみ。目標値の送信もトルクも入れない
    Run,       // 通常運転
    Stop,      // 非常停止。トルクを切って保持する
};

constexpr uint8_t SLOT_COUNT = 3;

// 物理アクチュエータslotを指すビット。機体上の軸名は持たない。
namespace slot_bit {
constexpr uint8_t SLOT0 = 1 << 0;
constexpr uint8_t SLOT1 = 1 << 1;
constexpr uint8_t SLOT2 = 1 << 2;
constexpr uint8_t ALL = SLOT0 | SLOT1 | SLOT2;
}  // namespace slot_bit

}  // namespace domain

#pragma once

#include <cstdint>

namespace domain {

// 機体の運転状態。立ち上げ時は指令を出さない Safe から始める。
enum class RunMode : uint8_t {
    Safe = 0,  // 初期化のみ。目標値の送信もトルクも入れない
    Run,       // 通常運転
    Stop,      // 非常停止。トルクを切って保持する
};

// 軸を指すビット。テレメトリと指令で共通に使う。
namespace axis_bit {
constexpr uint8_t R = 1 << 0;
constexpr uint8_t THETA = 1 << 1;
constexpr uint8_t Z = 1 << 2;
constexpr uint8_t ALL = R | THETA | Z;
}  // namespace axis_bit

}  // namespace domain

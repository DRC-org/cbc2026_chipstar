#pragma once

#include "domain/run_state.hpp"

#include <cstdint>

namespace domain {

// LED1..3 の点灯パターン。ビット 0 が LED1。
// 端末を見られない状態でも機体の状態が分かるようにする。
//
//   Safe : LED1 だけを 1Hz で点滅
//   Run  : 有効な軸をそのまま点灯（LED1=r, LED2=θ, LED3=z）
//   Stop : 3 つとも 5Hz で点滅
uint8_t ledPattern(uint32_t tick_ms, RunMode mode, uint8_t enabled_axes);

}  // namespace domain

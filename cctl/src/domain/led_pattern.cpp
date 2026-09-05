#include "domain/led_pattern.hpp"

namespace domain {
namespace {
// 点灯・消灯を half_period_ms ごとに入れ替える。
bool blink(uint32_t tick_ms, uint32_t half_period_ms) {
    return (tick_ms / half_period_ms) % 2 == 0;
}
}  // namespace

uint8_t ledPattern(uint32_t tick_ms, RunMode mode, uint8_t enabled_axes) {
    switch (mode) {
        case RunMode::Safe:
            return blink(tick_ms, 500) ? slot_bit::SLOT0 : 0;
        case RunMode::Stop:
            return blink(tick_ms, 100) ? slot_bit::ALL : 0;
        case RunMode::Run:
            return enabled_axes & slot_bit::ALL;
    }
    return 0;
}

}  // namespace domain

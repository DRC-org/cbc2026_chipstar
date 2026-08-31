#include "domain/delta_timer.hpp"

namespace domain {

float DeltaTimer::update(uint32_t now_ms) {
    if (!has_previous_) {
        previous_ms_ = now_ms;
        has_previous_ = true;
        return 0.0f;
    }

    const uint32_t elapsed_ms = now_ms - previous_ms_;
    previous_ms_ = now_ms;
    return static_cast<float>(elapsed_ms) / 1000.0f;
}

void DeltaTimer::reset() {
    has_previous_ = false;
    previous_ms_ = 0;
}

}  // namespace domain

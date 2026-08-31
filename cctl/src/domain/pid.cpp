#include "domain/pid.hpp"

#include <algorithm>

namespace domain {

void Pid::reset() {
    integral_ = 0.0f;
    prev_error_ = 0.0f;
}

float Pid::update(float target, float measured, float dt_s) {
    const float error = target - measured;
    const float proportional = kp_ * error;

    float derivative = 0.0f;
    if (dt_s > 0.0f) {
        integral_ += ki_ * error * dt_s;
        derivative = kd_ * (error - prev_error_) / dt_s;
    }
    prev_error_ = error;

    const float raw = proportional + integral_ + derivative;
    const float clamped = std::clamp(raw, -out_limit_, out_limit_);

    // anti-windup: 出力飽和かつ誤差が同方向なら積分を捨てる。
    if (raw != clamped && (raw * error > 0.0f)) {
        integral_ = 0.0f;
    }

    return clamped;
}

}  // namespace domain

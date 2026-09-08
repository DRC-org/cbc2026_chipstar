#pragma once

#include <algorithm>
#include <cmath>

namespace domain {

// 速度入力を位置制御器へ渡すための短い軌道。遅れを無制限に積算しない。
// 単位はslotのネイティブ位置 / 秒。機構の換算はhostが担当する。
class Jog {
 public:
  void reset(float measured) { target_ = measured; velocity_ = 0; active_ = false; }
  void command(float velocity, float measured) {
    if (!active_ || velocity * velocity_ < 0 || (velocity == 0 && velocity_ != 0)) {
      target_ = measured;
    }
    velocity_ = velocity;
    active_ = true;
  }
  bool active() const { return active_; }
  float step(float measured, float dt) {
    if (velocity_ != 0) {
      const float lead = std::abs(velocity_) * 0.1f;
      target_ = std::clamp(target_ + velocity_ * std::clamp(dt, 0.0f, 0.02f),
                           measured - lead, measured + lead);
    }
    return target_;
  }
 private:
  float target_ = 0;
  float velocity_ = 0;
  bool active_ = false;
};

}  // namespace domain

#pragma once
#include <cstdint>

namespace domain {
// raw/stableは接点がGNDへ閉じたとき1。停止判定は生値、表示は10ms安定値を使う。
class DigitalInputs {
 public:
  explicit DigitalInputs(uint8_t available) : available_(available) {}
  void sample(uint8_t closed, uint8_t dip, uint32_t now) {
    closed &= available_;
    dip_ = dip;
    if (!initialized_) {
      initialized_ = true;
      candidate_ = closed;
      for (uint8_t bit = 0; bit < 8; ++bit) changed_[bit] = now;
    }
    raw_ = closed;
    for (uint8_t bit = 0; bit < 8; ++bit) {
      const uint8_t mask = static_cast<uint8_t>(1U << bit);
      if ((candidate_ ^ closed) & mask) {
        candidate_ ^= mask;
        changed_[bit] = now;
      }
      if (now - changed_[bit] >= 10) {
        stable_ = static_cast<uint8_t>((stable_ & ~mask) | (candidate_ & mask));
      }
    }
    if (active()) tripped_ = true;
  }
  bool configure(uint8_t mask, uint8_t high) {
    if (!initialized_ || (mask & ~available_) || (high & ~mask)) return false;
    guard_ = mask;
    high_ = high;
    tripped_ = active();
    return true;
  }
  bool tripped() const { return tripped_; }
  void encode(uint8_t* out) const {
    out[0] = 1; out[1] = raw_; out[2] = stable_; out[3] = dip_;
    out[4] = guard_; out[5] = high_; out[6] = tripped_ ? 1 : 0; out[7] = available_;
  }
 private:
  bool active() const { return ((raw_ ^ high_) & guard_) != 0; }
  uint8_t available_, raw_ = 0, stable_ = 0, candidate_ = 0, dip_ = 0;
  uint8_t guard_ = 0, high_ = 0;
  bool initialized_ = false, tripped_ = false;
  uint32_t changed_[8] = {};
};
}  // namespace domain

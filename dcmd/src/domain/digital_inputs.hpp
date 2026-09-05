#pragma once
#include <cstdint>

namespace domain {
// 接点はGNDへ閉じたとき1。10msのデバウンスまでを基板が受け持ち、
// どの接点をリミットとして扱うかはhostが決める。
class DigitalInputs {
 public:
  explicit DigitalInputs(uint8_t available) : available_(available) {}

  void sample(uint8_t closed, uint8_t dip, uint32_t now) {
    closed &= available_;
    dip_ = dip;
    if (!initialized_) {
      initialized_ = true;
      candidate_ = closed;
      stable_ = closed;
      for (uint8_t bit = 0; bit < 8; ++bit) changed_[bit] = now;
    }
    raw_ = closed;
    for (uint8_t bit = 0; bit < 8; ++bit) {
      const uint8_t mask = static_cast<uint8_t>(1U << bit);
      if ((candidate_ ^ closed) & mask) {
        candidate_ ^= mask;
        changed_[bit] = now;
      }
      if (now - changed_[bit] >= DEBOUNCE_MS) {
        stable_ = static_cast<uint8_t>((stable_ & ~mask) | (candidate_ & mask));
      }
    }
  }

  uint8_t raw() const { return raw_; }
  uint8_t stable() const { return stable_; }
  uint8_t dip() const { return dip_; }
  uint8_t available() const { return available_; }

 private:
  static constexpr uint32_t DEBOUNCE_MS = 10;
  uint8_t available_, raw_ = 0, stable_ = 0, candidate_ = 0, dip_ = 0;
  bool initialized_ = false;
  uint32_t changed_[8] = {};
};
}  // namespace domain

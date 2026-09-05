#pragma once
#include <cstdint>

namespace dcmd {
// TIM3の16bitカウンタを32bitの周回カウントに拡張する。
// 各サンプル間の移動は32768カウント未満とする。
class Encoder {
 public:
  void sample(uint16_t raw) {
    const uint16_t wrapped = static_cast<uint16_t>(raw - previous_);
    const int32_t delta = wrapped < 32768 ? wrapped : static_cast<int32_t>(wrapped) - 65536;
    count_ += static_cast<uint32_t>(delta);
    previous_ = raw;
  }
  uint32_t count() const { return count_; }
 private:
  uint16_t previous_ = 0;
  uint32_t count_ = 0;
};
}

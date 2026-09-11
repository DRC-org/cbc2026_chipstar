#pragma once
#include <atomic>
#include <cstdint>

namespace domain {
struct CanCommandFrame {
  uint32_t received_ms = 0;
  uint8_t length = 0;
  uint8_t data[8]{};
  bool expired(uint32_t now, uint32_t timeout_ms) const {
    return now - received_ms > timeout_ms;
  }
};

// CAN割込みがpushし、サーボ通信を行うメインループがpopする。
class CanRxQueue {
 public:
  static constexpr uint32_t CAPACITY = 32;
  bool push(const CanCommandFrame& frame) {
    const auto head = head_.load(std::memory_order_relaxed);
    if (head - tail_.load(std::memory_order_acquire) == CAPACITY) {
      markOverflow();
      return false;
    }
    frames_[head % CAPACITY] = frame;
    head_.store(head + 1, std::memory_order_release);
    return true;
  }
  bool pop(CanCommandFrame& frame) {
    const auto tail = tail_.load(std::memory_order_relaxed);
    if (tail == head_.load(std::memory_order_acquire)) return false;
    frame = frames_[tail % CAPACITY];
    tail_.store(tail + 1, std::memory_order_release);
    return true;
  }
  void markOverflow() { overflow_.store(true, std::memory_order_release); }
  bool takeOverflow() { return overflow_.exchange(false, std::memory_order_acq_rel); }
  void discard() { tail_.store(head_.load(std::memory_order_acquire), std::memory_order_release); }

 private:
  static_assert(std::atomic<uint32_t>::is_always_lock_free);
  static_assert(std::atomic<bool>::is_always_lock_free);
  CanCommandFrame frames_[CAPACITY]{};
  std::atomic<uint32_t> head_{0}, tail_{0};
  std::atomic<bool> overflow_{false};
};
}

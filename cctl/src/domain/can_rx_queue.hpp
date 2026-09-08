#pragma once
#include "domain/can_frame.hpp"
#include <atomic>
#include <cstdint>

namespace domain {
// CAN受信割込みだけがpushし、メインループだけがpopする。
class CanRxQueue {
 public:
  static constexpr uint32_t CAPACITY = 64;
  bool push(const CanFrame& frame) {
    const auto head = head_.load(std::memory_order_relaxed);
    if (head - tail_.load(std::memory_order_acquire) == CAPACITY) return false;
    frames_[head % CAPACITY] = frame;
    head_.store(head + 1, std::memory_order_release);
    return true;
  }
  bool pop(CanFrame& frame) {
    const auto tail = tail_.load(std::memory_order_relaxed);
    if (tail == head_.load(std::memory_order_acquire)) return false;
    frame = frames_[tail % CAPACITY];
    tail_.store(tail + 1, std::memory_order_release);
    return true;
  }
 private:
  static_assert(std::atomic<uint32_t>::is_always_lock_free);
  CanFrame frames_[CAPACITY]{};
  std::atomic<uint32_t> head_{0}, tail_{0};
};
}

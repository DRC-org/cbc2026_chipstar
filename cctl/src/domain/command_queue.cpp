#include "domain/command_queue.hpp"

namespace domain {

bool CommandQueue::push(const Command& command) {
    const uint32_t head = head_;
    const uint32_t next = (head + 1) % CAPACITY;
    if (next == tail_) {
        return false;
    }

    buffer_[head] = command;
    // 要素を書き終えてから位置を進める。読み手が未完成の要素を見ないため。
    head_ = next;
    return true;
}

bool CommandQueue::pop(Command& out) {
    const uint32_t tail = tail_;
    if (tail == head_) {
        return false;
    }

    out = buffer_[tail];
    tail_ = (tail + 1) % CAPACITY;
    return true;
}

}  // namespace domain

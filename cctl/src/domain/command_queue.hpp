#pragma once

#include "domain/command.hpp"

#include <cstddef>
#include <cstdint>

namespace domain {

// 割込みで積んで、メインループで取り出すための指令キュー。
// 書き手・読み手が 1 つずつ（USB 受信割込みと loop）である前提の実装。
//
// 指令の実行は CAN 送信を伴うため割込み内では行えない。かといって
// 1 個だけ保持する方式では、STOP が後続の指令に上書きされて消えうる。
class CommandQueue {
public:
    static constexpr std::size_t CAPACITY = 8;

    // 割込み側から呼ぶ。満杯なら false を返して捨てる。
    bool push(const Command& command);

    // ループ側から呼ぶ。取り出せたら true。
    bool pop(Command& out);

    bool empty() const { return head_ == tail_; }

private:
    Command buffer_[CAPACITY] = {};
    volatile uint32_t head_ = 0;  // 次に書く位置
    volatile uint32_t tail_ = 0;  // 次に読む位置
};

}  // namespace domain

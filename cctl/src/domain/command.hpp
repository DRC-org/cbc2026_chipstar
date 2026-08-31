#pragma once

#include "domain/run_state.hpp"

#include <cstddef>
#include <cstdint>

namespace domain {

// host やシリアル端末から受け取る立ち上げ用の指令。
enum class CommandKind : uint8_t {
    None,    // 解釈できない行
    Stop,    // 全軸トルクオフ
    Run,     // 通常運転へ
    Safe,    // 指令を出さない状態へ戻す
    Enable,  // 軸ごとの有効・無効
    Home,    // 原点を取り直す
    Test,    // 内蔵テストシーケンスの切替
};

struct Command {
    CommandKind kind = CommandKind::None;
    uint8_t axes = 0;    // Enable のときの対象軸ビット
    bool value = false;  // Enable / Test のときの有効・無効
};

// 1 行を指令として解釈する。解釈できなければ kind = None を返す。
// 綴りが合っていれば大文字小文字は問わない。前後の空白は無視する。
Command parseCommand(const char* line, std::size_t length);

}  // namespace domain

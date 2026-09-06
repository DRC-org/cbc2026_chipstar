#pragma once

#include "domain/run_state.hpp"

#include <cstddef>
#include <cstdint>

namespace domain {

enum class CommandKind : uint8_t {
    None,
    Hello,
    Stop,
    Run,
    Safe,
    Heartbeat,
    Enable,
    Home,
    Target,
    CanTx,
    ParamSet,
    ParamGet,
};

struct Command {
    CommandKind kind = CommandKind::None;
    uint8_t mask = 0;
    uint8_t slot = 0;
    uint8_t param_id = 0;
    uint8_t protocol_version = 0;
    bool value = false;
    float target = 0.0f;
    uint16_t can_id = 0;
    uint8_t can_length = 0;
    uint8_t can_data[8] = {};
};

Command parseCommand(const char* line, std::size_t length);

// 通信期限を延ばす指令か。
//
// hostは接続維持のためHELLOを毎秒送る。これで期限が延びると、
// ゲームパッドが外れて目標指令が止まってもWatchdogが働かない。
// 実際に出力を動かす指令だけを「生きている」証拠として扱う。
bool extendsDeadline(CommandKind kind);

}  // namespace domain

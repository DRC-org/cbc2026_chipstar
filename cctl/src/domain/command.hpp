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
    InputRead,
    InputGuard,
};

struct Command {
    CommandKind kind = CommandKind::None;
    uint8_t mask = 0;
    uint8_t input_high = 0;
    uint8_t slot = 0;
    uint8_t protocol_version = 0;
    bool value = false;
    float target = 0.0f;
    uint16_t can_id = 0;
    uint8_t can_length = 0;
    uint8_t can_data[8] = {};
};

Command parseCommand(const char* line, std::size_t length);

}  // namespace domain

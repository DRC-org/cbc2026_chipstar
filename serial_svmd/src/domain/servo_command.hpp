#pragma once

#include <cstddef>
#include <cstdint>

namespace domain {

enum class ServoCommandKind : uint8_t {
    None,
    Hello,
    Safe,
    Run,
    Stop,
    Heartbeat,
    Enable,
    Target,
    Read,
    InputRead,
    InputGuard,
};

struct ServoCommand {
    ServoCommandKind kind = ServoCommandKind::None;
    uint8_t protocol_version = 0;
    uint8_t id = 0;
    uint8_t input_mask = 0;
    uint8_t input_high = 0;
    bool enabled = false;
    uint16_t position = 0;
    uint16_t speed = 0;
    uint8_t acceleration = 0;
};

ServoCommand parseServoCommand(const char* line, std::size_t length);

}  // namespace domain

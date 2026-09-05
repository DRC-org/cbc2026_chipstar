#pragma once

#include <cstddef>
#include <cstdint>

namespace domain::servo_can {

constexpr uint8_t PROTOCOL_VERSION = 1;
constexpr uint8_t CHANNEL_COUNT = 4;
constexpr uint16_t MIN_PULSE_US = 500;
constexpr uint16_t MAX_PULSE_US = 2500;

enum class CommandKind : uint8_t {
    Stop = 0,
    Set = 1,
    Enable = 2,
    Heartbeat = 3,
    InputRead = 4,
    Invalid = 0xFF,
};

struct Command {
    CommandKind kind = CommandKind::Invalid;
    uint8_t channel = 0;
    bool enabled = false;
    uint16_t pulse_us = 0;
};

bool parse(const uint8_t* data, std::size_t length, Command& command);

}  // namespace domain::servo_can

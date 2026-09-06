#pragma once

#include "domain/parameters.hpp"

#include <cstddef>
#include <cstdint>

namespace domain::servo_can {

constexpr uint8_t PROTOCOL_VERSION = 1;
constexpr uint8_t CHANNEL_COUNT = 4;

enum class CommandKind : uint8_t {
    Stop = 0,
    Set = 1,
    Enable = 2,
    Heartbeat = 3,
    ParamSet = 4,
    Invalid = 0xFF,
};

struct Command {
    CommandKind kind = CommandKind::Invalid;
    uint8_t channel = 0;
    bool enabled = false;
    uint16_t pulse_us = 0;
    uint8_t param_id = 0;
    float value = 0.0f;
};

// パルス幅の範囲は実行時に変わるため、現在のパラメータを渡して検証する。
bool parse(const uint8_t* data, std::size_t length, Command& command,
           const Parameters& parameters);

}  // namespace domain::servo_can

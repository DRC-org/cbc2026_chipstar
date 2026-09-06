#pragma once

#include "domain/parameters.hpp"

#include <cstddef>
#include <cstdint>

namespace domain::servo_can {

constexpr uint8_t PROTOCOL_VERSION = 1;
// 基板アドレス。基板上のID用DIP A0..A1で0..3を選ぶ。address 0 は従来と同じID。
constexpr uint16_t ADDRESS_STRIDE = 0x100;
constexpr uint8_t MAX_ADDRESS = 3;
constexpr uint16_t COMMAND_ID = 0x300;
constexpr uint16_t STATUS_ID = 0x301;

constexpr uint16_t canId(uint16_t base, uint8_t address) {
    return static_cast<uint16_t>(base + ADDRESS_STRIDE * address);
}
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

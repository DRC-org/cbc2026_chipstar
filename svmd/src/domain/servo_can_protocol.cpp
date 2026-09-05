#include "domain/servo_can_protocol.hpp"

namespace domain::servo_can {
namespace {
bool reservedBytesAreZero(const uint8_t* data) {
    return data[6] == 0 && data[7] == 0;
}
}  // namespace

bool parse(const uint8_t* data, std::size_t length, Command& command) {
    command = {};
    if (data == nullptr || length != 8 || data[0] != PROTOCOL_VERSION ||
        !reservedBytesAreZero(data)) {
        return false;
    }

    const auto kind = static_cast<CommandKind>(data[1]);
    if (kind == CommandKind::Stop || kind == CommandKind::Heartbeat) {
        if (data[2] != 0 || data[3] != 0 || data[4] != 0 || data[5] != 0) return false;
        command.kind = kind;
        return true;
    }

    if (data[2] >= CHANNEL_COUNT) return false;
    command.channel = data[2];
    if (kind == CommandKind::Enable) {
        if (data[3] > 1 || data[4] != 0 || data[5] != 0) return false;
        command.kind = kind;
        command.enabled = data[3] != 0;
        return true;
    }
    if (kind == CommandKind::Set) {
        if (data[3] != 0) return false;
        const uint16_t pulse = static_cast<uint16_t>(data[4] << 8 | data[5]);
        if (pulse < MIN_PULSE_US || pulse > MAX_PULSE_US) return false;
        command.kind = kind;
        command.pulse_us = pulse;
        return true;
    }
    return false;
}

}  // namespace domain::servo_can

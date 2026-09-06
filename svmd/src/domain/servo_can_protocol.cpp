#include "domain/servo_can_protocol.hpp"

namespace domain::servo_can {
namespace {
bool reservedBytesAreZero(const uint8_t* data) {
    return data[6] == 0 && data[7] == 0;
}
}  // namespace

bool parse(const uint8_t* data, std::size_t length, Command& command,
           const Parameters& parameters) {
    command = {};
    if (data == nullptr || length != 8 || data[0] != PROTOCOL_VERSION) return false;

    const auto kind = static_cast<CommandKind>(data[1]);
    // PARAMだけはbyte 4..7をfloatとして使うため、予約byteの検査対象が違う。
    if (kind != CommandKind::ParamSet && !reservedBytesAreZero(data)) return false;
    if (kind == CommandKind::ParamSet) {
        if (data[3] != 0 || data[2] >= PARAM_COUNT) return false;
        uint32_t bits = 0;
        for (uint8_t index = 0; index < 4; ++index) bits = (bits << 8) | data[4 + index];
        float value = 0.0f;
        __builtin_memcpy(&value, &bits, sizeof(value));
        if (!Parameters::valid(data[2], value)) return false;
        command.kind = kind;
        command.param_id = data[2];
        command.value = value;
        return true;
    }
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
        if (pulse < parameters.minPulseUs() || pulse > parameters.maxPulseUs()) return false;
        command.kind = kind;
        command.pulse_us = pulse;
        return true;
    }
    return false;
}

}  // namespace domain::servo_can

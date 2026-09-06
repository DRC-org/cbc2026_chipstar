#include "domain/servo_can.hpp"

#include "device_config.hpp"

namespace domain::servo_can {
namespace {
constexpr uint16_t MAX_POSITION = 4095;

bool validId(uint8_t id) {
    return id >= 1 && id <= 253;
}

uint16_t be16(const uint8_t* data) {
    return static_cast<uint16_t>(static_cast<uint16_t>(data[0]) << 8 | data[1]);
}

void putBe16(uint16_t value, uint8_t* out) {
    out[0] = static_cast<uint8_t>(value >> 8);
    out[1] = static_cast<uint8_t>(value & 0xFF);
}
}  // namespace

bool parse(const uint8_t* data, std::size_t length, ServoCommand& out) {
    out = {};
    if (data == nullptr || length != 8 || data[0] != PROTOCOL_VERSION || data[1] > 8) {
        return false;
    }

    const auto op = static_cast<Op>(data[1]);
    const uint8_t id = data[2];
    const uint8_t flag = data[3];
    const uint16_t position = be16(&data[4]);
    const uint16_t speed = be16(&data[6]);

    // 引数を取らない指令では、余った領域を0に固定する。
    const bool bare = id == 0 && flag == 0 && position == 0 && speed == 0;

    switch (op) {
        case Op::Hello:
            if (!bare) return false;
            out.kind = ServoCommandKind::Hello;
            out.protocol_version = PROTOCOL_VERSION;
            return true;
        case Op::Safe:
            if (!bare) return false;
            out.kind = ServoCommandKind::Safe;
            return true;
        case Op::Run:
            if (!bare) return false;
            out.kind = ServoCommandKind::Run;
            return true;
        case Op::Stop:
            if (!bare) return false;
            out.kind = ServoCommandKind::Stop;
            return true;
        case Op::Heartbeat:
            if (!bare) return false;
            out.kind = ServoCommandKind::Heartbeat;
            return true;
        case Op::InputRead:
            if (!bare) return false;
            out.kind = ServoCommandKind::InputRead;
            return true;
        case Op::Read:
            if (!validId(id) || flag != 0 || position != 0 || speed != 0) return false;
            out.kind = ServoCommandKind::Read;
            out.id = id;
            return true;
        case Op::Enable:
            if (!validId(id) || flag > 1 || position != 0 || speed != 0) return false;
            out.kind = ServoCommandKind::Enable;
            out.id = id;
            out.enabled = flag != 0;
            return true;
        case Op::Target:
            if (!validId(id) || flag > config::MAX_ACCELERATION || position > MAX_POSITION ||
                speed > config::MAX_SPEED) {
                return false;
            }
            out.kind = ServoCommandKind::Target;
            out.id = id;
            out.acceleration = flag;
            out.position = position;
            out.speed = speed;
            return true;
    }
    return false;
}

void encodeStatus(Status status, uint8_t mode, uint8_t servo_count, uint8_t* out) {
    out[0] = PROTOCOL_VERSION;
    out[1] = static_cast<uint8_t>(status);
    out[2] = mode;
    out[3] = servo_count;
    out[4] = 0;
    out[5] = 0;
    out[6] = 0;
    out[7] = 0;
}

void encodePosition(uint8_t id, uint16_t position, bool enabled, uint8_t error, uint8_t* out) {
    out[0] = PROTOCOL_VERSION;
    out[1] = id;
    putBe16(position, &out[2]);
    out[4] = enabled ? 1 : 0;
    out[5] = error;
    out[6] = 0;
    out[7] = 0;
}

void encodeInputs(uint8_t raw, uint8_t stable, uint8_t dip, uint8_t available, uint8_t* out) {
    out[0] = PROTOCOL_VERSION;
    out[1] = raw;
    out[2] = stable;
    out[3] = dip;
    out[4] = available;
    out[5] = 0;
    out[6] = 0;
    out[7] = 0;
}

}  // namespace domain::servo_can

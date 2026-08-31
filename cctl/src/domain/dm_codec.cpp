#include "domain/dm_codec.hpp"

#include <cstring>

namespace domain::dm {
namespace {
uint32_t fullScale(uint8_t bits) {
    return bits >= 32 ? 0xFFFFFFFFu : ((1u << bits) - 1u);
}

void writeFloatLe(float value, uint8_t* out) {
    std::memcpy(out, &value, sizeof(float));
}
}  // namespace

uint32_t floatToRaw(float value, float min, float max, uint8_t bits) {
    const float span = max - min;
    if (span <= 0.0f) {
        return 0;
    }

    const uint32_t scale = fullScale(bits);
    float ratio = (value - min) / span;
    // 範囲外は端に張り付かせる。巻き戻して逆向きの指令になるのを防ぐ。
    if (!(ratio > 0.0f)) {
        return 0;  // NaN もここに落ちる
    }
    if (ratio >= 1.0f) {
        return scale;
    }
    return static_cast<uint32_t>(ratio * static_cast<float>(scale) + 0.5f);
}

float rawToFloat(uint32_t raw, float min, float max, uint8_t bits) {
    const uint32_t scale = fullScale(bits);
    if (scale == 0) {
        return min;
    }
    return static_cast<float>(raw) * (max - min) / static_cast<float>(scale) + min;
}

Feedback decodeFeedback(const uint8_t data[8], const Range& range) {
    // D0: ID|ERR<<4, D1-2: POS(16), D3: VEL[11:4], D4: VEL[3:0]|T[11:8], D5: T[7:0]
    Feedback feedback;
    feedback.id = data[0] & 0x0F;
    feedback.error = (data[0] >> 4) & 0x0F;

    const uint32_t pos_raw = (static_cast<uint32_t>(data[1]) << 8) | data[2];
    const uint32_t vel_raw = (static_cast<uint32_t>(data[3]) << 4) | (data[4] >> 4);
    const uint32_t tau_raw = (static_cast<uint32_t>(data[4] & 0x0F) << 8) | data[5];

    feedback.position_rad = rawToFloat(pos_raw, -range.p_max, range.p_max, 16);
    feedback.velocity_rad_s = rawToFloat(vel_raw, -range.v_max, range.v_max, 12);
    feedback.torque_nm = rawToFloat(tau_raw, -range.t_max, range.t_max, 12);
    feedback.mos_temperature_c = data[6];
    feedback.rotor_temperature_c = data[7];
    return feedback;
}

void encodeMit(float position_rad, float velocity_rad_s, float kp, float kd, float torque_nm,
               const Range& range, uint8_t out[8]) {
    const uint32_t p = floatToRaw(position_rad, -range.p_max, range.p_max, 16);
    const uint32_t v = floatToRaw(velocity_rad_s, -range.v_max, range.v_max, 12);
    const uint32_t kp_raw = floatToRaw(kp, KP_MIN, KP_MAX, 12);
    const uint32_t kd_raw = floatToRaw(kd, KD_MIN, KD_MAX, 12);
    const uint32_t t = floatToRaw(torque_nm, -range.t_max, range.t_max, 12);

    out[0] = static_cast<uint8_t>(p >> 8);
    out[1] = static_cast<uint8_t>(p & 0xFF);
    out[2] = static_cast<uint8_t>(v >> 4);
    out[3] = static_cast<uint8_t>(((v & 0x0F) << 4) | (kp_raw >> 8));
    out[4] = static_cast<uint8_t>(kp_raw & 0xFF);
    out[5] = static_cast<uint8_t>(kd_raw >> 4);
    out[6] = static_cast<uint8_t>(((kd_raw & 0x0F) << 4) | (t >> 8));
    out[7] = static_cast<uint8_t>(t & 0xFF);
}

void encodePositionVelocity(float position_rad, float velocity_limit, uint8_t out[8]) {
    writeFloatLe(position_rad, &out[0]);
    writeFloatLe(velocity_limit, &out[4]);
}

void encodeVelocity(float velocity_rad_s, uint8_t out[8]) {
    for (int i = 0; i < 8; ++i) {
        out[i] = 0;
    }
    writeFloatLe(velocity_rad_s, &out[0]);
}

void encodeSpecial(uint8_t command, uint8_t out[8]) {
    for (int i = 0; i < 7; ++i) {
        out[i] = 0xFF;
    }
    out[7] = command;
}

void encodeConfig(uint16_t can_id, uint8_t command, uint8_t rid, uint32_t value, uint8_t out[8]) {
    out[0] = static_cast<uint8_t>(can_id & 0xFF);
    out[1] = static_cast<uint8_t>((can_id >> 8) & 0xFF);
    out[2] = command;
    out[3] = rid;
    out[4] = static_cast<uint8_t>(value & 0xFF);
    out[5] = static_cast<uint8_t>((value >> 8) & 0xFF);
    out[6] = static_cast<uint8_t>((value >> 16) & 0xFF);
    out[7] = static_cast<uint8_t>((value >> 24) & 0xFF);
}

bool isConfigReply(uint16_t can_id, const uint8_t data[8]) {
    const bool id_matches = data[0] == (can_id & 0xFF) && data[1] == ((can_id >> 8) & 0xFF);
    const bool is_config_command =
        data[2] == CONFIG_READ || data[2] == CONFIG_WRITE || data[2] == CONFIG_STORE;
    return id_matches && is_config_command;
}

uint8_t configReplyRegister(const uint8_t data[8]) { return data[3]; }

uint32_t configReplyValue(const uint8_t data[8]) {
    return static_cast<uint32_t>(data[4]) | (static_cast<uint32_t>(data[5]) << 8) |
           (static_cast<uint32_t>(data[6]) << 16) | (static_cast<uint32_t>(data[7]) << 24);
}

}  // namespace domain::dm

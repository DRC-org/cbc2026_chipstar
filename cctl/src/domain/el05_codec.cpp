#include "domain/el05_codec.hpp"

#include <cstring>

namespace domain::el05 {

uint32_t buildCanId(uint8_t comm_type, uint16_t data_area_2, uint8_t target_id) {
    return ((static_cast<uint32_t>(comm_type) & 0x1F) << 24) |
           ((static_cast<uint32_t>(data_area_2) & 0xFFFF) << 8) |
           static_cast<uint32_t>(target_id);
}

uint8_t commType(uint32_t ext_id) { return static_cast<uint8_t>((ext_id >> 24) & 0x1F); }
uint8_t targetId(uint32_t ext_id) { return static_cast<uint8_t>(ext_id & 0xFF); }
uint16_t dataArea2(uint32_t ext_id) { return static_cast<uint16_t>((ext_id >> 8) & 0xFFFF); }

float uint16ToFloat(uint16_t raw, float min, float max) {
    return static_cast<float>(raw) * (max - min) / 65535.0f + min;
}

uint16_t floatToUint16(float value, float min, float max) {
    const float span = max - min;
    if (span <= 0.0f) {
        return 0;
    }
    const float ratio = (value - min) / span;
    if (!(ratio > 0.0f)) {
        return 0;  // NaN もここに落ちる
    }
    if (ratio >= 1.0f) {
        return 65535;
    }
    return static_cast<uint16_t>(ratio * 65535.0f + 0.5f);
}

Feedback decodeFeedback(uint32_t ext_id, const uint8_t data[8]) {
    Feedback feedback;
    // フィードバックの data_area_2 には下位 8bit にモータID、上位に状態が載る。
    feedback.motor_id = static_cast<uint8_t>((ext_id >> 8) & 0xFF);
    feedback.fault_bits = static_cast<uint8_t>((ext_id >> 16) & 0x3F);

    const uint16_t pos_raw = (static_cast<uint16_t>(data[0]) << 8) | data[1];
    const uint16_t vel_raw = (static_cast<uint16_t>(data[2]) << 8) | data[3];
    const uint16_t tau_raw = (static_cast<uint16_t>(data[4]) << 8) | data[5];
    const uint16_t tmp_raw = (static_cast<uint16_t>(data[6]) << 8) | data[7];

    feedback.position_rad = uint16ToFloat(pos_raw, POSITION_MIN, POSITION_MAX);
    feedback.velocity_rad_s = uint16ToFloat(vel_raw, VELOCITY_MIN, VELOCITY_MAX);
    feedback.torque_nm = uint16ToFloat(tau_raw, TORQUE_MIN, TORQUE_MAX);
    feedback.temperature_c = static_cast<float>(tmp_raw) / 10.0f;
    return feedback;
}

void encodeParamFloat(uint16_t param, float value, uint8_t out[8]) {
    for (int i = 0; i < 8; ++i) {
        out[i] = 0;
    }
    out[0] = static_cast<uint8_t>(param & 0xFF);
    out[1] = static_cast<uint8_t>((param >> 8) & 0xFF);
    std::memcpy(&out[4], &value, sizeof(float));
}

void encodeParamU8(uint16_t param, uint8_t value, uint8_t out[8]) {
    for (int i = 0; i < 8; ++i) {
        out[i] = 0;
    }
    out[0] = static_cast<uint8_t>(param & 0xFF);
    out[1] = static_cast<uint8_t>((param >> 8) & 0xFF);
    out[4] = value;
}

void encodeParamRequest(uint16_t param, uint8_t out[8]) {
    for (int i = 0; i < 8; ++i) {
        out[i] = 0;
    }
    out[0] = static_cast<uint8_t>(param & 0xFF);
    out[1] = static_cast<uint8_t>((param >> 8) & 0xFF);
}

void encodeSave(uint8_t out[8]) {
    // マニュアル指定の固定列。
    for (uint8_t i = 0; i < 8; ++i) {
        out[i] = static_cast<uint8_t>(i + 1);
    }
}

uint16_t paramReplyIndex(const uint8_t data[8]) {
    return static_cast<uint16_t>(data[0]) | (static_cast<uint16_t>(data[1]) << 8);
}

float paramReplyFloat(const uint8_t data[8]) {
    float value = 0.0f;
    std::memcpy(&value, &data[4], sizeof(float));
    return value;
}

}  // namespace domain::el05

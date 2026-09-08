#include "domain/parameters.hpp"

#include "device_config.hpp"

#include <cmath>

namespace domain {
namespace {

struct Range {
    float min;
    float max;
};

// 値そのものの安全性ではなく、FWが成立するかどうかで決めた範囲。
// 制御周期は0だと分母や待ち時間が壊れ、CAN IDは規格外だと送信できない。
constexpr Range RANGES[PARAM_COUNT] = {
    {0.0f, 1.0e6f},    // M3508PosKp
    {0.0f, 1.0e6f},    // M3508PosKi
    {0.0f, 1.0e6f},    // M3508PosKd
    {1.0f, 20000.0f},  // M3508MaxRpm
    {0.0f, 1.0e6f},    // M3508VelKp
    {0.0f, 1.0e6f},    // M3508VelKi
    {0.0f, 1.0e6f},    // M3508VelKd
    {0.0f, 20000.0f},  // M3508MaxCurrentMa
    {0.0f, 1.0e6f},    // El05LocKp
    {0.0f, 1000.0f},   // El05LimitSpd
    {0.0f, 100.0f},    // El05LimitCur
    {0.001f, 1.0e6f},  // DmPMax
    {0.001f, 1.0e6f},  // DmVMax
    {0.001f, 1.0e6f},  // DmTMax
    {0.0f, 1.0e6f},    // DmPosVelLimit
    {0.0f, 0.0f},      // Reserved15
    {0.0f, 0.0f},      // Reserved16
    {0.0f, 0.0f},      // Reserved17
    {0.0f, 0.0f},      // Reserved18
    {0.0f, 0.0f},      // Reserved19
    {0.0f, 0.0f},      // Reserved20
    {1.0f, 8.0f},      // C620EscId
    {0.0f, 2047.0f},   // DmCanId
    {0.0f, 2047.0f},   // DmMstId
    {0.0f, 255.0f},    // El05MotorId
    {0.0f, 255.0f},    // El05HostId
    {1.0f, 10000.0f},  // M3508PeriodMs
    {1.0f, 10000.0f},  // DmPeriodMs
    {1.0f, 10000.0f},  // El05PeriodMs
    {1.0f, 10000.0f},  // TelemetryPeriodMs
    {1.0f, 60000.0f},  // WatchdogMs
    {10.0f, 60000.0f}, // FeedbackTimeoutMs
    {0.0f, 200.0f},    // M3508MaxTemperatureC
    {0.0f, 1.0e6f}, {0.0f, 1.0e6f}, {0.0f, 1.0e6f},
    {1.0f, 20000.0f},
    {0.0f, 1.0e6f}, {0.0f, 1.0e6f}, {0.0f, 1.0e6f},
    {0.0f, 20000.0f}, {1.0f, 8.0f}, {0.0f, 200.0f},
};

constexpr float DEFAULTS[PARAM_COUNT] = {
    config::m3508::POS_KP,
    config::m3508::POS_KI,
    config::m3508::POS_KD,
    config::m3508::MAX_RPM,
    config::m3508::VEL_KP,
    config::m3508::VEL_KI,
    config::m3508::VEL_KD,
    config::m3508::MAX_CURRENT_MA,
    config::el05::LOC_KP,
    config::el05::LIMIT_SPD,
    config::el05::LIMIT_CUR,
    config::dm::P_MAX,
    config::dm::V_MAX,
    config::dm::T_MAX,
    config::dm::POS_VEL_LIMIT,
    0.0f,
    0.0f,
    0.0f,
    0.0f,
    0.0f,
    0.0f,
    static_cast<float>(config::can_id::C620_ESC_ID),
    static_cast<float>(config::can_id::DM_CAN_ID),
    static_cast<float>(config::can_id::DM_MST_ID),
    static_cast<float>(config::can_id::EL05_MOTOR_ID),
    static_cast<float>(config::can_id::EL05_HOST_ID),
    static_cast<float>(config::period::M3508_MS),
    static_cast<float>(config::period::DM_MS),
    static_cast<float>(config::period::EL05_MS),
    static_cast<float>(config::period::TELEMETRY_MS),
    static_cast<float>(config::period::WATCHDOG_MS),
    200.0f,
    80.0f,
    8.0f, 0.0f, 0.0f, 500.0f, 7.0f, 0.5f, 0.05f, 1000.0f, 2.0f, 80.0f,
};

}  // namespace

bool requiresSafe(uint8_t id) {
    switch (static_cast<ParamId>(id)) {
        case ParamId::C620Slot2EscId:
        case ParamId::C620EscId:
        case ParamId::DmCanId:
        case ParamId::DmMstId:
        case ParamId::El05MotorId:
        case ParamId::El05HostId:
            return true;
        default:
            return false;
    }
}

bool Parameters::valid(uint8_t id, float value) {
    if (id >= PARAM_COUNT || (id >= 15 && id <= 20) || !std::isfinite(value)) return false;
    const Range& range = RANGES[id];
    return value >= range.min && value <= range.max;
}

bool Parameters::restore(const float* values, std::size_t count) {
    if (count != PARAM_COUNT) return false;
    for (uint8_t id = 0; id < PARAM_COUNT; ++id) {
        // 旧FWが保存したslot可動域は読み捨て、後続IDの保存値は引き継ぐ。
        if (id >= 15 && id <= 20) continue;
        if (!valid(id, values[id])) return false;
    }
    const float first = values[static_cast<uint8_t>(ParamId::C620EscId)];
    const float second = values[static_cast<uint8_t>(ParamId::C620Slot2EscId)];
    if (first == second || std::floor(first) != first || std::floor(second) != second) return false;
    for (uint8_t id = 0; id < PARAM_COUNT; ++id) {
        values_[id] = (id >= 15 && id <= 20) ? DEFAULTS[id] : values[id];
    }
    return true;
}

void Parameters::reset() {
    for (std::size_t index = 0; index < PARAM_COUNT; ++index) {
        values_[index] = DEFAULTS[index];
    }
}

bool Parameters::set(uint8_t id, float value) {
    if (!valid(id, value)) return false;
    if (id == static_cast<uint8_t>(ParamId::C620EscId) ||
        id == static_cast<uint8_t>(ParamId::C620Slot2EscId)) {
        const auto other = id == static_cast<uint8_t>(ParamId::C620EscId)
            ? ParamId::C620Slot2EscId : ParamId::C620EscId;
        if (std::floor(value) != value || value == get(other)) return false;
    }
    values_[id] = value;
    return true;
}

}  // namespace domain

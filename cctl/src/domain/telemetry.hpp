#pragma once

#include "domain/run_state.hpp"

#include <cstddef>
#include <cstdint>

namespace domain {

struct Telemetry {
    uint32_t uptime_ms = 0;
    float targets[SLOT_COUNT] = {};
    float measured[SLOT_COUNT] = {};
    uint8_t enabled_slots = 0;
    RunMode mode = RunMode::Safe;
    uint8_t error_bits = 0;
};

constexpr std::size_t TELEMETRY_LINE_CAPACITY = 144;

std::size_t formatFixed3(float value, char* out, std::size_t capacity);
std::size_t formatTelemetry(const Telemetry& telemetry, char* out, std::size_t capacity);

}  // namespace domain

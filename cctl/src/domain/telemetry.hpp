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
    // slotごとの異常。混ぜるとどのモータの異常か分からなくなる。
    uint8_t error_bits[SLOT_COUNT] = {};
    uint8_t contacts = 0;  // SW1..SW3の10ms安定値。閉で1。
    uint8_t stale_slots = 0;  // フィードバックが途絶えたslotのbit mask。
    uint8_t buses = 0;  // 使えるCANバス。bit0=FDCAN1, bit1=FDCAN2。
};

constexpr std::size_t TELEMETRY_LINE_CAPACITY = 144;

// PARAM応答はhostの指令と同じ小数5桁で返す。STATEの表示精度とは分離する。
std::size_t formatFixed5(float value, char* out, std::size_t capacity);
std::size_t formatFixed3(float value, char* out, std::size_t capacity);
std::size_t formatTelemetry(const Telemetry& telemetry, char* out, std::size_t capacity);

}  // namespace domain

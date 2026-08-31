#pragma once

#include "domain/run_state.hpp"

#include <cstddef>
#include <cstdint>

namespace domain {

// 1 回分のテレメトリ。目標値と実測値を並べて送る。
struct Telemetry {
    uint32_t uptime_ms = 0;
    float r_target_mm = 0.0f;
    float r_measured_mm = 0.0f;
    float theta_target_deg = 0.0f;
    float theta_measured_deg = 0.0f;
    float z_target_mm = 0.0f;
    float z_measured_mm = 0.0f;
    uint8_t enabled_axes = 0;
    RunMode mode = RunMode::Safe;
    uint8_t error_bits = 0;
};

// テレメトリ 1 行に必要な長さ（終端を含む）。
constexpr std::size_t TELEMETRY_LINE_CAPACITY = 128;

// 小数 3 桁の固定小数として書き、書いた長さを返す。capacity 不足なら 0。
// newlib-nano の printf は %f を持たないので、整数演算で組み立てる。
// 有限でない値は "nan" と書く。制御が発散したことを現場で見落とさないため。
std::size_t formatFixed3(float value, char* out, std::size_t capacity);

// テレメトリ行を組み立て、書いた長さを返す。capacity 不足なら 0。
// 行末の改行は付けない。
std::size_t formatTelemetry(const Telemetry& telemetry, char* out, std::size_t capacity);

}  // namespace domain

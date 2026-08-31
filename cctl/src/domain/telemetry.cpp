#include "domain/telemetry.hpp"

#include <cstdio>
#include <cstring>

namespace domain {
namespace {
// 1000 倍して int32 に収まる範囲。これを超える値は数値として信用しない。
constexpr float FIXED3_LIMIT = 2000000.0f;

const char* modeName(RunMode mode) {
    switch (mode) {
        case RunMode::Safe: return "SAFE";
        case RunMode::Run: return "RUN";
        case RunMode::Stop: return "STOP";
    }
    return "SAFE";
}

// out へ書けるなら書いて長さを進める。書けなければ false。
bool appendFixed3(char* out, std::size_t capacity, std::size_t& length, float value) {
    if (length >= capacity) {
        return false;
    }
    const std::size_t written = formatFixed3(value, &out[length], capacity - length);
    if (written == 0) {
        return false;
    }
    length += written;
    return true;
}
}  // namespace

std::size_t formatFixed3(float value, char* out, std::size_t capacity) {
    if (out == nullptr || capacity == 0) {
        return 0;
    }

    // NaN は自分自身と等しくない。無限大と過大な値もここで弾く。
    const bool representable =
        (value == value) && (value > -FIXED3_LIMIT) && (value < FIXED3_LIMIT);
    if (!representable) {
        if (capacity < 4) {
            return 0;
        }
        std::memcpy(out, "nan", 4);
        return 3;
    }

    const bool negative = value < 0.0f;
    const float rounding = negative ? -0.5f : 0.5f;
    int32_t scaled = static_cast<int32_t>(value * 1000.0f + rounding);
    if (scaled < 0) {
        scaled = -scaled;
    }

    const int32_t integer_part = scaled / 1000;
    const int32_t fraction_part = scaled % 1000;

    const int written = std::snprintf(out, capacity, "%s%ld.%03ld", negative ? "-" : "",
                                      static_cast<long>(integer_part),
                                      static_cast<long>(fraction_part));
    if (written < 0 || static_cast<std::size_t>(written) >= capacity) {
        return 0;
    }
    return static_cast<std::size_t>(written);
}

std::size_t formatTelemetry(const Telemetry& telemetry, char* out, std::size_t capacity) {
    if (out == nullptr || capacity == 0) {
        return 0;
    }

    std::size_t length = 0;
    const auto append = [&](const char* format, auto... args) {
        if (length >= capacity) {
            return false;
        }
        const int written = std::snprintf(&out[length], capacity - length, format, args...);
        if (written < 0 || static_cast<std::size_t>(written) >= capacity - length) {
            return false;
        }
        length += static_cast<std::size_t>(written);
        return true;
    };

    const auto text = [&](const char* literal) { return append("%s", literal); };

    const bool ok =
        append("ST t=%lu r=", static_cast<unsigned long>(telemetry.uptime_ms)) &&
        appendFixed3(out, capacity, length, telemetry.r_target_mm) && text("/") &&
        appendFixed3(out, capacity, length, telemetry.r_measured_mm) && text(" th=") &&
        appendFixed3(out, capacity, length, telemetry.theta_target_deg) && text("/") &&
        appendFixed3(out, capacity, length, telemetry.theta_measured_deg) && text(" z=") &&
        appendFixed3(out, capacity, length, telemetry.z_target_mm) && text("/") &&
        appendFixed3(out, capacity, length, telemetry.z_measured_mm) &&
        append(" en=%u mode=%s err=%02X", static_cast<unsigned>(telemetry.enabled_axes),
               modeName(telemetry.mode), static_cast<unsigned>(telemetry.error_bits));

    return ok ? length : 0;
}

}  // namespace domain

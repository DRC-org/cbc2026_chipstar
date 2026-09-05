#include "domain/telemetry.hpp"

#include <cstdio>
#include <cstring>

namespace domain {
namespace {
constexpr float FIXED3_LIMIT = 2000000.0f;

const char* modeName(RunMode mode) {
    switch (mode) {
        case RunMode::Safe: return "SAFE";
        case RunMode::Run: return "RUN";
        case RunMode::Stop: return "STOP";
    }
    return "SAFE";
}

bool appendFixed3(char* out, std::size_t capacity, std::size_t& length, float value) {
    if (length >= capacity) return false;
    const std::size_t written = formatFixed3(value, &out[length], capacity - length);
    if (written == 0) return false;
    length += written;
    return true;
}
}  // namespace

std::size_t formatFixed3(float value, char* out, std::size_t capacity) {
    if (out == nullptr || capacity == 0) return 0;
    const bool representable =
        (value == value) && (value > -FIXED3_LIMIT) && (value < FIXED3_LIMIT);
    if (!representable) {
        if (capacity < 4) return 0;
        std::memcpy(out, "nan", 4);
        return 3;
    }

    const bool negative = value < 0.0f;
    int32_t scaled = static_cast<int32_t>(value * 1000.0f + (negative ? -0.5f : 0.5f));
    if (scaled < 0) scaled = -scaled;
    const int written = std::snprintf(out, capacity, "%s%ld.%03ld", negative ? "-" : "",
                                      static_cast<long>(scaled / 1000),
                                      static_cast<long>(scaled % 1000));
    if (written < 0 || static_cast<std::size_t>(written) >= capacity) return 0;
    return static_cast<std::size_t>(written);
}

std::size_t formatTelemetry(const Telemetry& telemetry, char* out, std::size_t capacity) {
    if (out == nullptr || capacity == 0) return 0;
    std::size_t length = 0;
    const auto append = [&](const char* format, auto... args) {
        if (length >= capacity) return false;
        const int written = std::snprintf(&out[length], capacity - length, format, args...);
        if (written < 0 || static_cast<std::size_t>(written) >= capacity - length) return false;
        length += static_cast<std::size_t>(written);
        return true;
    };
    const auto text = [&](const char* literal) { return append("%s", literal); };

    bool ok = append("STATE t=%lu mode=%s en=%u", static_cast<unsigned long>(telemetry.uptime_ms),
                     modeName(telemetry.mode), static_cast<unsigned>(telemetry.enabled_slots));
    for (uint8_t slot = 0; ok && slot < SLOT_COUNT; ++slot) {
        ok = append(" a%u=", static_cast<unsigned>(slot)) &&
             appendFixed3(out, capacity, length, telemetry.targets[slot]) && text("/") &&
             appendFixed3(out, capacity, length, telemetry.measured[slot]);
    }
    ok = ok && append(" err=%02X sw=%u", static_cast<unsigned>(telemetry.error_bits),
                      static_cast<unsigned>(telemetry.contacts));
    return ok ? length : 0;
}

}  // namespace domain

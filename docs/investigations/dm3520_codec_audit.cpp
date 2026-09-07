// DM SDKの復号式とCCTL実装の数値比較。モータの物理単位は検証しない。
// SDK: dmBots/motor-sdk @ 0b2ede457bdbf0882e29ab9958ab8fda047b7f4a
#include "domain/dm_codec.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <iostream>

int main() {
    for (const float range : {12.5f, 256.0f, 2048.0f}) {
        double maximum_error = 0;
        for (uint32_t raw = 0; raw <= 65535; ++raw) {
            // SDKはPython floatで計算し、最後にnumpy.float32へ変換する。
            const float reference = static_cast<float>(
                (static_cast<double>(raw) / 65535.0) * (2.0 * range) - range);
            const uint8_t frame[8] = {
                0x19, static_cast<uint8_t>(raw >> 8), static_cast<uint8_t>(raw),
                0x7f, 0xf7, 0xff, 36, 27};
            const auto decoded = domain::dm::decodeFeedback(frame, {range, 200, 10});
            maximum_error = std::max(maximum_error,
                std::abs(static_cast<double>(decoded.position_rad) - reference));
        }
        std::cout << "PMAX=" << range << " cases=65536 max_absolute_error="
                  << maximum_error << '\n';
        if (maximum_error > range * 2e-7) return 1;
    }

    uint8_t payload[8] = {};
    domain::dm::encodePositionVelocity(3.14159f, 40.0f, payload);
    // IEEE754 floatの既知値。マニュアルp.9の位置末尾はこれより1 ULP小さい。
    constexpr std::array<uint8_t, 8> expected{0xd0, 0x0f, 0x49, 0x40, 0, 0, 0x20, 0x42};
    if (!std::equal(expected.begin(), expected.end(), payload)) return 2;
    std::cout << "position_velocity_payload=matches_ieee754\n";
}

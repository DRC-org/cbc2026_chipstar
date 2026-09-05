#include "doctest.h"

#include "domain/telemetry.hpp"

#include <limits>
#include <string>

using domain::RunMode;
using domain::Telemetry;

namespace {
std::string fixed3(float value) {
    char buffer[32] = {};
    const std::size_t length = domain::formatFixed3(value, buffer, sizeof(buffer));
    return std::string(buffer, length);
}

std::string line(const Telemetry& telemetry) {
    char buffer[domain::TELEMETRY_LINE_CAPACITY] = {};
    const std::size_t length = domain::formatTelemetry(telemetry, buffer, sizeof(buffer));
    return std::string(buffer, length);
}

Telemetry sample() {
    Telemetry telemetry;
    telemetry.uptime_ms = 12345;
    telemetry.targets[0] = 1.2f;
    telemetry.measured[0] = 1.1f;
    telemetry.targets[1] = -45.0f;
    telemetry.measured[1] = -44.2f;
    telemetry.targets[2] = 0.5f;
    telemetry.measured[2] = 0.4f;
    telemetry.enabled_slots = domain::slot_bit::ALL;
    telemetry.mode = RunMode::Run;
    telemetry.error_bits = 0x0A;
    return telemetry;
}
}  // namespace

TEST_CASE("固定小数を丸めて書く") {
    CHECK(fixed3(12.3f) == "12.300");
    CHECK(fixed3(-0.25f) == "-0.250");
    CHECK(fixed3(0.0006f) == "0.001");
}

TEST_CASE("有限でない値をnanと書く") {
    CHECK(fixed3(std::numeric_limits<float>::quiet_NaN()) == "nan");
    CHECK(fixed3(std::numeric_limits<float>::infinity()) == "nan");
}

TEST_CASE("slot単位の状態を出力する") {
    CHECK(line(sample()) ==
          "STATE t=12345 mode=RUN en=7 a0=1.200/1.100 a1=-45.000/-44.200 "
          "a2=0.500/0.400 err=0A");
}

TEST_CASE("容量不足では出力しない") {
    char buffer[8] = {};
    CHECK(domain::formatTelemetry(sample(), buffer, sizeof(buffer)) == 0);
}

TEST_CASE("最大構成が規定容量に収まる") {
    Telemetry telemetry;
    telemetry.uptime_ms = 4294967295u;
    for (uint8_t slot = 0; slot < domain::SLOT_COUNT; ++slot) {
        telemetry.targets[slot] = -999999.0f;
        telemetry.measured[slot] = 999999.0f;
    }
    telemetry.enabled_slots = domain::slot_bit::ALL;
    telemetry.mode = RunMode::Stop;
    telemetry.error_bits = 0xFF;
    CHECK(!line(telemetry).empty());
}

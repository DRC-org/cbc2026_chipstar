#include "doctest.h"

#include "domain/telemetry.hpp"

#include <limits>
#include <string>
#include <utility>

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
    telemetry.error_bits[0] = 0x0A;
    telemetry.error_bits[2] = 0x03;
    telemetry.contacts = 5;
    telemetry.stale_slots = 2;
    telemetry.buses = 3;
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

TEST_CASE("slot単位の状態と接点を出力する") {
    CHECK(line(sample()) ==
          "STATE t=12345 mode=RUN en=7 a0=1.200/1.100 a1=-45.000/-44.200 "
          "a2=0.500/0.400 err=0A,00,03 sw=5 stale=2 can=3 hold=0");
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
    for (auto& bits : telemetry.error_bits) bits = 0xFF;
    telemetry.contacts = 7;
    telemetry.stale_slots = 7;
    telemetry.buses = 3;
    CHECK(!line(telemetry).empty());
}

TEST_CASE("設定応答は小さいゲインと大きい上限値を小数5桁で返す") {
    for (const auto& item : {std::pair<float, const char*>{0.0005f, "0.00050"},
                            {0.00001f, "0.00001"}, {-0.0005f, "-0.00050"},
                            {1000000.0f, "1000000.00000"}, {0.0f, "0.00000"}}) {
        char buffer[32] = {};
        const auto length = domain::formatFixed5(item.first, buffer, sizeof(buffer));
        CHECK(std::string(buffer, length) == item.second);
    }
}

TEST_CASE("設定応答の不正値と容量不足を処理する") {
    char buffer[32] = {};
    for (float value : {std::numeric_limits<float>::quiet_NaN(),
                        std::numeric_limits<float>::infinity(), 2000000.0f}) {
        CHECK(domain::formatFixed5(value, buffer, sizeof(buffer)) == 3);
        CHECK(std::string(buffer) == "nan");
    }
    CHECK(domain::formatFixed5(0.0005f, buffer, 7) == 0);
    CHECK(domain::formatFixed5(0.0005f, buffer, 8) == 7);
    CHECK(domain::formatFixed5(0.0005f, nullptr, 0) == 0);
}

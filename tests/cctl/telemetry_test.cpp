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
    const std::size_t length =
        domain::formatTelemetry(telemetry, buffer, sizeof(buffer));
    return std::string(buffer, length);
}

Telemetry sample() {
    Telemetry t;
    t.uptime_ms = 12345;
    t.r_target_mm = 12.3f;
    t.r_measured_mm = 11.8f;
    t.theta_target_deg = 45.0f;
    t.theta_measured_deg = 44.2f;
    t.z_target_mm = 100.0f;
    t.z_measured_mm = 99.1f;
    t.enabled_axes = domain::axis_bit::ALL;
    t.mode = RunMode::Run;
    t.error_bits = 0;
    return t;
}
}  // namespace

TEST_CASE("固定小数は常に 3 桁の小数部を持つ") {
    CHECK(fixed3(0.0f) == "0.000");
    CHECK(fixed3(1.0f) == "1.000");
    CHECK(fixed3(12.3f) == "12.300");
    CHECK(fixed3(100.5f) == "100.500");
}

TEST_CASE("固定小数は負号を保つ") {
    CHECK(fixed3(-1.0f) == "-1.000");
    CHECK(fixed3(-0.25f) == "-0.250");
}

TEST_CASE("固定小数は 4 桁目を四捨五入する") {
    CHECK(fixed3(1.23456f) == "1.235");
    CHECK(fixed3(-1.23456f) == "-1.235");
    CHECK(fixed3(0.0004f) == "0.000");
    CHECK(fixed3(0.0006f) == "0.001");
}

TEST_CASE("有限でない値は nan と書く") {
    // 制御が発散したことを現場で見落とさないため、0 に丸めない。
    const float inf = std::numeric_limits<float>::infinity();
    CHECK(fixed3(std::numeric_limits<float>::quiet_NaN()) == "nan");
    CHECK(fixed3(inf) == "nan");
    CHECK(fixed3(-inf) == "nan");
}

TEST_CASE("表現できない大きさは nan として扱う") {
    // int32 に収まらない値を黙って巻き戻すと、嘘の数値が現場に出る。
    CHECK(fixed3(1.0e9f) == "nan");
    CHECK(fixed3(-1.0e9f) == "nan");
}

TEST_CASE("書き込み先が足りなければ 0 を返す") {
    char buffer[4] = {};
    CHECK(domain::formatFixed3(12.345f, buffer, sizeof(buffer)) == 0);
    CHECK(domain::formatTelemetry(sample(), buffer, sizeof(buffer)) == 0);
}

TEST_CASE("テレメトリ行は目標と実測を並べる") {
    CHECK(line(sample()) ==
          "ST t=12345 r=12.300/11.800 th=45.000/44.200 z=100.000/99.100 "
          "en=7 mode=RUN err=00");
}

TEST_CASE("運転状態は名前で出る") {
    Telemetry t = sample();

    t.mode = RunMode::Safe;
    CHECK(line(t).find("mode=SAFE") != std::string::npos);

    t.mode = RunMode::Stop;
    CHECK(line(t).find("mode=STOP") != std::string::npos);
}

TEST_CASE("有効な軸はビットマスクで出る") {
    Telemetry t = sample();

    t.enabled_axes = 0;
    CHECK(line(t).find("en=0 ") != std::string::npos);

    t.enabled_axes = domain::axis_bit::THETA;
    CHECK(line(t).find("en=2 ") != std::string::npos);
}

TEST_CASE("エラービットは 16 進 2 桁") {
    Telemetry t = sample();
    t.error_bits = 0x0A;

    CHECK(line(t).find("err=0A") != std::string::npos);
}

TEST_CASE("行に改行は含まない") {
    const std::string text = line(sample());
    CHECK(text.find('\n') == std::string::npos);
    CHECK(text.find('\r') == std::string::npos);
}

TEST_CASE("最大構成でも行の容量に収まる") {
    Telemetry t;
    t.uptime_ms = 4294967295u;
    t.r_target_mm = -999999.0f;
    t.r_measured_mm = -999999.0f;
    t.theta_target_deg = -999999.0f;
    t.theta_measured_deg = -999999.0f;
    t.z_target_mm = -999999.0f;
    t.z_measured_mm = -999999.0f;
    t.enabled_axes = domain::axis_bit::ALL;
    t.mode = RunMode::Stop;
    t.error_bits = 0xFF;

    char buffer[domain::TELEMETRY_LINE_CAPACITY] = {};
    const std::size_t length = domain::formatTelemetry(t, buffer, sizeof(buffer));
    CHECK(length > 0);
    CHECK(length < domain::TELEMETRY_LINE_CAPACITY);
}

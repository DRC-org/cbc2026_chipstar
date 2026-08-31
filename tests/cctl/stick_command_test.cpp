#include "doctest.h"

#include "domain/stick_command.hpp"

#include <cstring>

using domain::ManualDelta;
using domain::StickCommand;

namespace {
bool parse(const char* line, StickCommand& out) {
    return domain::parseStickCommand(line, std::strlen(line), out);
}
}  // namespace

TEST_CASE("host の行から 2 軸を取り出す") {
    StickCommand cmd = {};
    REQUIRE(parse("LX+050 LY-030", cmd));

    CHECK(cmd.lx_percent == 50);
    CHECK(cmd.ly_percent == -30);
}

TEST_CASE("後続のフィールドがあっても先頭 2 軸だけ読む") {
    StickCommand cmd = {};
    REQUIRE(parse("LX+100 LY-100 RX+000 RY+000", cmd));

    CHECK(cmd.lx_percent == 100);
    CHECK(cmd.ly_percent == -100);
}

TEST_CASE("ゼロは符号付きで表現される") {
    StickCommand cmd = {};
    REQUIRE(parse("LX+000 LY+000", cmd));

    CHECK(cmd.lx_percent == 0);
    CHECK(cmd.ly_percent == 0);
}

TEST_CASE("形式が違う行は取り込まない") {
    StickCommand cmd = {};

    SUBCASE("短すぎる") {
        CHECK_FALSE(parse("LX+050 LY-0", cmd));
    }
    SUBCASE("ラベルが違う") {
        CHECK_FALSE(parse("RX+050 LY-030", cmd));
        CHECK_FALSE(parse("LX+050 RY-030", cmd));
    }
    SUBCASE("区切りが空白でない") {
        CHECK_FALSE(parse("LX+050,LY-030", cmd));
    }
    SUBCASE("符号がない") {
        CHECK_FALSE(parse("LX 050 LY-030", cmd));
    }
    SUBCASE("数字でない") {
        CHECK_FALSE(parse("LX+0a0 LY-030", cmd));
    }
    SUBCASE("範囲外") {
        CHECK_FALSE(parse("LX+101 LY+000", cmd));
        CHECK_FALSE(parse("LX+000 LY-101", cmd));
    }
}

TEST_CASE("不正な行を読んでも出力は書き換えない") {
    // 途中まで解釈できた値が漏れると、直前の指令が壊れる。
    StickCommand cmd = {12, 34};
    CHECK_FALSE(parse("LX+050 RY-030", cmd));

    CHECK(cmd.lx_percent == 12);
    CHECK(cmd.ly_percent == 34);
}

TEST_CASE("デッドゾーン内は 0 になる") {
    CHECK(domain::applyDeadzone(0.05f, 0.1f) == doctest::Approx(0.0f));
    CHECK(domain::applyDeadzone(-0.1f, 0.1f) == doctest::Approx(0.0f));
}

TEST_CASE("デッドゾーンの外は 0..1 へ引き伸ばされる") {
    // 不感帯のすぐ外で出力が飛ばないよう、残り区間を再スケールする。
    CHECK(domain::applyDeadzone(0.55f, 0.1f) == doctest::Approx(0.5f));
    CHECK(domain::applyDeadzone(1.0f, 0.1f) == doctest::Approx(1.0f));
    CHECK(domain::applyDeadzone(-1.0f, 0.1f) == doctest::Approx(-1.0f));
}

TEST_CASE("移動量は速度と経過時間の積") {
    const StickCommand cmd = {100, -100};
    const ManualDelta delta = domain::computeManualDelta(cmd, 0.1f, 0.0f, 100.0f, 90.0f);

    // ly が r、lx が θ に対応する。
    CHECK(delta.r_mm == doctest::Approx(-10.0f));
    CHECK(delta.theta_deg == doctest::Approx(9.0f));
}

TEST_CASE("デッドゾーン内の入力では動かない") {
    const StickCommand cmd = {5, -5};
    const ManualDelta delta = domain::computeManualDelta(cmd, 0.1f, 0.1f, 100.0f, 90.0f);

    CHECK(delta.r_mm == doctest::Approx(0.0f));
    CHECK(delta.theta_deg == doctest::Approx(0.0f));
}

TEST_CASE("経過時間が 0 なら動かない") {
    const StickCommand cmd = {100, 100};
    const ManualDelta delta = domain::computeManualDelta(cmd, 0.0f, 0.0f, 100.0f, 90.0f);

    CHECK(delta.r_mm == doctest::Approx(0.0f));
    CHECK(delta.theta_deg == doctest::Approx(0.0f));
}

#include "doctest.h"

#include "domain/command.hpp"

#include <cstring>

using domain::Command;
using domain::CommandKind;

namespace {
Command parse(const char* line) {
    return domain::parseCommand(line, std::strlen(line));
}
}  // namespace

TEST_CASE("引数のない指令") {
    CHECK(parse("STOP").kind == CommandKind::Stop);
    CHECK(parse("RUN").kind == CommandKind::Run);
    CHECK(parse("SAFE").kind == CommandKind::Safe);
    CHECK(parse("HOME").kind == CommandKind::Home);
}

TEST_CASE("大文字小文字は問わない") {
    // 現場でターミナルから直接打つため、綴りだけ合っていれば通す。
    CHECK(parse("stop").kind == CommandKind::Stop);
    CHECK(parse("Stop").kind == CommandKind::Stop);
    CHECK(parse("home").kind == CommandKind::Home);
}

TEST_CASE("前後の空白は無視する") {
    CHECK(parse("  STOP  ").kind == CommandKind::Stop);
    CHECK(parse("\tRUN").kind == CommandKind::Run);
}

TEST_CASE("TEST は 0 / 1 を取る") {
    const Command on = parse("TEST 1");
    CHECK(on.kind == CommandKind::Test);
    CHECK(on.value);

    const Command off = parse("TEST 0");
    CHECK(off.kind == CommandKind::Test);
    CHECK_FALSE(off.value);
}

TEST_CASE("EN は対象軸と 0 / 1 を取る") {
    const Command all = parse("EN RTZ 1");
    CHECK(all.kind == CommandKind::Enable);
    CHECK(all.axes == domain::axis_bit::ALL);
    CHECK(all.value);

    const Command theta = parse("EN T 0");
    CHECK(theta.kind == CommandKind::Enable);
    CHECK(theta.axes == domain::axis_bit::THETA);
    CHECK_FALSE(theta.value);

    const Command z = parse("EN Z 1");
    CHECK(z.axes == domain::axis_bit::Z);

    const Command rz = parse("EN RZ 1");
    CHECK(rz.axes == (domain::axis_bit::R | domain::axis_bit::Z));
}

TEST_CASE("軸指定も大文字小文字を問わない") {
    CHECK(parse("en rtz 1").axes == domain::axis_bit::ALL);
}

TEST_CASE("解釈できない行は None") {
    CHECK(parse("").kind == CommandKind::None);
    CHECK(parse("   ").kind == CommandKind::None);
    CHECK(parse("GO").kind == CommandKind::None);
    CHECK(parse("STOPPED").kind == CommandKind::None);
}

TEST_CASE("スティック入力の行は指令として扱わない") {
    // 同じリンクを流れるので、取り違えると操作が指令になる。
    CHECK(parse("LX+000 LY+000 RX+000 RY+000").kind == CommandKind::None);
}

TEST_CASE("引数が欠けている指令は受け付けない") {
    CHECK(parse("TEST").kind == CommandKind::None);
    CHECK(parse("EN").kind == CommandKind::None);
    CHECK(parse("EN RTZ").kind == CommandKind::None);
}

TEST_CASE("引数が不正な指令は受け付けない") {
    CHECK(parse("TEST 2").kind == CommandKind::None);
    CHECK(parse("TEST x").kind == CommandKind::None);
    CHECK(parse("EN X 1").kind == CommandKind::None);
    CHECK(parse("EN RTZ 2").kind == CommandKind::None);
    CHECK(parse("EN RR 1").kind == CommandKind::None);
}

TEST_CASE("余分な引数がある指令は受け付けない") {
    // 打ち間違いを黙って実行しない。
    CHECK(parse("STOP NOW").kind == CommandKind::None);
    CHECK(parse("TEST 1 2").kind == CommandKind::None);
}

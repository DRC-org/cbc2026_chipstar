#include "doctest.h"

#include "domain/servo_command.hpp"

#include <cstring>

namespace {
domain::ServoCommand parse(const char* line) {
    return domain::parseServoCommand(line, std::strlen(line));
}
}  // namespace

TEST_CASE("共通状態指令を解釈する") {
    CHECK(parse("HELLO 1").kind == domain::ServoCommandKind::Hello);
    CHECK(parse("SAFE").kind == domain::ServoCommandKind::Safe);
    CHECK(parse("RUN").kind == domain::ServoCommandKind::Run);
    CHECK(parse("STOP").kind == domain::ServoCommandKind::Stop);
    CHECK(parse("HEARTBEAT").kind == domain::ServoCommandKind::Heartbeat);
}

TEST_CASE("任意IDのサーボ指令を解釈する") {
    const auto enable = parse("SERVO ENABLE 253 1");
    CHECK(enable.kind == domain::ServoCommandKind::Enable);
    CHECK(enable.id == 253);
    CHECK(enable.enabled);

    const auto target = parse("SERVO TARGET 12 2048 500 50");
    CHECK(target.kind == domain::ServoCommandKind::Target);
    CHECK(target.id == 12);
    CHECK(target.position == 2048);
    CHECK(target.speed == 500);
    CHECK(target.acceleration == 50);

    CHECK(parse("SERVO READ 9").kind == domain::ServoCommandKind::Read);
}

TEST_CASE("範囲外と余分な引数を拒否する") {
    CHECK(parse("SERVO READ 0").kind == domain::ServoCommandKind::None);
    CHECK(parse("SERVO READ 254").kind == domain::ServoCommandKind::None);
    CHECK(parse("SERVO TARGET 1 4096 1 1").kind == domain::ServoCommandKind::None);
    CHECK(parse("SERVO TARGET 1 1 1001 1").kind == domain::ServoCommandKind::None);
    CHECK(parse("SERVO TARGET 1 1 1 255").kind == domain::ServoCommandKind::None);
    CHECK(parse("SERVO ENABLE 1 9").kind == domain::ServoCommandKind::None);
    CHECK(parse("STOP NOW").kind == domain::ServoCommandKind::None);
}

TEST_CASE("接点入力の読取りを解釈する") {
    CHECK(parse("INPUT READ").kind == domain::ServoCommandKind::InputRead);
    CHECK(parse("INPUT READ 1").kind == domain::ServoCommandKind::None);
    CHECK(parse("INPUT GUARD 1 0").kind == domain::ServoCommandKind::None);
}

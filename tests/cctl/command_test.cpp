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

TEST_CASE("共通状態指令を解釈する") {
    CHECK(parse("STOP").kind == CommandKind::Stop);
    CHECK(parse("RUN").kind == CommandKind::Run);
    CHECK(parse("SAFE").kind == CommandKind::Safe);
    CHECK(parse("HEARTBEAT").kind == CommandKind::Heartbeat);
}

TEST_CASE("HELLOはプロトコルバージョンを持つ") {
    const Command command = parse("HELLO 1");
    CHECK(command.kind == CommandKind::Hello);
    CHECK(command.protocol_version == 1);
}

TEST_CASE("slotの有効状態を解釈する") {
    const Command command = parse("ENABLE 5 1");
    CHECK(command.kind == CommandKind::Enable);
    CHECK(command.mask == 5);
    CHECK(command.value);
    CHECK(parse("ENABLE 7 0").kind == CommandKind::Enable);
}

TEST_CASE("HOMEはslot maskを要求する") {
    const Command command = parse("HOME 6");
    CHECK(command.kind == CommandKind::Home);
    CHECK(command.mask == 6);
    CHECK(parse("HOME").kind == CommandKind::None);
}

TEST_CASE("TARGETはslotと有限な実数を持つ") {
    const Command command = parse("TARGET 1 -123.5");
    CHECK(command.kind == CommandKind::Target);
    CHECK(command.slot == 1);
    CHECK(command.target == doctest::Approx(-123.5f));
    CHECK(parse("TARGET 3 0").kind == CommandKind::None);
    CHECK(parse("TARGET 0 nan").kind == CommandKind::None);
    CHECK(parse("TARGET 0 inf").kind == CommandKind::None);
}

TEST_CASE("CANはFDCAN2の標準IDと0から8 byteを解釈する") {
    const Command command = parse("CAN 2 768 0101000005DC0000");
    CHECK(command.kind == CommandKind::CanTx);
    CHECK(command.can_id == 0x300);
    CHECK(command.can_length == 8);
    CHECK(command.can_data[0] == 0x01);
    CHECK(command.can_data[1] == 0x01);
    CHECK(command.can_data[4] == 0x05);
    CHECK(command.can_data[5] == 0xDC);

    const Command empty = parse("CAN 2 0 -");
    CHECK(empty.kind == CommandKind::CanTx);
    CHECK(empty.can_length == 0);
}

TEST_CASE("CANは未提供bus、不正ID、不正payloadを拒否する") {
    CHECK(parse("CAN 1 768 00").kind == CommandKind::None);
    CHECK(parse("CAN 2 2048 00").kind == CommandKind::None);
    CHECK(parse("CAN 2 768 0").kind == CommandKind::None);
    CHECK(parse("CAN 2 768 GG").kind == CommandKind::None);
    CHECK(parse("CAN 2 768 000102030405060708").kind == CommandKind::None);
}

TEST_CASE("大文字小文字と前後空白を許容する") {
    CHECK(parse("  heartbeat  ").kind == CommandKind::Heartbeat);
    CHECK(parse("target 2 1.25").kind == CommandKind::Target);
}

TEST_CASE("範囲外または余分な引数を拒否する") {
    CHECK(parse("HELLO 0").kind == CommandKind::None);
    CHECK(parse("ENABLE 0 1").kind == CommandKind::None);
    CHECK(parse("ENABLE 8 1").kind == CommandKind::None);
    CHECK(parse("ENABLE 1 2").kind == CommandKind::None);
    CHECK(parse("STOP NOW").kind == CommandKind::None);
    CHECK(parse("TARGET 0 1 extra").kind == CommandKind::None);
}

TEST_CASE("接点入力の読取りと停止条件を解釈する") {
    CHECK(parse("INPUT READ").kind == CommandKind::InputRead);

    const Command command = parse("INPUT GUARD 5 4");
    CHECK(command.kind == CommandKind::InputGuard);
    CHECK(command.mask == 5);
    CHECK(command.input_high == 4);

    const Command cleared = parse("INPUT GUARD 0 0");
    CHECK(cleared.kind == CommandKind::InputGuard);
    CHECK(cleared.mask == 0);
}

TEST_CASE("未実装の接点と監視外の極性指定を拒否する") {
    CHECK(parse("INPUT GUARD 8 0").kind == CommandKind::None);
    CHECK(parse("INPUT GUARD 1 2").kind == CommandKind::None);
    CHECK(parse("INPUT GUARD 1").kind == CommandKind::None);
    CHECK(parse("INPUT READ 1").kind == CommandKind::None);
}

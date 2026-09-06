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


TEST_CASE("パラメータの設定と読み出しを解釈する") {
    const Command set = parse("PARAM 4 0.75");
    CHECK(set.kind == CommandKind::ParamSet);
    CHECK(set.param_id == 4);
    CHECK(set.target == doctest::Approx(0.75f));

    const Command get = parse("PARAM 4");
    CHECK(get.kind == CommandKind::ParamGet);
    CHECK(get.param_id == 4);

    CHECK(parse("param 30 -12.5").kind == CommandKind::ParamSet);
}

TEST_CASE("パラメータの不正な指定を拒否する") {
    CHECK(parse("PARAM").kind == CommandKind::None);
    CHECK(parse("PARAM 256 1").kind == CommandKind::None);
    CHECK(parse("PARAM 0 nan").kind == CommandKind::None);
    CHECK(parse("PARAM 0 1 2").kind == CommandKind::None);
}

TEST_CASE("DMドライバのレジスタ指令を解釈する") {
    const Command read = parse("DMREG 10");
    CHECK(read.kind == CommandKind::DmRegRead);
    CHECK(read.param_id == 10);

    // CTRL_MODE(0x0A) へ 2（位置速度モード）を書く。
    const Command write = parse("DMREG 10 00000002");
    CHECK(write.kind == CommandKind::DmRegWrite);
    CHECK(write.param_id == 10);
    CHECK(write.raw_value == 2);

    // PMAX(0x15) へ 12.5f を書く。
    const Command as_float = parse("DMREG 21 41480000");
    CHECK(as_float.kind == CommandKind::DmRegWrite);
    CHECK(as_float.raw_value == 0x41480000);
}

TEST_CASE("DMレジスタの不正な指定を拒否する") {
    CHECK(parse("DMREG").kind == CommandKind::None);
    CHECK(parse("DMREG 256").kind == CommandKind::None);
    CHECK(parse("DMREG 10 2").kind == CommandKind::None);        // 8桁必須
    CHECK(parse("DMREG 10 0000000G").kind == CommandKind::None);
    CHECK(parse("DMREG 10 00000002 x").kind == CommandKind::None);
}

TEST_CASE("CANの診断指令を解釈する") {
    CHECK(parse("CANSTAT").kind == CommandKind::CanStat);
    CHECK(parse("canstat").kind == CommandKind::CanStat);
    CHECK(parse("CANSTAT 1").kind == CommandKind::None);
}

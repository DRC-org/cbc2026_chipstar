#include "doctest.h"

#include "domain/servo_can_protocol.hpp"

using domain::servo_can::Command;
using domain::servo_can::CommandKind;
using domain::servo_can::ParamId;
using domain::servo_can::Parameters;

namespace {
const Parameters DEFAULTS;
}  // namespace

TEST_CASE("停止とheartbeatを解釈する") {
    Command command;
    const uint8_t stop[8] = {1, 0, 0, 0, 0, 0, 0, 0};
    CHECK(domain::servo_can::parse(stop, sizeof(stop), command, DEFAULTS));
    CHECK(command.kind == CommandKind::Stop);

    const uint8_t heartbeat[8] = {1, 3, 0, 0, 0, 0, 0, 0};
    CHECK(domain::servo_can::parse(heartbeat, sizeof(heartbeat), command, DEFAULTS));
    CHECK(command.kind == CommandKind::Heartbeat);
}

TEST_CASE("チャネルごとの有効化とパルス幅を解釈する") {
    Command command;
    const uint8_t enable[8] = {1, 2, 3, 1, 0, 0, 0, 0};
    CHECK(domain::servo_can::parse(enable, sizeof(enable), command, DEFAULTS));
    CHECK(command.kind == CommandKind::Enable);
    CHECK(command.channel == 3);
    CHECK(command.enabled);

    const uint8_t set[8] = {1, 1, 2, 0, 0x05, 0xDC, 0, 0};
    CHECK(domain::servo_can::parse(set, sizeof(set), command, DEFAULTS));
    CHECK(command.kind == CommandKind::Set);
    CHECK(command.channel == 2);
    CHECK(command.pulse_us == 1500);
}

TEST_CASE("不正なversion、長さ、範囲、予約bitを拒否する") {
    Command command;
    uint8_t data[8] = {1, 1, 0, 0, 0x05, 0xDC, 0, 0};
    CHECK_FALSE(domain::servo_can::parse(data, 7, command, DEFAULTS));
    data[0] = 2;
    CHECK_FALSE(domain::servo_can::parse(data, sizeof(data), command, DEFAULTS));
    data[0] = 1;
    data[2] = 4;
    CHECK_FALSE(domain::servo_can::parse(data, sizeof(data), command, DEFAULTS));
    data[2] = 0;
    data[4] = 0;
    data[5] = 100;
    CHECK_FALSE(domain::servo_can::parse(data, sizeof(data), command, DEFAULTS));
    data[4] = 0x05;
    data[5] = 0xDC;
    data[7] = 1;
    CHECK_FALSE(domain::servo_can::parse(data, sizeof(data), command, DEFAULTS));
}


TEST_CASE("実行時パラメータでパルス幅の範囲を変えられる") {
    Parameters parameters;
    Command command;
    // 既定の範囲外。
    const uint8_t narrow[8] = {1, 1, 0, 0, 0x01, 0x2C, 0, 0};  // 300us
    CHECK_FALSE(domain::servo_can::parse(narrow, sizeof(narrow), command, parameters));

    // 下限を200usへ広げると通る。
    const uint8_t set[8] = {1, 4, static_cast<uint8_t>(ParamId::MinPulseUs), 0,
                            0x43, 0x48, 0x00, 0x00};  // 200.0f
    CHECK(domain::servo_can::parse(set, sizeof(set), command, parameters));
    CHECK(command.kind == CommandKind::ParamSet);
    CHECK(parameters.set(command.param_id, command.value));
    CHECK(parameters.minPulseUs() == 200);
    CHECK(domain::servo_can::parse(narrow, sizeof(narrow), command, parameters));
    CHECK(command.pulse_us == 300);
}

TEST_CASE("FWが壊れるパラメータと未定義idを拒否する") {
    Command command;
    const uint8_t zero[8] = {1, 4, static_cast<uint8_t>(ParamId::WatchdogMs), 0, 0, 0, 0, 0};
    CHECK_FALSE(domain::servo_can::parse(zero, sizeof(zero), command, DEFAULTS));
    const uint8_t unknown[8] = {1, 4, domain::servo_can::PARAM_COUNT, 0, 0x43, 0x48, 0x00, 0x00};
    CHECK_FALSE(domain::servo_can::parse(unknown, sizeof(unknown), command, DEFAULTS));
    const uint8_t reserved[8] = {1, 4, 0, 1, 0x43, 0x48, 0x00, 0x00};
    CHECK_FALSE(domain::servo_can::parse(reserved, sizeof(reserved), command, DEFAULTS));
}

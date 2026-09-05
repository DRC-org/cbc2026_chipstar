#include "doctest.h"

#include "domain/servo_can_protocol.hpp"

using domain::servo_can::Command;
using domain::servo_can::CommandKind;

TEST_CASE("停止とheartbeatを解釈する") {
    Command command;
    const uint8_t stop[8] = {1, 0, 0, 0, 0, 0, 0, 0};
    CHECK(domain::servo_can::parse(stop, sizeof(stop), command));
    CHECK(command.kind == CommandKind::Stop);

    const uint8_t heartbeat[8] = {1, 3, 0, 0, 0, 0, 0, 0};
    CHECK(domain::servo_can::parse(heartbeat, sizeof(heartbeat), command));
    CHECK(command.kind == CommandKind::Heartbeat);
}

TEST_CASE("チャネルごとの有効化とパルス幅を解釈する") {
    Command command;
    const uint8_t enable[8] = {1, 2, 3, 1, 0, 0, 0, 0};
    CHECK(domain::servo_can::parse(enable, sizeof(enable), command));
    CHECK(command.kind == CommandKind::Enable);
    CHECK(command.channel == 3);
    CHECK(command.enabled);

    const uint8_t set[8] = {1, 1, 2, 0, 0x05, 0xDC, 0, 0};
    CHECK(domain::servo_can::parse(set, sizeof(set), command));
    CHECK(command.kind == CommandKind::Set);
    CHECK(command.channel == 2);
    CHECK(command.pulse_us == 1500);
}

TEST_CASE("不正なversion、長さ、範囲、予約bitを拒否する") {
    Command command;
    uint8_t data[8] = {1, 1, 0, 0, 0x05, 0xDC, 0, 0};
    CHECK_FALSE(domain::servo_can::parse(data, 7, command));
    data[0] = 2;
    CHECK_FALSE(domain::servo_can::parse(data, sizeof(data), command));
    data[0] = 1;
    data[2] = 4;
    CHECK_FALSE(domain::servo_can::parse(data, sizeof(data), command));
    data[2] = 0;
    data[4] = 0;
    data[5] = 100;
    CHECK_FALSE(domain::servo_can::parse(data, sizeof(data), command));
    data[4] = 0x05;
    data[5] = 0xDC;
    data[7] = 1;
    CHECK_FALSE(domain::servo_can::parse(data, sizeof(data), command));
}

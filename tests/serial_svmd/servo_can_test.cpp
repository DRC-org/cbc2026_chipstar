#include "doctest.h"

#include "domain/parameters.hpp"
#include "domain/servo_can.hpp"

#include <utility>

using domain::ServoCommandKind;
using namespace domain::servo_can;

namespace {
domain::ServoCommand parse_frame(const uint8_t (&data)[8]) {
    domain::ServoCommand command;
    parse(data, sizeof(data), command);
    return command;
}
}  // namespace

TEST_CASE("引数を取らない指令を解釈する") {
    domain::ServoCommand command;
    for (auto [op, kind] : {
             std::pair{Op::Hello, ServoCommandKind::Hello},
             std::pair{Op::Safe, ServoCommandKind::Safe},
             std::pair{Op::Run, ServoCommandKind::Run},
             std::pair{Op::Stop, ServoCommandKind::Stop},
             std::pair{Op::Heartbeat, ServoCommandKind::Heartbeat},
             std::pair{Op::InputRead, ServoCommandKind::InputRead},
         }) {
        const uint8_t data[8] = {1, static_cast<uint8_t>(op), 0, 0, 0, 0, 0, 0};
        CHECK(parse(data, sizeof(data), command));
        CHECK(command.kind == kind);
    }
}

TEST_CASE("位置指令はID・加速度・位置・速度を運ぶ") {
    const uint8_t data[8] = {1, 4, 12, 30, 0x08, 0x00, 0x01, 0xF4};
    const auto command = parse_frame(data);
    CHECK(command.kind == ServoCommandKind::Target);
    CHECK(command.id == 12);
    CHECK(command.acceleration == 30);
    CHECK(command.position == 2048);
    CHECK(command.speed == 500);
}

TEST_CASE("通常位置指令は符号付き多回転位置を運ぶ") {
    const uint8_t positive[8] = {1, 4, 12, 50, 0x18, 0x00, 0x00, 0x00};
    const auto plus = parse_frame(positive);
    CHECK(plus.kind == ServoCommandKind::Target);
    CHECK(plus.position == 6144);

    const uint8_t negative[8] = {1, 4, 12, 50, 0x98, 0x00, 0x00, 0x00};
    const auto minus = parse_frame(negative);
    CHECK(minus.kind == ServoCommandKind::Target);
    CHECK(minus.position == 0x9800);
}

TEST_CASE("トルク切替と位置取得を解釈する") {
    const uint8_t enable[8] = {1, 6, 253, 1, 0, 0, 0, 0};
    const auto on = parse_frame(enable);
    CHECK(on.kind == ServoCommandKind::Enable);
    CHECK(on.id == 253);
    CHECK(on.enabled);

    const uint8_t read[8] = {1, 7, 9, 0, 0, 0, 0, 0};
    const auto position = parse_frame(read);
    CHECK(position.kind == ServoCommandKind::Read);
    CHECK(position.id == 9);
}

TEST_CASE("不正なversion、長さ、op、範囲を拒否する") {
    domain::ServoCommand command;
    uint8_t data[8] = {1, 4, 12, 30, 0x08, 0x00, 0x01, 0xF4};
    CHECK_FALSE(parse(data, 7, command));
    CHECK_FALSE(parse(nullptr, 8, command));
    data[0] = 2;
    CHECK_FALSE(parse(data, 8, command));
    data[0] = 1;
    data[1] = 9;  // 未定義のop
    CHECK_FALSE(parse(data, 8, command));
    data[1] = 4;
    data[2] = 0;  // IDは1..253
    CHECK_FALSE(parse(data, 8, command));
    data[2] = 254;
    CHECK_FALSE(parse(data, 8, command));
    data[2] = 12;
    data[4] = 0x70;  // 位置28673
    data[5] = 0x01;
    CHECK_FALSE(parse(data, 8, command));
    data[4] = 0x08;
    data[6] = 0x03;  // 速度1001
    data[7] = 0xE9;
    CHECK_FALSE(parse(data, 8, command));
    data[6] = 0x01;
    data[7] = 0xF4;
    data[3] = 255;  // 加速度255
    CHECK_FALSE(parse(data, 8, command));
}

TEST_CASE("引数を取らない指令に値が付いていたら拒否する") {
    domain::ServoCommand command;
    for (uint8_t index = 2; index < 8; ++index) {
        uint8_t data[8] = {1, 2, 0, 0, 0, 0, 0, 0};
        data[index] = 1;
        CHECK_FALSE(parse(data, sizeof(data), command));
    }
}

TEST_CASE("応答を8 byteへ符号化する") {
    uint8_t frame[8] = {};
    encodeStatus(Status::Timeout, 2, 3, frame);
    CHECK(frame[0] == 1);
    CHECK(frame[1] == 2);
    CHECK(frame[2] == 2);
    CHECK(frame[3] == 3);

    encodePosition(12, 2048, true, 0x04, frame);
    CHECK(frame[1] == 12);
    CHECK(frame[2] == 0x08);
    CHECK(frame[3] == 0x00);
    CHECK(frame[4] == 1);
    CHECK(frame[5] == 0x04);

    encodeInputs(3, 1, 5, 63, frame);
    CHECK(frame[1] == 3);
    CHECK(frame[2] == 1);
    CHECK(frame[3] == 5);
    CHECK(frame[4] == 63);
}

TEST_CASE("実行時パラメータの設定を解釈する") {
    domain::ServoCommand command;
    // サーボバスを1 Mbpsへ（1000000.0f = 0x49742400）
    const uint8_t baud[8] = {1, 9, static_cast<uint8_t>(domain::ServoParamId::ServoBaud), 0,
                             0x49, 0x74, 0x24, 0x00};
    CHECK(parse(baud, sizeof(baud), command));
    CHECK(command.kind == ServoCommandKind::ParamSet);
    CHECK(command.param_id == 0);
    CHECK(command.value == doctest::Approx(1000000.0f));

    domain::ServoParameters parameters;
    CHECK(parameters.baud() == 1000000);
    CHECK(parameters.set(command.param_id, command.value));
    CHECK(parameters.baud() == 1000000);
}

TEST_CASE("FWが壊れるパラメータと未定義idを拒否する") {
    domain::ServoCommand command;
    // ボーレート0は通信が成立しない。
    const uint8_t zero[8] = {1, 9, 0, 0, 0, 0, 0, 0};
    CHECK_FALSE(parse(zero, sizeof(zero), command));
    const uint8_t unknown[8] = {1, 9, domain::SERVO_PARAM_COUNT, 0, 0x47, 0x00, 0x00, 0x00};
    CHECK_FALSE(parse(unknown, sizeof(unknown), command));
    // byte 3 は予約。
    const uint8_t reserved[8] = {1, 9, 0, 1, 0x49, 0x74, 0x24, 0x00};
    CHECK_FALSE(parse(reserved, sizeof(reserved), command));
    // 未定義のop。
    const uint8_t bad_op[8] = {1, 10, 0, 0, 0, 0, 0, 0};
    CHECK_FALSE(parse(bad_op, sizeof(bad_op), command));
}

TEST_CASE("基板アドレスでCAN IDをずらす") {
    CHECK(canId(COMMAND_ID, 0) == 0x320);
    CHECK(canId(COMMAND_ID, 1) == 0x420);
    CHECK(canId(INPUT_ID, 3) == 0x623);
    CHECK(canId(INPUT_ID, MAX_ADDRESS) <= 0x7FF);
}

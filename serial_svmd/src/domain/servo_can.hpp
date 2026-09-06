#pragma once

#include "domain/servo_command.hpp"

#include <cstddef>
#include <cstdint>

namespace domain::servo_can {

// cctlのFDCAN2から届く指令。USART2のASCIIと同じ状態機械へ入れるため、
// 解析結果は ServoCommand に揃える。
constexpr uint16_t COMMAND_ID = 0x320;
constexpr uint16_t STATUS_ID = 0x321;
constexpr uint16_t POSITION_ID = 0x322;
constexpr uint16_t INPUT_ID = 0x323;
constexpr uint8_t PROTOCOL_VERSION = 1;

enum class Op : uint8_t {
    Hello = 0,
    Safe = 1,
    Run = 2,
    Stop = 3,
    Target = 4,
    Heartbeat = 5,
    Enable = 6,
    Read = 7,
    InputRead = 8,
    ParamSet = 9,
};

enum class Status : uint8_t { Ok = 0, Rejected = 1, Timeout = 2 };

// 8 byte固定。
//   0    version = 1
//   1    op
//   2    サーボID（IDを取らないopは0）
//   3    ENABLEの0/1、TARGETの加速度、それ以外は0
//   4..5 TARGETの位置、big endian。それ以外は0
//   6..7 TARGETの速度、big endian。それ以外は0
// PARAM SET だけは byte 2 をパラメータid、byte 4..7 を float32 として使う。
bool parse(const uint8_t* data, std::size_t length, ServoCommand& out);

// 指令の受理結果と現在のモード。指令受信時と定期送信で返す。
void encodeStatus(Status status, uint8_t mode, uint8_t servo_count, uint8_t* out);

// READ の応答。
void encodePosition(uint8_t id, uint16_t position, bool enabled, uint8_t error, uint8_t* out);

// 接点とDIPの状態。
void encodeInputs(uint8_t raw, uint8_t stable, uint8_t dip, uint8_t available, uint8_t* out);

}  // namespace domain::servo_can

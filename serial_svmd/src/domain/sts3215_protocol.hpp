#pragma once

#include <cstddef>
#include <cstdint>

// Feetech STS プロトコルのパケット組立・検証。
// HAL には依存しないので、ホスト側で単体テストできる。
namespace domain::sts3215 {

constexpr uint8_t HEADER = 0xFF;
constexpr uint8_t BROADCAST_ID = 0xFE;
constexpr uint16_t MAX_POSITION = 4095;
constexpr uint8_t BAUD_CODE_115200 = 4;

constexpr uint8_t INSTRUCTION_PING = 0x01;
constexpr uint8_t INSTRUCTION_READ = 0x02;
constexpr uint8_t INSTRUCTION_WRITE = 0x03;
constexpr uint8_t INSTRUCTION_SYNC_WRITE = 0x83;

constexpr uint8_t MAX_TX_PARAMETERS = 132;
constexpr uint8_t MAX_RX_PARAMETERS = 64;
// 加速度・目標位置・目標時間・目標速度の合計バイト数。
constexpr uint8_t TARGET_DATA_LENGTH = 7;
constexpr std::size_t MAX_SYNC_TARGETS = 16;

// FF FF <ID> <長さ> <命令> <パラメータ...> <チェックサム>
constexpr std::size_t PACKET_OVERHEAD = 6;
constexpr std::size_t MAX_PACKET_SIZE = MAX_TX_PARAMETERS + PACKET_OVERHEAD;
constexpr std::size_t MAX_SYNC_PARAMETERS =
    2 + (MAX_SYNC_TARGETS * (TARGET_DATA_LENGTH + 1));

// レジスタアドレス。
namespace reg {
constexpr uint8_t ID = 5;
constexpr uint8_t BAUD_RATE = 6;
constexpr uint8_t TORQUE_ENABLE = 40;
constexpr uint8_t ACCELERATION = 41;
constexpr uint8_t GOAL_POSITION = 42;
constexpr uint8_t GOAL_TIME = 44;
constexpr uint8_t GOAL_SPEED = 46;
constexpr uint8_t PRESENT_POSITION = 56;
}  // namespace reg

// 加速度・目標位置・目標時間・目標速度をまとめた指令値。
// アドレス41から連続する4レジスタに対応する。
struct Target {
    uint8_t id;
    uint8_t acceleration;
    uint16_t position;
    uint16_t time;
    uint16_t speed;
};

// ID からチェックサム直前までの総和のビット反転。
uint8_t checksum(const uint8_t* data, std::size_t length);

// 命令パケットを packet へ組み立て、その長さを返す。
// 引数が不正、または capacity が足りない場合は 0 を返す。
std::size_t buildInstruction(uint8_t* packet, std::size_t capacity, uint8_t id,
                             uint8_t instruction, const uint8_t* parameters,
                             uint8_t parameter_count);

// 受信したステータス本体 <エラービット> <パラメータ...> <チェックサム> を検証する。
// body_length は応答ヘッダが示す長さ（パラメータ数 + 2）。
bool verifyStatusChecksum(uint8_t response_id, uint8_t body_length, const uint8_t* body);

// 16 ビット値は STS 仕様どおりリトルエンディアン。
void encodeUint16(uint16_t value, uint8_t* data);
uint16_t decodeUint16(const uint8_t* data);

// 目標値を 7 バイトへ符号化する。
void encodeTarget(const Target& target, uint8_t* data);

}  // namespace domain::sts3215

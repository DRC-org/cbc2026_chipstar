#pragma once

#include <cstdint>

// Damiao DM-S3519-1EC（DM3520-1EC ドライバ）のフレーム符号化・復号。
// 出典: docs/datasheets/dm-s3519-1ec_user_manual_v1.1.pdf
namespace domain::dm {

// 制御モード（レジスタ CTRL_MODE）。モードごとに指令フレームの ID が変わる。
enum class ControlMode : uint32_t {
    Mit = 1,
    PositionVelocity = 2,
    Velocity = 3,
};

// 指令フレーム ID の基点。実際の ID は base + CAN_ID。
constexpr uint16_t MIT_CMD_BASE = 0x000;
constexpr uint16_t POS_VEL_CMD_BASE = 0x100;
constexpr uint16_t VELOCITY_CMD_BASE = 0x200;

// 設定フレーム
constexpr uint16_t CONFIG_ID = 0x7FF;
constexpr uint8_t CONFIG_READ = 0x33;
constexpr uint8_t CONFIG_WRITE = 0x55;
constexpr uint8_t CONFIG_STORE = 0xAA;

// 特殊コマンド（D0..D6 = 0xFF, D7 = コード）
constexpr uint8_t SPECIAL_ENABLE = 0xFC;
constexpr uint8_t SPECIAL_DISABLE = 0xFD;
constexpr uint8_t SPECIAL_ZERO = 0xFE;

// MIT モードのゲイン値域。
constexpr float KP_MIN = 0.0f;
constexpr float KP_MAX = 500.0f;
constexpr float KD_MIN = 0.0f;
constexpr float KD_MAX = 5.0f;

// レジスタアドレス。
namespace reg {
constexpr uint8_t UV_VALUE = 0x00;
constexpr uint8_t KT_VALUE = 0x01;
constexpr uint8_t OT_VALUE = 0x02;
constexpr uint8_t OC_VALUE = 0x03;
constexpr uint8_t ACC = 0x04;
constexpr uint8_t DEC = 0x05;
constexpr uint8_t MAX_SPD = 0x06;
constexpr uint8_t MST_ID = 0x07;
constexpr uint8_t ESC_ID = 0x08;
constexpr uint8_t TIMEOUT = 0x09;
constexpr uint8_t CTRL_MODE = 0x0A;
constexpr uint8_t GEAR_RATIO = 0x14;
constexpr uint8_t PMAX = 0x15;
constexpr uint8_t VMAX = 0x16;
constexpr uint8_t TMAX = 0x17;
constexpr uint8_t I_BW = 0x18;
constexpr uint8_t KP_ASR = 0x19;
constexpr uint8_t KI_ASR = 0x1A;
constexpr uint8_t KP_APR = 0x1B;
constexpr uint8_t KI_APR = 0x1C;
constexpr uint8_t OV_VALUE = 0x1D;
constexpr uint8_t GREF = 0x1E;
constexpr uint8_t BAUD = 0x23;
}  // namespace reg

// フィードバック D0 上位 4bit の状態。
namespace error_code {
constexpr uint8_t DISABLED = 0;
constexpr uint8_t ENABLED = 1;
constexpr uint8_t SENSOR = 5;
constexpr uint8_t PARAMETER = 6;
constexpr uint8_t OVERVOLTAGE = 8;
constexpr uint8_t UNDERVOLTAGE = 9;
constexpr uint8_t OVERCURRENT = 0x0A;
}  // namespace error_code

// 位置・速度・トルクの線形マッピング範囲。
// モータ側のレジスタ PMAX / VMAX / TMAX と一致していないと値がずれる。
struct Range {
    float p_max;
    float v_max;
    float t_max;
};

struct Feedback {
    uint8_t id = 0;
    uint8_t error = 0;
    float position_rad = 0.0f;
    float velocity_rad_s = 0.0f;
    float torque_nm = 0.0f;
    uint8_t mos_temperature_c = 0;    // ドライバ上側 MOS の平均温度
    uint8_t rotor_temperature_c = 0;  // モータ内部コイルの平均温度
};

// float と生値の線形マッピング。bits は 1..32。
uint32_t floatToRaw(float value, float min, float max, uint8_t bits);
float rawToFloat(uint32_t raw, float min, float max, uint8_t bits);

Feedback decodeFeedback(const uint8_t data[8], const Range& range);

// MIT 指令を 8 バイトへ詰める。
// 位置 16bit / 速度 12bit / Kp 12bit / Kd 12bit / トルク 12bit。
void encodeMit(float position_rad, float velocity_rad_s, float kp, float kd, float torque_nm,
               const Range& range, uint8_t out[8]);

// 位置速度指令（float 2 つ、リトルエンディアン）。
void encodePositionVelocity(float position_rad, float velocity_limit, uint8_t out[8]);

// 速度指令（float 1 つ）。
void encodeVelocity(float velocity_rad_s, uint8_t out[8]);

// 特殊コマンド（Enable / Disable / Zero）。
void encodeSpecial(uint8_t command, uint8_t out[8]);

// 設定フレーム（読み出し 0x33 / 書き込み 0x55 / 保存 0xAA）。
void encodeConfig(uint16_t can_id, uint8_t command, uint8_t rid, uint32_t value, uint8_t out[8]);

// 設定フレームへの応答か判定する。
// 応答はフィードバックと同じ MST_ID で返るため、取り違えると
// 位置や温度に設定値が化けて入る。
bool isConfigReply(uint16_t can_id, const uint8_t data[8]);

uint8_t configReplyRegister(const uint8_t data[8]);
uint32_t configReplyValue(const uint8_t data[8]);

}  // namespace domain::dm

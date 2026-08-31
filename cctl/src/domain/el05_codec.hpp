#pragma once

#include <cstdint>

// RobStride EL05 の拡張ID・パラメータ定義とフィードバック復号。
// 出典: docs/datasheets/robstride_el05_user_manual_260713.pdf
namespace domain::el05 {

// 通信種別（拡張ID bit28..24）。
namespace comm {
constexpr uint8_t GET_DEVICE_ID = 0;
constexpr uint8_t MOTION_CONTROL = 1;
constexpr uint8_t FEEDBACK = 2;
constexpr uint8_t ENABLE = 3;
constexpr uint8_t DISABLE = 4;
constexpr uint8_t SET_ZERO = 6;
constexpr uint8_t SET_CAN_ID = 7;
constexpr uint8_t READ_PARAM = 17;
constexpr uint8_t WRITE_PARAM = 18;
constexpr uint8_t FAULT_FEEDBACK = 21;
constexpr uint8_t SAVE_PARAM = 22;
}  // namespace comm

// 運転モード（パラメータ run_mode）。
enum class RunMode : uint8_t {
    Operation = 0,    // MIT 相当の運動制御
    Position = 1,     // 位置 (PP)
    Velocity = 2,     // 速度
    Current = 3,      // 電流
    PositionCsp = 5,  // 位置 (CSP)
};

// パラメータID。読み書きの可否はマニュアルの表に従う。
namespace param {
constexpr uint16_t RUN_MODE = 0x7005;      // uint8
constexpr uint16_t IQ_REF = 0x7006;        // 電流モードの目標電流 [-11, 11] A
constexpr uint16_t SPD_REF = 0x700A;       // 速度モードの目標速度
constexpr uint16_t LIMIT_TORQUE = 0x700B;  // トルク制限 [0, 6] Nm
constexpr uint16_t CUR_KP = 0x7010;
constexpr uint16_t CUR_KI = 0x7011;
constexpr uint16_t CUR_FILT_GAIN = 0x7014;
constexpr uint16_t LOC_REF = 0x7016;    // 位置モードの目標位置 [rad]
constexpr uint16_t LIMIT_SPD = 0x7017;  // 位置モードの速度制限
constexpr uint16_t LIMIT_CUR = 0x7018;  // 電流制限 [0, 11] A
constexpr uint16_t MECH_POS = 0x7019;   // 読出: 機械角 [rad]
constexpr uint16_t IQF = 0x701A;        // 読出: フィルタ後電流
constexpr uint16_t MECH_VEL = 0x701B;   // 読出: 負荷側速度
constexpr uint16_t VBUS = 0x701C;       // 読出: バス電圧 [V]
constexpr uint16_t LOC_KP = 0x701E;
constexpr uint16_t SPD_KP = 0x701F;
constexpr uint16_t SPD_KI = 0x7020;
constexpr uint16_t SPD_FILT_GAIN = 0x7021;
constexpr uint16_t ACC_RAD = 0x7022;  // 速度モードの加速度
constexpr uint16_t VEL_MAX = 0x7024;  // 位置モードの最大速度
constexpr uint16_t ACC_SET = 0x7025;  // 位置モードの加速度
constexpr uint16_t DCC_SET = 0x702E;  // 減速度
}  // namespace param

// フィードバックの線形マッピング範囲。
constexpr float POSITION_MIN = -12.57f;
constexpr float POSITION_MAX = 12.57f;
constexpr float VELOCITY_MIN = -50.0f;
constexpr float VELOCITY_MAX = 50.0f;
constexpr float TORQUE_MIN = -6.0f;
constexpr float TORQUE_MAX = 6.0f;

struct Feedback {
    uint8_t motor_id = 0;
    uint8_t fault_bits = 0;
    float position_rad = 0.0f;
    float velocity_rad_s = 0.0f;
    float torque_nm = 0.0f;
    float temperature_c = 0.0f;
};

// 拡張ID を組み立てる: [comm_type(5)][data_area_2(16)][target_id(8)]。
uint32_t buildCanId(uint8_t comm_type, uint16_t data_area_2, uint8_t target_id);

uint8_t commType(uint32_t ext_id);
uint8_t targetId(uint32_t ext_id);
uint16_t dataArea2(uint32_t ext_id);

float uint16ToFloat(uint16_t raw, float min, float max);
uint16_t floatToUint16(float value, float min, float max);

// フィードバックフレーム（comm_type = 2）を復号する。
Feedback decodeFeedback(uint32_t ext_id, const uint8_t data[8]);

// パラメータ書込のペイロード（Byte0-1 = index, Byte4-7 = 値）。
void encodeParamFloat(uint16_t param, float value, uint8_t out[8]);
void encodeParamU8(uint16_t param, uint8_t value, uint8_t out[8]);
// パラメータ読出の要求ペイロード。
void encodeParamRequest(uint16_t param, uint8_t out[8]);
// 保存フレームのペイロードは固定列。
void encodeSave(uint8_t out[8]);

uint16_t paramReplyIndex(const uint8_t data[8]);
float paramReplyFloat(const uint8_t data[8]);

}  // namespace domain::el05

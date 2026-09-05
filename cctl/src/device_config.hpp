#pragma once

#include <cstdint>

// cctl基板が安全に提供できるデバイス能力。機体の軸名や機械換算はhostに置く。
namespace config {
namespace can_id {
constexpr uint16_t C620_COMMAND = 0x200;
constexpr uint8_t C620_ESC_ID = 1;
constexpr uint16_t C620_FEEDBACK = 0x200 + C620_ESC_ID;
constexpr uint16_t DM_CAN_ID = 0x09;
constexpr uint16_t DM_MST_ID = 0x0A;
constexpr uint8_t EL05_MOTOR_ID = 0x7F;
constexpr uint8_t EL05_HOST_ID = 0xFD;
}  // namespace can_id

namespace limit {
constexpr float SLOT0_MIN = -12.5f;     // EL05出力位置 [rad]
constexpr float SLOT0_MAX = 12.5f;
constexpr float SLOT1_MIN = -26000.0f;  // M3508モータ多回転角 [deg]
constexpr float SLOT1_MAX = 26000.0f;
constexpr float SLOT2_MIN = -12.5f;     // DM出力位置 [rad]
constexpr float SLOT2_MAX = 12.5f;
}  // namespace limit

namespace dm {
constexpr float P_MAX = 12.5f;
constexpr float V_MAX = 45.0f;
constexpr float T_MAX = 18.0f;
constexpr float POS_VEL_LIMIT = 8.0f;
}  // namespace dm

namespace el05 {
constexpr float LIMIT_SPD = 5.0f;
constexpr float LIMIT_CUR = 8.0f;
constexpr float LOC_KP = 30.0f;
}  // namespace el05

namespace m3508 {
constexpr float POS_KP = 8.0f;
constexpr float POS_KI = 0.0f;
constexpr float POS_KD = 0.0f;
constexpr float MAX_RPM = 4000.0f;
constexpr float VEL_KP = 0.7f;
constexpr float VEL_KI = 0.0005f;
constexpr float VEL_KD = 50.0f;
constexpr float MAX_CURRENT_MA = 5000.0f;
}  // namespace m3508

namespace period {
constexpr uint32_t M3508_MS = 1;
constexpr uint32_t DM_MS = 10;
constexpr uint32_t EL05_MS = 20;
constexpr uint32_t LCD_MS = 200;
constexpr uint32_t TELEMETRY_MS = 50;
constexpr uint32_t WATCHDOG_MS = 250;
}  // namespace period
}  // namespace config

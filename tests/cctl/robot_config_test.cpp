#include "doctest.h"

#include "robot_config.hpp"

#include <cmath>

// robot_config.hpp は実測値が判明するたびに書き換える前提のファイルなので、
// 書き換えで壊れると影響が大きい不変条件をここで固定する。

TEST_CASE("ソフトリミットは下限 < 上限") {
    CHECK(config::mech::R_MIN_MM < config::mech::R_MAX_MM);
    CHECK(config::mech::Z_MIN_MM < config::mech::Z_MAX_MM);
    CHECK(config::mech::THETA_MIN_DEG < config::mech::THETA_MAX_DEG);
}

TEST_CASE("換算に使う機械定数は正の値") {
    // 0 を入れると mm→rad 換算がゼロ除算になる。
    CHECK(config::mech::R_MM_PER_OUTPUT_REV > 0.0f);
    CHECK(config::mech::Z_MM_PER_OUTPUT_REV > 0.0f);
    CHECK(config::mech::PINION_TEETH > 0.0f);
    CHECK(config::mech::RING_TEETH > 0.0f);
}

TEST_CASE("方向反転の符号は ±1 のみ") {
    CHECK(std::abs(config::mech::R_SIGN) == doctest::Approx(1.0f));
    CHECK(std::abs(config::mech::Z_SIGN) == doctest::Approx(1.0f));
    CHECK(std::abs(config::mech::THETA_SIGN) == doctest::Approx(1.0f));
}

TEST_CASE("M3508 の内蔵減速比は公称値") {
    CHECK(config::mech::C620_REDUCTION == doctest::Approx(19.2032f).epsilon(0.0001));
}

TEST_CASE("制御周期は 0 でない") {
    CHECK(config::period::M3508_MS > 0);
    CHECK(config::period::DM_MS > 0);
    CHECK(config::period::EL05_MS > 0);
    CHECK(config::period::LCD_MS > 0);
}

TEST_CASE("スティックのデッドゾーンは [0, 1)") {
    // 1.0 にすると入力のスケーリングがゼロ除算になる。
    CHECK(config::manual_control::STICK_DEADZONE >= 0.0f);
    CHECK(config::manual_control::STICK_DEADZONE < 1.0f);
}

TEST_CASE("同一バス上で標準 CAN ID が衝突しない") {
    // モータは 1 本の FDCAN1 に集約するため、標準 ID の衝突は事故に直結する。
    // EL05 は拡張 ID なので標準 ID とは空間が別。
    constexpr uint16_t c620_command = config::can_id::C620_COMMAND;
    constexpr uint16_t c620_feedback = config::can_id::C620_FEEDBACK;
    constexpr uint16_t dm_command = 0x100 + config::can_id::DM_CAN_ID;
    constexpr uint16_t dm_feedback = config::can_id::DM_MST_ID;

    CHECK(c620_feedback == c620_command + config::can_id::C620_ESC_ID);

    CHECK(dm_command != c620_command);
    CHECK(dm_command != c620_feedback);
    CHECK(dm_feedback != c620_command);
    CHECK(dm_feedback != c620_feedback);
}

TEST_CASE("C620 の ESC ID は 1..4") {
    CHECK(config::can_id::C620_ESC_ID >= 1);
    CHECK(config::can_id::C620_ESC_ID <= 4);
}

#include "doctest.h"

#include "domain/dm_codec.hpp"

#include <cstring>

#include <cmath>
#include <limits>

using namespace domain::dm;

namespace {
// マニュアルの既定値に近い、扱いやすいレンジ。
constexpr Range RANGE = {12.5f, 45.0f, 18.0f};
}  // namespace

TEST_CASE("線形マッピングは端と中央が一致する") {
    CHECK(floatToRaw(-12.5f, -12.5f, 12.5f, 16) == 0);
    CHECK(floatToRaw(12.5f, -12.5f, 12.5f, 16) == 65535);
    CHECK(floatToRaw(0.0f, -12.5f, 12.5f, 16) == 32768);

    CHECK(rawToFloat(0, -12.5f, 12.5f, 16) == doctest::Approx(-12.5f));
    CHECK(rawToFloat(65535, -12.5f, 12.5f, 16) == doctest::Approx(12.5f));
    CHECK(rawToFloat(32768, -12.5f, 12.5f, 16) == doctest::Approx(0.0f).epsilon(0.001));
}

TEST_CASE("範囲外の指令は端に張り付く") {
    // 巻き戻して逆向きの指令になると、そのまま暴走につながる。
    CHECK(floatToRaw(100.0f, -12.5f, 12.5f, 16) == 65535);
    CHECK(floatToRaw(-100.0f, -12.5f, 12.5f, 16) == 0);
}

TEST_CASE("NaN は最小値として扱う") {
    CHECK(floatToRaw(std::numeric_limits<float>::quiet_NaN(), -12.5f, 12.5f, 16) == 0);
}

TEST_CASE("フィードバックは ID とエラーを分けて取り出す") {
    // D0 = ID(下位4bit) | ERR(上位4bit)
    const uint8_t data[8] = {0x89, 0x80, 0x00, 0x80, 0x00, 0x00, 45, 38};
    const Feedback fb = decodeFeedback(data, RANGE);

    CHECK(fb.id == 9);
    CHECK(fb.error == error_code::OVERVOLTAGE);
}

TEST_CASE("フィードバックの位置・速度・トルクを復号する") {
    // 位置 0x8000、速度 0x800、トルク 0x800 はいずれもほぼ中央（= 0）。
    // 12bit のフルスケールは 4095 なので、中央 0x800 はわずかに正へずれる。
    const uint8_t data[8] = {0x01, 0x80, 0x00, 0x80, 0x08, 0x00, 0, 0};
    const Feedback fb = decodeFeedback(data, RANGE);

    CHECK(std::abs(fb.position_rad) < 0.01f);
    CHECK(std::abs(fb.velocity_rad_s) < 0.05f);
    CHECK(std::abs(fb.torque_nm) < 0.02f);
}

TEST_CASE("フィードバックのトルクは符号を持つ") {
    // トルク 0x000 は最小、0xFFF は最大。
    uint8_t data[8] = {0x01, 0x80, 0x00, 0x80, 0x00, 0x00, 0, 0};
    CHECK(decodeFeedback(data, RANGE).torque_nm == doctest::Approx(-18.0f).epsilon(0.001));

    data[4] = 0x0F;
    data[5] = 0xFF;
    CHECK(decodeFeedback(data, RANGE).torque_nm == doctest::Approx(18.0f).epsilon(0.001));
}

TEST_CASE("フィードバックの温度は摂氏そのまま") {
    // 温度を読めないと、連続運転で焼くまで気付けない。
    const uint8_t data[8] = {0x01, 0, 0, 0, 0, 0, 52, 71};
    const Feedback fb = decodeFeedback(data, RANGE);

    CHECK(fb.mos_temperature_c == 52);
    CHECK(fb.rotor_temperature_c == 71);
}

TEST_CASE("MIT 指令は 5 つの値をビット単位で詰める") {
    uint8_t out[8] = {};
    encodeMit(0.0f, 0.0f, 0.0f, 0.0f, 0.0f, RANGE, out);

    // 位置と速度は中央、Kp/Kd/トルクは最小。
    CHECK(out[0] == 0x80);
    CHECK(out[1] == 0x00);
    CHECK(out[2] == 0x80);
    CHECK((out[3] >> 4) == 0x0);      // v[3:0]
    CHECK((out[3] & 0x0F) == 0x00);   // kp[11:8]
    CHECK(out[4] == 0x00);            // kp[7:0]
    CHECK(out[5] == 0x00);            // kd[11:4]
}

TEST_CASE("MIT 指令のゲインは最大でフルスケール") {
    uint8_t out[8] = {};
    encodeMit(0.0f, 0.0f, 500.0f, 5.0f, 0.0f, RANGE, out);

    // Kp[11:0] = 0xFFF
    CHECK((out[3] & 0x0F) == 0x0F);
    CHECK(out[4] == 0xFF);
    // Kd[11:0] = 0xFFF
    CHECK(out[5] == 0xFF);
    CHECK((out[6] >> 4) == 0x0F);
}

TEST_CASE("MIT 指令の位置と速度は復号して戻る") {
    uint8_t out[8] = {};
    encodeMit(3.0f, -10.0f, 0.0f, 0.0f, 0.0f, RANGE, out);

    const uint32_t p = (static_cast<uint32_t>(out[0]) << 8) | out[1];
    const uint32_t v = (static_cast<uint32_t>(out[2]) << 4) | (out[3] >> 4);

    CHECK(rawToFloat(p, -RANGE.p_max, RANGE.p_max, 16) == doctest::Approx(3.0f).epsilon(0.001));
    CHECK(rawToFloat(v, -RANGE.v_max, RANGE.v_max, 12) == doctest::Approx(-10.0f).epsilon(0.01));
}

TEST_CASE("位置速度指令は float 2 つをリトルエンディアンで並べる") {
    uint8_t out[8] = {};
    encodePositionVelocity(1.0f, 2.0f, out);

    // 1.0f = 0x3F800000, 2.0f = 0x40000000
    CHECK(out[3] == 0x3F);
    CHECK(out[2] == 0x80);
    CHECK(out[7] == 0x40);
}

TEST_CASE("特殊コマンドは 0xFF 埋めの最終バイト") {
    uint8_t out[8] = {};
    encodeSpecial(SPECIAL_ENABLE, out);

    for (int i = 0; i < 7; ++i) {
        CHECK(out[i] == 0xFF);
    }
    CHECK(out[7] == 0xFC);
}

TEST_CASE("設定フレームは CAN ID とコマンドと値を並べる") {
    uint8_t out[8] = {};
    encodeConfig(0x09, CONFIG_WRITE, reg::CTRL_MODE, 2, out);

    CHECK(out[0] == 0x09);
    CHECK(out[1] == 0x00);
    CHECK(out[2] == 0x55);
    CHECK(out[3] == 0x0A);
    CHECK(out[4] == 0x02);
    CHECK(out[7] == 0x00);
}

TEST_CASE("設定応答をフィードバックと取り違えない") {
    // 応答は通常のフィードバックと同じ ID で返る。
    // 見分けを誤ると、位置や温度に設定値が化けて入る。
    uint8_t reply[8] = {};
    encodeConfig(0x09, CONFIG_READ, reg::PMAX, 0, reply);
    reply[4] = 0x00;
    reply[5] = 0x00;
    reply[6] = 0x48;
    reply[7] = 0x41;  // 12.5f

    CHECK(isConfigReply(0x09, reply));
    CHECK(configReplyRegister(reply) == reg::PMAX);

    const uint32_t bits = configReplyValue(reply);
    float value = 0.0f;
    std::memcpy(&value, &bits, sizeof(float));
    CHECK(value == doctest::Approx(12.5f));
}

TEST_CASE("通常のフィードバックは設定応答と判定されない") {
    const uint8_t feedback[8] = {0x19, 0x80, 0x00, 0x80, 0x08, 0x00, 45, 38};
    CHECK_FALSE(isConfigReply(0x09, feedback));
}

TEST_CASE("別 ID 宛の設定応答は自分のものではない") {
    uint8_t reply[8] = {};
    encodeConfig(0x0A, CONFIG_READ, reg::PMAX, 0, reply);

    CHECK_FALSE(isConfigReply(0x09, reply));
}

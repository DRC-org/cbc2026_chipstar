#include "doctest.h"

#include "domain/el05_codec.hpp"

#include <cmath>
#include <cstring>
#include <initializer_list>

using namespace domain::el05;

TEST_CASE("拡張IDは通信種別・data_area_2・対象IDを詰める") {
    const uint32_t id = buildCanId(comm::WRITE_PARAM, 0xFD, 0x7F);

    CHECK(commType(id) == comm::WRITE_PARAM);
    CHECK(dataArea2(id) == 0xFD);
    CHECK(targetId(id) == 0x7F);
}

TEST_CASE("拡張IDは 29bit に収まる") {
    const uint32_t id = buildCanId(0x1F, 0xFFFF, 0xFF);
    CHECK(id <= 0x1FFFFFFFu);
}

TEST_CASE("通信種別の上位ビットは切り捨てる") {
    // 5bit を超える値を渡しても他のフィールドを壊さない。
    const uint32_t id = buildCanId(0x3F, 0x00, 0x10);
    CHECK(commType(id) == 0x1F);
    CHECK(targetId(id) == 0x10);
}

TEST_CASE("uint16 と float は往復する") {
    for (const float value : {-12.57f, -5.0f, 0.0f, 7.5f, 12.57f}) {
        const uint16_t raw = floatToUint16(value, POSITION_MIN, POSITION_MAX);
        CHECK(uint16ToFloat(raw, POSITION_MIN, POSITION_MAX) ==
              doctest::Approx(value).epsilon(0.001));
    }
}

TEST_CASE("範囲外は端に張り付く") {
    CHECK(floatToUint16(100.0f, POSITION_MIN, POSITION_MAX) == 65535);
    CHECK(floatToUint16(-100.0f, POSITION_MIN, POSITION_MAX) == 0);
}

TEST_CASE("フィードバックはビッグエンディアンで復号する") {
    const uint32_t id = buildCanId(comm::FEEDBACK, 0x7F, 0xFD);
    const uint8_t data[8] = {0x80, 0x00, 0x80, 0x00, 0x80, 0x00, 0x01, 0x2C};
    const Feedback fb = decodeFeedback(id, data);

    CHECK(fb.motor_id == 0x7F);
    CHECK(std::abs(fb.position_rad) < 0.01f);
    CHECK(std::abs(fb.velocity_rad_s) < 0.01f);
    CHECK(std::abs(fb.torque_nm) < 0.01f);
    // 温度は raw/10 [degC]。0x012C = 300 → 30.0
    CHECK(fb.temperature_c == doctest::Approx(30.0f));
}

TEST_CASE("フィードバックの位置は端まで復号できる") {
    const uint32_t id = buildCanId(comm::FEEDBACK, 0x7F, 0xFD);
    const uint8_t low[8] = {0x00, 0x00, 0, 0, 0, 0, 0, 0};
    const uint8_t high[8] = {0xFF, 0xFF, 0, 0, 0, 0, 0, 0};

    CHECK(decodeFeedback(id, low).position_rad == doctest::Approx(POSITION_MIN));
    CHECK(decodeFeedback(id, high).position_rad == doctest::Approx(POSITION_MAX));
}

TEST_CASE("パラメータ書込はインデックスと値を並べる") {
    uint8_t out[8] = {};
    encodeParamFloat(param::LOC_REF, 1.5f, out);

    CHECK(out[0] == 0x16);
    CHECK(out[1] == 0x70);
    CHECK(out[2] == 0);
    CHECK(out[3] == 0);

    float value = 0.0f;
    std::memcpy(&value, &out[4], sizeof(value));
    CHECK(value == doctest::Approx(1.5f));
}

TEST_CASE("uint8 パラメータは値を 1 バイトで置く") {
    uint8_t out[8] = {};
    encodeParamU8(param::RUN_MODE, static_cast<uint8_t>(RunMode::Velocity), out);

    CHECK(out[0] == 0x05);
    CHECK(out[1] == 0x70);
    CHECK(out[4] == 2);
    CHECK(out[5] == 0);
}

TEST_CASE("パラメータ読出要求は値の欄を空にする") {
    uint8_t out[8] = {0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF};
    encodeParamRequest(param::VBUS, out);

    CHECK(out[0] == 0x1C);
    CHECK(out[1] == 0x70);
    for (int i = 2; i < 8; ++i) {
        CHECK(out[i] == 0);
    }
}

TEST_CASE("保存フレームは固定列") {
    uint8_t out[8] = {};
    encodeSave(out);

    for (uint8_t i = 0; i < 8; ++i) {
        CHECK(out[i] == i + 1);
    }
}

TEST_CASE("パラメータ応答から値を取り出す") {
    uint8_t reply[8] = {};
    encodeParamFloat(param::VBUS, 24.5f, reply);

    CHECK(paramReplyIndex(reply) == param::VBUS);
    CHECK(paramReplyFloat(reply) == doctest::Approx(24.5f));
}

TEST_CASE("運転モードの値はマニュアルどおり") {
    CHECK(static_cast<uint8_t>(RunMode::Operation) == 0);
    CHECK(static_cast<uint8_t>(RunMode::Position) == 1);
    CHECK(static_cast<uint8_t>(RunMode::Velocity) == 2);
    CHECK(static_cast<uint8_t>(RunMode::Current) == 3);
    CHECK(static_cast<uint8_t>(RunMode::PositionCsp) == 5);
}

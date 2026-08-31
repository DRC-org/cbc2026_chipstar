#include "doctest.h"

#include "domain/sts3215_protocol.hpp"

#include <vector>

using namespace domain::sts3215;

TEST_CASE("チェックサムは総和のビット反転") {
    // ID=1, 長さ=2, 命令=PING → 1 + 2 + 1 = 4、~4 = 0xFB。
    const uint8_t body[] = {0x01, 0x02, 0x01};
    CHECK(checksum(body, sizeof(body)) == 0xFB);
}

TEST_CASE("チェックサムは 8 ビットで巻き戻る") {
    const uint8_t body[] = {0xFF, 0x01};
    // 0x100 の下位 8 ビットは 0、~0 = 0xFF。
    CHECK(checksum(body, sizeof(body)) == 0xFF);
}

TEST_CASE("PING パケットは 6 バイト") {
    uint8_t packet[MAX_PACKET_SIZE] = {};
    const std::size_t length =
        buildInstruction(packet, sizeof(packet), 1, INSTRUCTION_PING, nullptr, 0);

    REQUIRE(length == 6);
    const std::vector<uint8_t> actual(packet, packet + length);
    CHECK(actual == std::vector<uint8_t>{0xFF, 0xFF, 0x01, 0x02, 0x01, 0xFB});
}

TEST_CASE("WRITE パケットはパラメータを載せる") {
    // ID=2 のトルクを有効化する: アドレス 40 に 1 を書く。
    const uint8_t parameters[] = {reg::TORQUE_ENABLE, 0x01};
    uint8_t packet[MAX_PACKET_SIZE] = {};
    const std::size_t length = buildInstruction(packet, sizeof(packet), 2, INSTRUCTION_WRITE,
                                                parameters, sizeof(parameters));

    REQUIRE(length == 8);
    CHECK(packet[2] == 2);     // ID
    CHECK(packet[3] == 4);     // 長さ = パラメータ数 + 2
    CHECK(packet[4] == INSTRUCTION_WRITE);
    CHECK(packet[5] == reg::TORQUE_ENABLE);
    CHECK(packet[6] == 0x01);

    // チェックサムは ID から最後のパラメータまでを対象にする。
    CHECK(packet[7] == checksum(&packet[2], length - 3));
}

TEST_CASE("組立は引数が不正なら 0 を返す") {
    uint8_t packet[MAX_PACKET_SIZE] = {};

    SUBCASE("パラメータ数に対して実体が無い") {
        CHECK(buildInstruction(packet, sizeof(packet), 1, INSTRUCTION_WRITE, nullptr, 3) == 0);
    }

    SUBCASE("パラメータ数が上限を超える") {
        const std::vector<uint8_t> parameters(MAX_TX_PARAMETERS, 0);
        CHECK(buildInstruction(packet, sizeof(packet), 1, INSTRUCTION_WRITE, parameters.data(),
                               MAX_TX_PARAMETERS + 1) == 0);
    }

    SUBCASE("書き込み先が足りない") {
        uint8_t small[5] = {};
        CHECK(buildInstruction(small, sizeof(small), 1, INSTRUCTION_PING, nullptr, 0) == 0);
    }
}

TEST_CASE("ステータスのチェックサムを検証する") {
    // ID=1、長さ=2、エラー=0 の応答: FF FF 01 02 00 FC
    const uint8_t body[] = {0x00, 0xFC};

    SUBCASE("正しいチェックサムは通る") {
        CHECK(verifyStatusChecksum(1, 2, body));
    }

    SUBCASE("1 ビットでも違えば弾く") {
        const uint8_t broken[] = {0x00, 0xFD};
        CHECK_FALSE(verifyStatusChecksum(1, 2, broken));
    }

    SUBCASE("ID が違えば弾く") {
        CHECK_FALSE(verifyStatusChecksum(2, 2, body));
    }
}

TEST_CASE("パラメータ付きステータスのチェックサム") {
    // ID=1 が現在位置 0x0123 を返す: 長さ=4, エラー=0, データ=23 01
    // 1 + 4 + 0 + 0x23 + 0x01 = 0x29、~0x29 = 0xD6
    const uint8_t body[] = {0x00, 0x23, 0x01, 0xD6};
    CHECK(verifyStatusChecksum(1, 4, body));
}

TEST_CASE("16 ビット値はリトルエンディアン") {
    uint8_t data[2] = {};
    encodeUint16(0x1234, data);

    CHECK(data[0] == 0x34);
    CHECK(data[1] == 0x12);
    CHECK(decodeUint16(data) == 0x1234);
}

TEST_CASE("目標値は加速度・位置・時間・速度の順で並ぶ") {
    const Target target = {1, 50, 2048, 0, 500};
    uint8_t data[TARGET_DATA_LENGTH] = {};
    encodeTarget(target, data);

    CHECK(data[0] == 50);       // 加速度
    CHECK(decodeUint16(&data[1]) == 2048);  // 位置
    CHECK(decodeUint16(&data[3]) == 0);     // 時間
    CHECK(decodeUint16(&data[5]) == 500);   // 速度
}

TEST_CASE("SyncWrite のパラメータ領域は最大構成でも上限に収まる") {
    CHECK(MAX_SYNC_PARAMETERS <= MAX_TX_PARAMETERS);
}

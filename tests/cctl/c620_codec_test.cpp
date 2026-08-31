#include "doctest.h"

#include "domain/c620_codec.hpp"

using namespace domain::c620;

TEST_CASE("フィードバックはビッグエンディアンで復号される") {
    // 角度 0x1234、回転数 -100、電流 0x0300、温度 42。
    const uint8_t data[8] = {0x12, 0x34, 0xFF, 0x9C, 0x03, 0x00, 42, 0};
    const Feedback fb = decodeFeedback(data);

    CHECK(fb.raw_angle == 0x1234);
    CHECK(fb.rpm == -100);
    CHECK(fb.current_raw == 0x0300);
    CHECK(fb.temperature == 42);
}

TEST_CASE("多回転カウンタは最初の角度を原点にする") {
    MultiTurnCounter counter;
    CHECK_FALSE(counter.hasReference());

    counter.update(5000);

    CHECK(counter.hasReference());
    CHECK(counter.counts() == 0);
    CHECK(counter.degrees() == doctest::Approx(0.0f));
}

TEST_CASE("多回転カウンタは差分を積算する") {
    MultiTurnCounter counter;
    counter.update(1000);
    counter.update(1500);
    counter.update(2000);

    CHECK(counter.counts() == 1000);
}

TEST_CASE("多回転カウンタは正転側の巻き戻りを跨ぐ") {
    MultiTurnCounter counter;
    counter.update(8100);
    // 8100 -> 100 は 0 を跨ぐ前進。8192 - 8100 + 100 = 192。
    counter.update(100);

    CHECK(counter.counts() == 192);
}

TEST_CASE("多回転カウンタは逆転側の巻き戻りを跨ぐ") {
    MultiTurnCounter counter;
    counter.update(100);
    // 100 -> 8100 は 0 を跨ぐ後退。-(100 + 8192 - 8100) = -192。
    counter.update(8100);

    CHECK(counter.counts() == -192);
}

TEST_CASE("多回転カウンタは複数回転を積算する") {
    MultiTurnCounter counter;
    counter.update(0);
    // 1/4 回転ずつ進めて 2 回転させる。
    for (int i = 1; i <= 8; ++i) {
        counter.update(static_cast<uint16_t>((i * 2048) % COUNTS_PER_REV));
    }

    CHECK(counter.counts() == 2 * COUNTS_PER_REV);
    CHECK(counter.degrees() == doctest::Approx(720.0f));
}

TEST_CASE("角度は 8192 カウントで 360 度") {
    MultiTurnCounter counter;
    counter.update(0);
    counter.update(2048);

    CHECK(counter.degrees() == doctest::Approx(90.0f));
}

TEST_CASE("電流はフルスケール ±20000mA が ±16384 に対応する") {
    CHECK(currentToRaw(0) == 0);
    CHECK(currentToRaw(20000) == 16384);
    CHECK(currentToRaw(-20000) == -16384);
    CHECK(currentToRaw(10000) == 8192);
    CHECK(currentToRaw(-10000) == -8192);
}

TEST_CASE("指令は ESC ID に対応するスロットへ書かれる") {
    SUBCASE("ESC ID 1 は先頭 2 バイト") {
        uint8_t frame[8] = {};
        writeCommandSlot(frame, 1, 0x1234);

        CHECK(frame[0] == 0x12);
        CHECK(frame[1] == 0x34);
        for (int i = 2; i < 8; ++i) {
            CHECK(frame[i] == 0);
        }
    }

    SUBCASE("ESC ID 4 は末尾 2 バイト") {
        uint8_t frame[8] = {};
        writeCommandSlot(frame, 4, 0x00FF);

        CHECK(frame[6] == 0x00);
        CHECK(frame[7] == 0xFF);
        for (int i = 0; i < 6; ++i) {
            CHECK(frame[i] == 0);
        }
    }

    SUBCASE("負の指令は 2 の補数で並ぶ") {
        uint8_t frame[8] = {};
        writeCommandSlot(frame, 2, -1);

        CHECK(frame[2] == 0xFF);
        CHECK(frame[3] == 0xFF);
    }
}

TEST_CASE("半回転ちょうどの移動は前進として扱われる") {
    // 1 サンプルで半回転を超えると方向は原理的に区別できない。
    // 境界がどちらに倒れるかをここで固定しておく。
    MultiTurnCounter counter;
    counter.update(0);
    counter.update(COUNTS_PER_REV / 2);

    CHECK(counter.counts() == COUNTS_PER_REV / 2);
}

TEST_CASE("生値と電流は往復する") {
    // テレメトリで実電流を mA に戻すため、逆変換を用意する。
    CHECK(rawToCurrent(16384) == 20000);
    CHECK(rawToCurrent(-16384) == -20000);
    CHECK(rawToCurrent(0) == 0);
    CHECK(rawToCurrent(currentToRaw(5000)) == doctest::Approx(5000).epsilon(0.001));
}

TEST_CASE("ESC ID で載る指令フレームが決まる") {
    CHECK(groupCommandId(1) == COMMAND_ID_1_TO_4);
    CHECK(groupCommandId(4) == COMMAND_ID_1_TO_4);
    CHECK(groupCommandId(5) == COMMAND_ID_5_TO_8);
    CHECK(groupCommandId(8) == COMMAND_ID_5_TO_8);
}

TEST_CASE("範囲外の ESC ID はどのフレームにも属さない") {
    CHECK(groupCommandId(0) == 0);
    CHECK(groupCommandId(9) == 0);
}

TEST_CASE("5..8 のスロットは 1..4 と同じ並び") {
    uint8_t frame[8] = {};
    writeCommandSlot(frame, 5, 0x1234);

    CHECK(frame[0] == 0x12);
    CHECK(frame[1] == 0x34);
}

TEST_CASE("指令フレームは 4 台分を保持する") {
    // 1 台ずつ別フレームで送ると、後のフレームが他台を 0 で上書きしてしまう。
    CommandFrame frame(COMMAND_ID_1_TO_4);
    frame.clear();

    CHECK(frame.set(1, 0x0111));
    CHECK(frame.set(2, 0x0222));
    CHECK(frame.set(3, 0x0333));
    CHECK(frame.set(4, 0x0444));

    const uint8_t* data = frame.data();
    CHECK(data[0] == 0x01);
    CHECK(data[1] == 0x11);
    CHECK(data[6] == 0x04);
    CHECK(data[7] == 0x44);
}

TEST_CASE("担当外の ESC ID は書き込めない") {
    CommandFrame frame(COMMAND_ID_1_TO_4);
    CHECK_FALSE(frame.set(5, 0x1234));
    CHECK_FALSE(frame.set(0, 0x1234));

    CommandFrame upper(COMMAND_ID_5_TO_8);
    CHECK(upper.set(5, 0x1234));
    CHECK_FALSE(upper.set(1, 0x1234));
}

TEST_CASE("clear で書かれなかった台は 0 電流になる") {
    CommandFrame frame(COMMAND_ID_1_TO_4);
    frame.set(1, 0x7FFF);
    frame.clear();

    for (int i = 0; i < 8; ++i) {
        CHECK(frame.data()[i] == 0);
    }
}

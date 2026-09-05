#include "doctest.h"

#include "domain/led_pattern.hpp"

using domain::ledPattern;
using domain::RunMode;

TEST_CASE("Safe では LED1 だけが 1Hz で点滅する") {
    CHECK(ledPattern(0, RunMode::Safe, 0) == domain::slot_bit::SLOT0);
    CHECK(ledPattern(499, RunMode::Safe, 0) == domain::slot_bit::SLOT0);
    CHECK(ledPattern(500, RunMode::Safe, 0) == 0);
    CHECK(ledPattern(999, RunMode::Safe, 0) == 0);
    CHECK(ledPattern(1000, RunMode::Safe, 0) == domain::slot_bit::SLOT0);
}

TEST_CASE("Safe では有効軸に関係なく同じ表示") {
    // まだ動かない状態であることを、軸の設定より優先して伝える。
    CHECK(ledPattern(0, RunMode::Safe, domain::slot_bit::ALL) == domain::slot_bit::SLOT0);
}

TEST_CASE("Stop では 3 つとも速く点滅する") {
    CHECK(ledPattern(0, RunMode::Stop, 0) == domain::slot_bit::ALL);
    CHECK(ledPattern(100, RunMode::Stop, 0) == 0);
    CHECK(ledPattern(200, RunMode::Stop, domain::slot_bit::ALL) == domain::slot_bit::ALL);
}

TEST_CASE("Run では有効な軸がそのまま点灯する") {
    CHECK(ledPattern(0, RunMode::Run, 0) == 0);
    CHECK(ledPattern(12345, RunMode::Run, domain::slot_bit::SLOT1) == domain::slot_bit::SLOT1);
    CHECK(ledPattern(12345, RunMode::Run, domain::slot_bit::ALL) == domain::slot_bit::ALL);
}

TEST_CASE("Run の表示は時間で変わらない") {
    // 点滅していると、軸が有効かどうかを読み取れない。
    const uint8_t axes = domain::slot_bit::SLOT0 | domain::slot_bit::SLOT2;
    CHECK(ledPattern(0, RunMode::Run, axes) == axes);
    CHECK(ledPattern(999999, RunMode::Run, axes) == axes);
}

TEST_CASE("軸ビット以外は落とす") {
    CHECK(ledPattern(0, RunMode::Run, 0xFF) == domain::slot_bit::ALL);
}

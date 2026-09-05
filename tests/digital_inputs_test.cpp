#include "doctest.h"
#include "domain/digital_inputs.hpp"

TEST_CASE("実装されていない接点をマスクする") {
    domain::DigitalInputs inputs(7);
    inputs.sample(0xFF, 0x0F, 0);
    CHECK(inputs.available() == 7);
    CHECK(inputs.raw() == 7);
    CHECK(inputs.dip() == 0x0F);
}

TEST_CASE("初回のサンプルは安定値として即座に確定する") {
    domain::DigitalInputs inputs(7);
    inputs.sample(5, 0, 1000);
    CHECK(inputs.raw() == 5);
    CHECK(inputs.stable() == 5);
}

TEST_CASE("生値は即時、安定値は10ms保持後に反映する") {
    domain::DigitalInputs inputs(7);
    inputs.sample(0, 0, 0);
    inputs.sample(1, 0, 1);
    CHECK(inputs.raw() == 1);
    CHECK(inputs.stable() == 0);
    inputs.sample(1, 0, 10);
    CHECK(inputs.stable() == 0);
    inputs.sample(1, 0, 11);
    CHECK(inputs.stable() == 1);
}

TEST_CASE("跳ねている間は安定値を更新しない") {
    domain::DigitalInputs inputs(7);
    inputs.sample(0, 0, 0);
    for (uint32_t now = 1; now < 40; ++now) inputs.sample(now % 2, 0, now);
    CHECK(inputs.stable() == 0);
    for (uint32_t now = 40; now <= 51; ++now) inputs.sample(1, 0, now);
    CHECK(inputs.stable() == 1);
}

TEST_CASE("接点ごとに独立してデバウンスする") {
    domain::DigitalInputs inputs(7);
    inputs.sample(0, 0, 0);
    inputs.sample(1, 0, 1);
    inputs.sample(3, 0, 6);
    inputs.sample(3, 0, 11);
    CHECK(inputs.stable() == 1);
    inputs.sample(3, 0, 16);
    CHECK(inputs.stable() == 3);
}

TEST_CASE("タイマの周回をまたいでも保持時間を数える") {
    domain::DigitalInputs inputs(7);
    inputs.sample(0, 0, UINT32_MAX - 4);
    inputs.sample(1, 0, UINT32_MAX - 3);
    CHECK(inputs.stable() == 0);
    inputs.sample(1, 0, 7);
    CHECK(inputs.stable() == 1);
}

TEST_CASE("接点を持たない基板でも DIP を読める") {
    domain::DigitalInputs inputs(0);
    inputs.sample(0xFF, 0x0A, 0);
    CHECK(inputs.raw() == 0);
    CHECK(inputs.stable() == 0);
    CHECK(inputs.dip() == 0x0A);
    CHECK(inputs.available() == 0);
}

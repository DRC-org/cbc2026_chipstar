#include "doctest.h"
#include "domain/status_led.hpp"

using domain::Status;
using domain::statusPattern;

TEST_CASE("RUNは全灯を点けたままにする") {
    for (uint32_t t = 0; t < 2000; t += 37) {
        CHECK(statusPattern(t, Status::Run, 3) == 0b111);
        CHECK(statusPattern(t, Status::Run, 1) == 0b1);
        CHECK(statusPattern(t, Status::Run, 6) == 0b111111);
    }
}

TEST_CASE("SAFEとSTOPは速さで区別する") {
    // SAFE: 1Hz、点灯と消灯が半々。
    CHECK(statusPattern(0, Status::Safe, 3) == 0b111);
    CHECK(statusPattern(499, Status::Safe, 3) == 0b111);
    CHECK(statusPattern(500, Status::Safe, 3) == 0);
    CHECK(statusPattern(1000, Status::Safe, 3) == 0b111);

    // STOP: 4Hz。SAFEの4倍の速さ。
    CHECK(statusPattern(0, Status::Stop, 3) == 0b111);
    CHECK(statusPattern(124, Status::Stop, 3) == 0b111);
    CHECK(statusPattern(125, Status::Stop, 3) == 0);
    CHECK(statusPattern(250, Status::Stop, 3) == 0b111);
}

TEST_CASE("ERRORは2回点滅して休む") {
    CHECK(statusPattern(0, Status::Error, 3) == 0b111);
    CHECK(statusPattern(150, Status::Error, 3) == 0);
    CHECK(statusPattern(250, Status::Error, 3) == 0b111);
    // 残りは消灯。この間があるので速い点滅と見分けられる。
    for (uint32_t t = 300; t < 1000; t += 50) {
        CHECK(statusPattern(t, Status::Error, 3) == 0);
    }
}

TEST_CASE("BOOTは1灯ずつ流れる") {
    CHECK(statusPattern(0, Status::Boot, 3) == 0b001);
    CHECK(statusPattern(100, Status::Boot, 3) == 0b010);
    CHECK(statusPattern(200, Status::Boot, 3) == 0b100);
    // 末尾は消灯して一拍おく。
    CHECK(statusPattern(300, Status::Boot, 3) == 0);
    CHECK(statusPattern(500, Status::Boot, 3) == 0);
    CHECK(statusPattern(600, Status::Boot, 3) == 0b001);
}

TEST_CASE("LEDが1個の基板でも状態を区別できる") {
    // 1個でも、流れる代わりに短い点滅と長い休みになる。
    CHECK(statusPattern(0, Status::Boot, 1) == 0b1);
    CHECK(statusPattern(100, Status::Boot, 1) == 0);
    CHECK(statusPattern(300, Status::Boot, 1) == 0);
    CHECK(statusPattern(400, Status::Boot, 1) == 0b1);
    // 点灯している割合がSAFE(50%)やSTOP(50%)と違う。
    CHECK(statusPattern(0, Status::Safe, 1) == 0b1);
    CHECK(statusPattern(600, Status::Safe, 1) == 0);
}

TEST_CASE("LEDがない構成では何も点けない") {
    CHECK(statusPattern(0, Status::Run, 0) == 0);
    CHECK(statusPattern(500, Status::Error, 0) == 0);
}

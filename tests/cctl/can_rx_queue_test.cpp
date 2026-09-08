#include "doctest.h"
#include "domain/can_rx_queue.hpp"

TEST_CASE("周辺CANの監視分割データとACKを順序通り保持する") {
    domain::CanRxQueue queue;
    for (unsigned cycle = 0; cycle < 100; ++cycle) {
        for (unsigned part = 0; part < 5; ++part) {
            domain::CanFrame f;
            f.id = part == 4 ? 806 : 807;
            f.length = 8;
            f.data[0] = cycle;
            f.data[3] = part;
            REQUIRE(queue.push(f));
        }
        domain::CanFrame f;
        for (unsigned part = 0; part < 5; ++part) {
            REQUIRE(queue.pop(f));
            CHECK(f.id == (part == 4 ? 806 : 807));
            CHECK(f.data[0] == cycle);
            CHECK(f.data[3] == part);
        }
        CHECK_FALSE(queue.pop(f));
    }
}

TEST_CASE("受信キュー満杯は未読フレームを上書きせず失敗を返す") {
    domain::CanRxQueue queue;
    domain::CanFrame f;
    for (unsigned i = 0; i < queue.CAPACITY; ++i) {
        f.id = i;
        REQUIRE(queue.push(f));
    }
    CHECK_FALSE(queue.push(f));
    for (unsigned i = 0; i < queue.CAPACITY; ++i) {
        REQUIRE(queue.pop(f));
        CHECK(f.id == i);
    }
    CHECK(queue.push(f));
    CHECK(queue.pop(f));
}

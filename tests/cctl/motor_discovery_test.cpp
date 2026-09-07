#include "doctest.h"

#include "domain/motor_discovery.hpp"

TEST_CASE("識別探索は全IDへの読取りだけを送りRUNで中断する") {
    domain::MotorDiscovery scan;
    domain::CanFrame frame;
    for (uint32_t i = 0; i < 2304; ++i) {
        REQUIRE(scan.next((i + 1) * 20, true, 253, frame));
        CHECK(frame.length == 8);
        if (i < 256) {
            CHECK(frame.extended);
            CHECK(frame.id == domain::el05::buildCanId(0, 253, static_cast<uint8_t>(i)));
            for (auto byte : frame.data) CHECK(byte == 0);
        } else {
            CHECK_FALSE(frame.extended);
            CHECK(frame.id == 0x7FF);
            CHECK(frame.data[2] == 0x33);
            CHECK(frame.data[3] == 8);
            CHECK((frame.data[0] | (frame.data[1] << 8)) == i - 256);
        }
    }
    CHECK(scan.done());
    CHECK_FALSE(scan.cancelled());
    CHECK_FALSE(scan.next(50000, true, 253, frame));
    domain::MotorDiscovery interrupted;
    CHECK_FALSE(interrupted.next(20, false, 253, frame));
    CHECK_FALSE(interrupted.next(40, true, 253, frame));
    CHECK(interrupted.cancelled());
    CHECK_FALSE(interrupted.next(60, false, 253, frame));
    CHECK(interrupted.cancelled());
}

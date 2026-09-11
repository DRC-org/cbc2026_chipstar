#include "doctest.h"
#include "domain/can_rx_queue.hpp"
#include "domain/servo_can.hpp"

TEST_CASE("STS停止の読戻し中に届くSAFE TARGET ENABLE RUNを保持する") {
  domain::CanRxQueue queue;
  for (unsigned cycle = 0; cycle < 100; ++cycle) {
    // 単体テストが送る、ハードウェアFIFOの3件を超える連続指令。
    const uint8_t frames[][8] = {
      {1, 3, 0, 0, 0, 0, 0, 0},
      {1, 1, 0, 0, 0, 0, 0, 0},
      {1, 4, 2, 20, 0x0c, 0x38, 1, 0x2c},
      {1, 6, 2, 1, 0, 0, 0, 0},
      {1, 2, 0, 0, 0, 0, 0, 0},
      {1, 5, 0, 0, 0, 0, 0, 0},
    };
    for (const auto& bytes : frames) {
      domain::CanCommandFrame frame;
      frame.length = 8;
      frame.received_ms = cycle * 100;
      for (unsigned i = 0; i < 8; ++i) frame.data[i] = bytes[i];
      REQUIRE(queue.push(frame));
    }
    for (const auto& bytes : frames) {
      domain::CanCommandFrame frame;
      REQUIRE(queue.pop(frame));
      CHECK(frame.received_ms == cycle * 100);
      CHECK_FALSE(frame.expired(cycle * 100 + 20, 250));
      for (unsigned i = 0; i < 8; ++i) CHECK(frame.data[i] == bytes[i]);
      domain::ServoCommand command;
      REQUIRE(domain::servo_can::parse(frame.data, frame.length, command));
    }
    CHECK_FALSE(queue.takeOverflow());
    domain::CanCommandFrame empty;
    CHECK_FALSE(queue.pop(empty));
  }
}

TEST_CASE("STSの受信欠落を通知して残った開始指令を破棄する") {
  domain::CanRxQueue queue;
  domain::CanCommandFrame frame;
  frame.data[1] = 2;
  for (unsigned i = 0; i < queue.CAPACITY; ++i) REQUIRE(queue.push(frame));
  CHECK_FALSE(queue.push(frame));
  CHECK(queue.takeOverflow());
  queue.discard();
  CHECK_FALSE(queue.pop(frame));
  CHECK_FALSE(queue.takeOverflow());
  queue.markOverflow(); // ハードウェアFIFOのオーバーランも同じ経路で通知する。
  CHECK(queue.takeOverflow());
  frame.data[1] = 0;
  REQUIRE(queue.push(frame));
  REQUIRE(queue.pop(frame));
  CHECK(frame.data[1] == 0);
}

TEST_CASE("キューで待った指令は受信時刻から期限を判定する") {
  domain::CanCommandFrame frame;
  frame.received_ms = UINT32_MAX - 100;
  CHECK_FALSE(frame.expired(149, 250));
  CHECK(frame.expired(150, 250));
}

#include "doctest.h"
#include "domain/controller.hpp"
#include "domain/encoder.hpp"
using namespace dcmd;

TEST_CASE("DCMD validates wire bounds and reserved bytes") {
  Command cmd;
  uint8_t data[8] = {1, 4, 0, 0, 0xFC, 0x7C, 0, 0};
  CHECK(parse(data, 8, cmd));
  CHECK(cmd.duty == -900);
  data[5] = 0x7B;
  CHECK_FALSE(parse(data, 8, cmd));
  data[5] = 0x7C;
  data[6] = 1;
  CHECK_FALSE(parse(data, 8, cmd));
  CHECK_FALSE(parse(nullptr, 8, cmd));
  data[6] = 0;
  data[2] = 1;
  CHECK_FALSE(parse(data, 8, cmd));
}

TEST_CASE("Encoder extends forward and reverse timer wraps") {
  Encoder encoder;
  encoder.sample(65535);
  CHECK(encoder.count() == UINT32_MAX);
  encoder.sample(0);
  CHECK(encoder.count() == 0);
  encoder.sample(30000);
  encoder.sample(60000);
  encoder.sample(100);
  CHECK(encoder.count() == 65636);
  encoder.sample(60000);
  CHECK(encoder.count() == 60000);
}

TEST_CASE("DCMD requires handshake and targets and latches timeout") {
  Controller c;
  CHECK(c.mode() == Mode::Safe);
  CHECK_FALSE(c.apply({Op::Run, 1, 0}, 0));
  CHECK(c.apply({Op::Hello}, 0));
  CHECK_FALSE(c.apply({Op::Run, 1, 0}, 0));
  CHECK(c.apply({Op::Target, 0, 100}, 0));
  CHECK_FALSE(c.apply({Op::Target, 1, 100}, 0));
  CHECK_FALSE(c.apply({Op::Run, 3, 0}, 0));
  CHECK(c.apply({Op::Run, 1, 0}, 0));
  c.tick(10);
  CHECK(c.output(0) == 1);
  c.tick(251);
  CHECK(c.output(0) == 0);
  CHECK(c.mode() == Mode::Stop);
  CHECK(c.timedOut());
  c.apply({Op::Target, 0, 100}, 252);
  CHECK_FALSE(c.apply({Op::Run, 1, 0}, 252));
  c.tick(262);
  CHECK(c.output(0) == 0);
}

TEST_CASE("DCMD reverses only after ramp to zero and two seconds braking") {
  Controller c;
  c.apply({Op::Hello}, 0);
  c.apply({Op::Target, 0, 2}, 0);
  c.apply({Op::Run, 1, 0}, 0);
  c.tick(10); c.tick(20);
  CHECK(c.output(0) == 2);
  c.apply({Op::Target, 0, -2}, 20);
  for (uint32_t t = 30; t < 2040; t += 10) {
    c.apply({Op::Heartbeat}, t);
    CHECK(c.output(0) >= 0);
  }
  c.apply({Op::Heartbeat}, 2040);
  CHECK(c.output(0) == -1);
  c.apply({Op::Stop}, 2041);
  CHECK(c.output(0) == 0);
}

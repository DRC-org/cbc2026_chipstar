#include "doctest.h"
#include "domain/digital_inputs.hpp"

TEST_CASE("Inputs debounce independently and guard latches raw contact immediately") {
  domain::DigitalInputs inputs(7);
  uint8_t state[8];
  CHECK_FALSE(inputs.configure(1, 0));
  inputs.sample(0, 5, 0);
  CHECK(inputs.configure(1, 0));
  inputs.sample(1, 5, 1);
  CHECK(inputs.tripped());
  inputs.encode(state);
  CHECK(state[1] == 1);
  CHECK(state[2] == 0);
  CHECK(state[3] == 5);
  inputs.sample(0, 5, 2); // bounce cannot release the stop latch
  CHECK(inputs.tripped());
  inputs.sample(1, 5, 3);
  inputs.sample(3, 5, 8);
  inputs.sample(3, 5, 13);
  inputs.encode(state);
  CHECK(state[2] == 1);
  inputs.sample(3, 5, 18);
  inputs.encode(state);
  CHECK(state[2] == 3);
  CHECK(inputs.configure(1, 0));
  CHECK(inputs.tripped()); // configuring an asserted input cannot clear the latch
  inputs.sample(0, 5, 19);
  CHECK(inputs.tripped());
  CHECK(inputs.configure(1, 0));
  CHECK_FALSE(inputs.tripped());
}

TEST_CASE("Open-circuit guard, invalid masks and timer wrap") {
  domain::DigitalInputs inputs(7);
  inputs.sample(1, 0, UINT32_MAX - 4);
  CHECK(inputs.configure(1, 1)); // NC circuit: opening to HIGH is a stop condition
  CHECK_FALSE(inputs.tripped());
  inputs.sample(1, 0, 5);
  uint8_t state[8]; inputs.encode(state);
  CHECK(state[2] == 1);
  inputs.sample(0, 0, 6);
  CHECK(inputs.tripped());
  CHECK_FALSE(inputs.configure(8, 0));
  CHECK_FALSE(inputs.configure(1, 2));
  CHECK(inputs.tripped());
  CHECK(inputs.configure(0, 0));
  CHECK_FALSE(inputs.tripped());
  domain::DigitalInputs dip_only(0);
  dip_only.sample(255, 15, 0);
  CHECK_FALSE(dip_only.configure(1, 0));
  dip_only.encode(state);
  CHECK(state[1] == 0);
  CHECK(state[3] == 15);
  CHECK(state[7] == 0);
}

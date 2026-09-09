#include "doctest.h"
#include "domain/indicator.hpp"
#include <string>

TEST_CASE("indicator explains output state and high error bits") {
  domain::IndicatorState state;
  auto frame = domain::indicatorFrame(state);
  CHECK(std::string(frame.lines[0]) == "SAFE            ");
  CHECK(std::string(frame.lines[1]) == "WAITING FOR HOST");
  state.host_ready = true;
  state.mode = domain::RunMode::Run;
  frame = domain::indicatorFrame(state);
  CHECK(std::string(frame.lines[1]) == "OUTPUTS OFF     ");
  state.enabled = 5;
  frame = domain::indicatorFrame(state);
  CHECK(std::string(frame.lines[1]) == "MOTORS ON: 0 2  ");
  state.errors[1] = domain::error_bit::FEEDBACK_LOST;
  frame = domain::indicatorFrame(state);
  CHECK(frame.alarm);
  CHECK(std::string(frame.lines[1]) == "MOTOR 1 NO REPLY");
  state.errors[1] = domain::error_bit::OVER_TEMPERATURE;
  CHECK(std::string(domain::indicatorFrame(state).lines[1]) == "MOTOR 1 TOO HOT ");
}

TEST_CASE("indicator preserves stop reason and distinguishes pending stop") {
  domain::IndicatorState state;
  state.mode = domain::RunMode::Stop;
  state.host_timeout = true;
  auto frame = domain::indicatorFrame(state);
  CHECK(std::string(frame.lines[0]) == "STOPPED         ");
  CHECK(std::string(frame.lines[1]) == "HOST TIMEOUT    ");
  state.stop_pending = true;
  frame = domain::indicatorFrame(state);
  CHECK(std::string(frame.lines[0]) == "STOPPING        ");
  CHECK(std::string(frame.lines[1]) == "CAN SEND FAILED ");
  CHECK(frame.alarm);
}

TEST_CASE("tone finishes across clock wrap and alarm preempts start") {
  domain::IndicatorTone tone;
  domain::IndicatorState state;
  CHECK(tone.update(0xffffffc0U, state, false) == 988);
  CHECK(tone.update(16, state, false) == 1319);
  CHECK(tone.update(136, state, false) == 0);
  state.mode = domain::RunMode::Run;
  state.enabled = 1;
  CHECK(tone.update(200, state, false) == 1319);
  CHECK(tone.update(220, state, true) == 2200);
  CHECK(tone.update(320, state, true) == 0);
  CHECK(tone.update(420, state, true) == 2200);
  CHECK(tone.update(620, state, true) == 2200);
  CHECK(tone.update(820, state, true) == 0);
  CHECK(tone.update(5000, state, true) == 0);
  CHECK(tone.update(5001, state, false) == 0);
  CHECK(tone.update(5002, state, true) == 2200);
}

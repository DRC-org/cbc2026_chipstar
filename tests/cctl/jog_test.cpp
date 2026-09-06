#include "doctest.h"
#include "domain/jog.hpp"
#include "domain/command.hpp"
#include <cstring>

TEST_CASE("速度指令は有限値のみ受け付ける") {
  const char* text = "JOG 2 -1.5";
  const auto command = domain::parseCommand(text, std::strlen(text));
  CHECK(command.kind == domain::CommandKind::Jog);
  CHECK(command.target == doctest::Approx(-1.5));
  CHECK(domain::extendsDeadline(command.kind));
  CHECK(domain::parseCommand("JOG 0 nan", 9).kind == domain::CommandKind::None);
}

TEST_CASE("拘束された軸へ位置目標を蓄積せず解放時はその場を保持する") {
  domain::Jog jog;
  jog.command(10, 5);
  float target = 0;
  for (int i = 0; i < 1000; ++i) target = jog.step(5, 0.01, -100, 100);
  CHECK(target == doctest::Approx(6));
  jog.command(0, 5);
  CHECK(jog.step(5, 0.01, -100, 100) == doctest::Approx(5));
  jog.command(0, 5.2f);
  CHECK(jog.step(5.2f, 0.01, -100, 100) == doctest::Approx(5));
  jog.reset(20);
  CHECK_FALSE(jog.active());
  jog.command(-10, 20);
  CHECK(jog.step(20, 0.01, -100, 100) == doctest::Approx(19.9f));
}

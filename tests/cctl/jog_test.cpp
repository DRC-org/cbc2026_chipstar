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
  for (int i = 0; i < 1000; ++i) target = jog.step(5, 0.01);
  CHECK(target == doctest::Approx(6));
  jog.command(0, 5);
  CHECK(jog.step(5, 0.01) == doctest::Approx(5));
  jog.command(0, 5.2f);
  CHECK(jog.step(5.2f, 0.01) == doctest::Approx(5));
  jog.reset(20);
  CHECK_FALSE(jog.active());
  jog.command(-10, 20);
  CHECK(jog.step(20, 0.01) == doctest::Approx(19.9f));
}

TEST_CASE("低速への減速と通常速度への復帰で位置目標の速度を不連続にしない") {
  for (const float direction : {-1.0f, 1.0f}) {
    domain::Jog jog;
    jog.command(direction * 100.0f, 0.0f);
    float target = 0.0f;
    for (int i = 0; i < 10; ++i) target = jog.step(0.0f, 0.01f);

    // hostで生成済みの加減速速度を入力する。追従遅れが新しい低速の
    // 0.1秒分より大きくても、既存目標を実測側へ引き戻してはならない。
    for (int sample = 0; sample < 180; ++sample) {
      const float speed = sample < 90 ? 99.0f - sample : 11.0f + (sample - 90);
      const float velocity = direction * speed;
      const float measured = target - direction * 8.0f;
      jog.command(velocity, measured);
      const float next = jog.step(measured, 0.01f);
      CHECK(next - target == doctest::Approx(velocity * 0.01f).epsilon(0.0001));
      CHECK(direction * (next - target) > 0.0f);
      target = next;
    }
  }
}

TEST_CASE("低速へ切り替えた後の拘束でも先行量は区間最高速の0.1秒に収まる") {
  for (const float direction : {-1.0f, 1.0f}) {
    domain::Jog jog;
    jog.command(direction * 100.0f, 0.0f);
    float target = 0.0f;
    for (int i = 0; i < 500; ++i) {
      target = jog.step(0.0f, 0.01f);
      CHECK(std::abs(target) <= 10.0f);
    }
    CHECK(target == doctest::Approx(direction * 10.0f));
    jog.command(direction * 10.0f, 0.0f);
    for (int i = 0; i < 500; ++i) {
      target = jog.step(0.0f, 0.01f);
      CHECK(target == doctest::Approx(direction * 10.0f));
    }
  }
}

TEST_CASE("停止と反転とresetで以前の最高速に由来する先行幅を引き継がない") {
  for (const float direction : {-1.0f, 1.0f}) {
    for (int action = 0; action < 3; ++action) {
      domain::Jog jog;
      jog.command(direction * 100.0f, 0.0f);
      for (int i = 0; i < 20; ++i) jog.step(0.0f, 0.01f);
      float velocity = direction * 10.0f;
      if (action == 0) {
        jog.command(0.0f, 2.0f);
        CHECK(jog.step(2.0f, 0.01f) == doctest::Approx(2.0f));
      } else if (action == 1) {
        velocity = -velocity;
      } else {
        jog.reset(2.0f);
        CHECK_FALSE(jog.active());
      }
      jog.command(velocity, 2.0f);
      float target = 0.0f;
      for (int i = 0; i < 500; ++i) target = jog.step(2.0f, 0.01f);
      CHECK(target == doctest::Approx(2.0f + velocity * 0.1f));
    }
  }
}

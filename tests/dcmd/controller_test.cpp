#include "doctest.h"
#include "domain/controller.hpp"
#include "domain/encoder.hpp"
using namespace dcmd;

TEST_CASE("接点は報告するだけで出力に作用しない") {
  Controller c;
  c.updateInputs(0, 0, 0);
  CHECK(c.apply({Op::Hello}, 0));
  CHECK(c.apply({Op::Target, 0, 100}, 0));
  CHECK(c.apply({Op::Run, 1}, 0));
  c.tick(10);
  CHECK(c.output(0) == 1);
  c.updateInputs(1, 2, 11);
  CHECK(c.output(0) == 1);
  CHECK(c.mode() == Mode::Run);
  CHECK(c.inputs().raw() == 1);
  CHECK(c.inputs().dip() == 2);
}

TEST_CASE("INPUT READ はchannelとdutyを0に要求する") {
  Command command;
  uint8_t data[8] = {1, 6, 0, 0, 0, 0, 0, 0};
  CHECK(parse(data, 8, command));
  CHECK(command.op == Op::InputRead);
  data[2] = 1;
  CHECK_FALSE(parse(data, 8, command));
  data[2] = 0;
  data[1] = 8;  // 未定義のop
  CHECK_FALSE(parse(data, 8, command));
}

TEST_CASE("指令の範囲と予約byteを検証する") {
  Command cmd;
  uint8_t data[8] = {1, 4, 0, 0, 0xFC, 0x7C, 0, 0};
  CHECK(parse(data, 8, cmd));
  CHECK(cmd.duty == -900);
  // Duty上限は実行時に変えられるため、範囲の判定はapply側が持つ。
  data[5] = 0x7B;
  CHECK(parse(data, 8, cmd));
  CHECK(cmd.duty == -901);
  Controller limits;
  CHECK_FALSE(limits.apply(cmd, 0));
  data[5] = 0x7C;
  data[6] = 1;
  CHECK_FALSE(parse(data, 8, cmd));
  CHECK_FALSE(parse(nullptr, 8, cmd));
  data[6] = 0;
  data[2] = 1;
  CHECK_FALSE(parse(data, 8, cmd));
}

TEST_CASE("エンコーダは正逆どちらの周回も積算する") {
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

TEST_CASE("HELLOと目標設定を要求しtimeoutをラッチする") {
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

TEST_CASE("方向反転はゼロまで減速し2秒制動してから行う") {
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

TEST_CASE("STOP中に経過した制動時間を逆転開始時にやり直さない") {
  Controller c;
  REQUIRE(c.apply({Op::Hello}, 0));
  REQUIRE(c.apply({Op::Target, 0, 2}, 0));
  REQUIRE(c.apply({Op::Run, 1}, 0));
  c.tick(10); c.tick(20);
  REQUIRE(c.output(0) == 2);
  REQUIRE(c.apply({Op::Stop}, 21));
  for (uint32_t t = 30; t <= 3020; t += 10) {
    c.tick(t);
    // GUIの出力解除・SAFE再送も停止時刻を更新してはいけない。
    if (t % 100 == 0) REQUIRE(c.apply({Op::Safe}, t));
  }
  REQUIRE(c.apply({Op::Target, 0, -2}, 3021));
  REQUIRE(c.apply({Op::Run, 1}, 3021));
  c.tick(3031);
  CHECK(c.output(0) == -1);
}

TEST_CASE("反転待ち0なら正逆切替は減速後に待機せず立ち上がる") {
  Controller c;
  Command parameter;
  parameter.op = Op::ParamSet;
  parameter.param_id = static_cast<uint8_t>(ParamId::ReverseBrakeMs);
  parameter.value = 0.0f;
  REQUIRE(c.apply(parameter, 0));
  parameter.param_id = static_cast<uint8_t>(ParamId::RampStep);
  parameter.value = 130.0f;
  REQUIRE(c.apply(parameter, 0));
  REQUIRE(c.apply({Op::Hello}, 0));
  REQUIRE(c.apply({Op::Target, 0, 600}, 0));
  REQUIRE(c.apply({Op::Run, 1}, 0));
  for (uint32_t t = 10; t <= 50; t += 10) c.tick(t);
  REQUIRE(c.output(0) == 600);

  // R1を保持したまま、右から左へ切り替える。
  REQUIRE(c.apply({Op::Target, 0, -600}, 50));
  for (uint32_t t = 60; t <= 100; t += 10) {
    REQUIRE(c.apply({Op::Run, 1}, t));
    CHECK(c.output(0) >= 0);
  }
  REQUIRE(c.output(0) == 0);
  c.tick(110);
  CHECK(c.output(0) == -130);
  for (uint32_t t = 120; t <= 150; t += 10) c.tick(t);
  REQUIRE(c.output(0) == -600);

  // ボタンを離してSTOPした直後の正方向操作にも追加待機を入れない。
  REQUIRE(c.apply({Op::Stop}, 151));
  REQUIRE(c.output(0) == 0);
  REQUIRE(c.apply({Op::Target, 0, 600}, 152));
  REQUIRE(c.apply({Op::Run, 1}, 152));
  c.tick(162);
  CHECK(c.output(0) == 130);
}

TEST_CASE("停止後すぐに逆転を指令した場合は残りの制動時間だけ待つ") {
  Controller c;
  c.apply({Op::Hello}, 0);
  c.apply({Op::Target, 0, 2}, 0);
  c.apply({Op::Run, 1}, 0);
  c.tick(10); c.tick(20);
  c.apply({Op::Stop}, 21);
  for (uint32_t t = 30; t <= 1020; t += 10) c.tick(t);
  c.apply({Op::Target, 0, -2}, 1021);
  c.apply({Op::Run, 1}, 1021);
  for (uint32_t t = 1031; t < 2021; t += 10) {
    c.apply({Op::Heartbeat}, t);
    CHECK(c.output(0) == 0);
  }
  c.apply({Op::Heartbeat}, 2021);
  CHECK(c.output(0) == -1);
}

TEST_CASE("RUNを繰り返してもDutyランプの更新時刻をリセットしない") {
  Controller c;
  Command interval;
  interval.op = Op::ParamSet;
  interval.param_id = static_cast<uint8_t>(ParamId::RampIntervalMs);
  interval.value = 100.0f;
  REQUIRE(c.apply(interval, 0));
  c.apply({Op::Hello}, 0);
  c.apply({Op::Target, 0, -2}, 0);
  c.apply({Op::Run, 1}, 0);
  for (uint32_t t = 1; t <= 100; ++t) REQUIRE(c.apply({Op::Run, 1}, t));
  CHECK(c.output(0) == -1);
}

TEST_CASE("実行時パラメータでDuty上限とランプを変えられる") {
  Controller c;
  // 既定では上限900、10msごとに1 permille。
  CHECK_FALSE(c.apply({Op::Target, 0, 950}, 0));

  uint8_t frame[8] = {1, 7, static_cast<uint8_t>(ParamId::MaxDuty), 0, 0x44, 0x7A, 0x00, 0x00};
  Command command;
  CHECK(parse(frame, 8, command));  // 1000.0f
  CHECK(command.op == Op::ParamSet);
  CHECK(c.apply(command, 0));
  CHECK(c.parameters().maxDuty() == 1000);
  CHECK(c.apply({Op::Target, 0, 950}, 0));

  // ランプ幅を10 permilleにすると1段で10進む。
  uint8_t step[8] = {1, 7, static_cast<uint8_t>(ParamId::RampStep), 0, 0x41, 0x20, 0x00, 0x00};
  CHECK(parse(step, 8, command));  // 10.0f
  CHECK(c.apply(command, 0));
  CHECK(c.apply({Op::Hello}, 0));
  CHECK(c.apply({Op::Run, 1}, 0));
  c.tick(10);
  CHECK(c.output(0) == 10);
}

TEST_CASE("FWが壊れるパラメータを拒否する") {
  Command command;
  // ランプ間隔0は待ち時間の判定を壊す。
  uint8_t zero[8] = {1, 7, static_cast<uint8_t>(ParamId::RampIntervalMs), 0, 0, 0, 0, 0};
  CHECK_FALSE(parse(zero, 8, command));
  // 未定義のパラメータid。
  uint8_t unknown[8] = {1, 7, PARAM_COUNT, 0, 0x3F, 0x80, 0x00, 0x00};
  CHECK_FALSE(parse(unknown, 8, command));
  // byte 3 は予約。
  uint8_t reserved[8] = {1, 7, 0, 1, 0x44, 0x7A, 0x00, 0x00};
  CHECK_FALSE(parse(reserved, 8, command));
}

TEST_CASE("基板アドレスでCAN IDをずらす") {
  // address 0 は従来と同じID。DIPで選んだぶんだけ 0x100 刻みでずれる。
  CHECK(canId(COMMAND_ID, 0) == 0x310);
  CHECK(canId(STATUS_ID, 0) == 0x311);
  CHECK(canId(COMMAND_ID, 1) == 0x410);
  CHECK(canId(INPUT_ID, 3) == 0x613);
  // 標準IDの範囲に収まる。
  CHECK(canId(INPUT_ID, MAX_ADDRESS) <= 0x7FF);
}

TEST_CASE("PWM周波数から周期tickを求める") {
  Parameters parameters;
  // 既定20kHz、TIM2は8MHz → 400 tick（ARR=399）。
  CHECK(parameters.pwmPeriodTicks() == 400);
  CHECK(parameters.set(static_cast<uint8_t>(ParamId::PwmFrequencyHz), 10000.0f));
  CHECK(parameters.pwmPeriodTicks() == 800);
  // 分解能が落ちすぎる、あるいは高すぎる指定は拒否する。
  CHECK_FALSE(parameters.set(static_cast<uint8_t>(ParamId::PwmFrequencyHz), 100.0f));
  CHECK_FALSE(parameters.set(static_cast<uint8_t>(ParamId::PwmFrequencyHz), 200000.0f));
}

#include "domain/controller.hpp"

namespace dcmd {
bool parse(const uint8_t* data, std::size_t size, Command& out) {
  if (!data || size != 8 || data[0] != 1 || data[1] > 6 || data[3] || data[6] || data[7]) return false;
  Command cmd;
  cmd.op = static_cast<Op>(data[1]);
  cmd.channel = data[2];
  const int32_t raw = (static_cast<uint16_t>(data[4]) << 8) | data[5];
  cmd.duty = static_cast<int16_t>(raw >= 32768 ? raw - 65536 : raw);
  if (cmd.op == Op::Target) {
    if (cmd.channel != 0 || cmd.duty < -MAX_DUTY || cmd.duty > MAX_DUTY) return false;
  } else if (cmd.duty || (cmd.op == Op::Run ? cmd.channel != 1 : cmd.channel != 0)) {
    return false;
  }
  out = cmd;
  return true;
}

void Controller::stop(Mode mode, uint32_t now) {
  mode_ = mode;
  enabled_ = 0;
  configured_ = 0;
  for (uint8_t i = 0; i < 2; ++i) { target_[i] = 0; output_[i] = 0; zero_since_[i] = now; }
}

bool Controller::apply(const Command& cmd, uint32_t now) {
  tick(now);  // 期限切れ直後の受信でも停止を先に確定する。
  switch (cmd.op) {
    case Op::Hello: ready_ = true; return true;
    case Op::Safe: stop(Mode::Safe, now); return true;
    case Op::Stop: stop(Mode::Stop, now); return true;
    case Op::Run:
      if (!ready_ || cmd.channel != 1 ||
          (configured_ & cmd.channel) != cmd.channel) return false;
      enabled_ = cmd.channel;
      mode_ = Mode::Run;
      timed_out_ = false;
      contact_ = now;
      step_ = now;
      return true;
    case Op::Target:
      if (cmd.channel != 0 || cmd.duty < -MAX_DUTY || cmd.duty > MAX_DUTY) return false;
      target_[cmd.channel] = cmd.duty;
      configured_ |= static_cast<uint8_t>(1U << cmd.channel);
      contact_ = now;
      return true;
    case Op::Heartbeat: contact_ = now; return true;
    case Op::InputRead: return true;
  }
  return false;
}

void Controller::tick(uint32_t now) {
  if (mode_ == Mode::Run && now - contact_ > WATCHDOG_MS) {
    stop(Mode::Stop, now);
    ready_ = false;
    timed_out_ = true;
  }
  if (now - step_ < 10) return;
  step_ = now;
  for (uint8_t i = 0; i < 2; ++i) {
    int16_t wanted = mode_ == Mode::Run && (enabled_ & (1U << i)) ? target_[i] : 0;
    const int8_t sign = wanted > 0 ? 1 : wanted < 0 ? -1 : 0;
    if (sign != 0 && direction_[i] != 0 && sign != direction_[i]) {
      if (output_[i] != 0 || now - zero_since_[i] < 2000) wanted = 0;
    }
    const int16_t previous = output_[i];
    if (output_[i] < wanted) ++output_[i];
    else if (output_[i] > wanted) --output_[i];
    if (output_[i] != 0) direction_[i] = output_[i] > 0 ? 1 : -1;
    if (previous != 0 && output_[i] == 0) zero_since_[i] = now;
    // STOPによる即時ゼロも、停止中の時刻から反転待ちを数える。
    if (mode_ != Mode::Run) zero_since_[i] = now;
  }
}
}  // namespace dcmd

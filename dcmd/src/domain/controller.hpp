#pragma once
#include <cstddef>
#include <cstdint>

namespace dcmd {
constexpr uint16_t COMMAND_ID = 0x310;
constexpr uint16_t STATUS_ID = 0x311;
constexpr int16_t MAX_DUTY = 900;  // permille
constexpr uint32_t WATCHDOG_MS = 250;
enum class Op : uint8_t { Hello, Safe, Run, Stop, Target, Heartbeat };
enum class Mode : uint8_t { Safe, Run, Stop };
struct Command { Op op = Op::Stop; uint8_t channel = 0; int16_t duty = 0; };
bool parse(const uint8_t* data, std::size_t size, Command& out);

// HAL非依存の指令検証、通信期限、Dutyランプと方向反転待ち。
class Controller {
 public:
  bool apply(const Command& command, uint32_t now);
  void tick(uint32_t now);
  Mode mode() const { return mode_; }
  int16_t output(uint8_t channel) const { return output_[channel]; }
  uint8_t enabled() const { return enabled_; }
  bool timedOut() const { return timed_out_; }
 private:
  void stop(Mode mode, uint32_t now);
  Mode mode_ = Mode::Safe;
  bool ready_ = false;
  bool timed_out_ = false;
  uint8_t configured_ = 0;
  uint8_t enabled_ = 0;
  int16_t target_[2] = {};
  int16_t output_[2] = {};
  int8_t direction_[2] = {};
  uint32_t zero_since_[2] = {};
  uint32_t contact_ = 0;
  uint32_t step_ = 0;
};
}  // namespace dcmd

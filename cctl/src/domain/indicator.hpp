#pragma once

#include "run_state.hpp"
#include <cstdio>
#include <cstring>

namespace domain {

struct IndicatorState {
  RunMode mode = RunMode::Safe;
  bool host_ready = false;
  bool host_timeout = false;
  bool bus_ready = true;
  bool stop_pending = false;
  uint8_t enabled = 0;
  uint8_t errors[SLOT_COUNT] = {};
};

struct IndicatorFrame {
  char lines[2][17] = {};
  bool alarm = false;
};

inline IndicatorFrame indicatorFrame(const IndicatorState& state) {
  IndicatorFrame frame;
  const char* mode = state.mode == RunMode::Run ? "RUNNING" :
                     state.mode == RunMode::Stop ? "STOPPED" : "SAFE";
  std::snprintf(frame.lines[0], 17, "%s", mode);
  if (state.stop_pending) {
    std::snprintf(frame.lines[0], 17, "STOPPING");
    std::snprintf(frame.lines[1], 17, "CAN SEND FAILED");
    frame.alarm = true;
  } else if (!state.bus_ready) {
    std::snprintf(frame.lines[1], 17, "CAN INIT FAILED");
    frame.alarm = true;
  } else if (state.host_timeout) {
    std::snprintf(frame.lines[1], 17, "HOST TIMEOUT");
    frame.alarm = true;
  } else {
    for (uint8_t slot = 0; slot < SLOT_COUNT; ++slot) {
      if (!state.errors[slot]) continue;
      const char* reason = (state.errors[slot] & error_bit::FEEDBACK_LOST) ? "NO REPLY" :
          (state.errors[slot] & error_bit::OVER_TEMPERATURE) ? "TOO HOT" : "FAULT";
      std::snprintf(frame.lines[1], 17, "MOTOR %u %s", static_cast<unsigned>(slot), reason);
      frame.alarm = true;
      break;
    }
    if (!frame.alarm) {
      if (!state.host_ready) std::snprintf(frame.lines[1], 17, "WAITING FOR HOST");
      else if (state.mode != RunMode::Run || !state.enabled)
        std::snprintf(frame.lines[1], 17, "OUTPUTS OFF");
      else std::snprintf(frame.lines[1], 17, "MOTORS ON: %c%c%c",
          state.enabled & 1 ? '0' : ' ', state.enabled & 2 ? '1' : ' ',
          state.enabled & 4 ? '2' : ' ');
    }
  }
  // 固定幅で上書きし、短くなった行の末尾も消す。
  for (auto& line : frame.lines) {
    const auto length = std::strlen(line);
    for (auto i = length; i < 16; ++i) line[i] = ' ';
  }
  return frame;
}

// 経過時間で音を切り替える。警報は発生時の3音のみで、継続中は再鳴動しない。
class IndicatorTone {
 public:
  // 1=受付、2=拒否、3=長押し成立。警報中は操作音を受け付けない。
  void feedback(uint32_t now, uint8_t cue) {
    if (alarm_ || cue < 1 || cue > 3) return;
    start(now, static_cast<uint8_t>(4 + cue));
    feedback_ = true;
  }
  uint32_t update(uint32_t now, const IndicatorState& state, bool alarm) {
    if (alarm && (!initialized_ || !alarm_)) {
      feedback_ = false;
      start(now, 3);
    } else if (feedback_) {
      if (alarm || now - started_ >= 600) { feedback_ = false; pattern_ = 0; }
    } else if (!initialized_) start(now, 1);
    else if (!alarm && (state.mode != mode_ || state.enabled != enabled_))
      start(now, state.mode == RunMode::Run && state.enabled ? 2 : 4);
    else if (!alarm && alarm_) pattern_ = 0;
    initialized_ = true;
    alarm_ = alarm;
    mode_ = state.mode;
    enabled_ = state.enabled;
    const uint32_t elapsed = now - started_;
    if (elapsed >= 600) pattern_ = 0;
    switch (pattern_) {
      case 1: return elapsed < 80 ? 988 : elapsed < 200 ? 1319 : 0;
      case 2: return elapsed < 80 ? 1319 : elapsed < 160 ? 1760 : 0;
      case 3: return elapsed < 600 && elapsed % 200 < 100 ? 2200 : 0;
      case 4: return elapsed < 120 ? 660 : 0;
      case 5: return elapsed < 80 ? 1568 : 0;
      case 6: return elapsed < 240 && elapsed % 160 < 80 ? 440 : 0;
      case 7: return elapsed < 60 ? 1319 : elapsed < 120 ? 1760 : 0;
      default: return 0;
    }
  }
 private:
  void start(uint32_t now, uint8_t pattern) { started_ = now; pattern_ = pattern; }
  bool feedback_ = false;
  bool initialized_ = false;
  bool alarm_ = false;
  RunMode mode_ = RunMode::Safe;
  uint8_t enabled_ = 0;
  uint8_t pattern_ = 0;
  uint32_t started_ = 0;
};
}  // namespace domain

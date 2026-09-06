#pragma once
#include <cstddef>
#include <cstdint>

namespace dcmd {

// hostが実行時に変更できる基板パラメータ。書き込みなしで実機調整を終えるために、
// 調整対象はここへ集める。RAMだけに保持し、電源投入で既定値へ戻る。
enum class ParamId : uint8_t {
  MaxDuty = 0,        // 絶対Duty上限 [permille]
  RampIntervalMs,     // 出力を1段動かす間隔 [ms]
  RampStep,           // 1段あたりのDuty変化 [permille]
  ReverseBrakeMs,     // 方向反転前にゼロを保つ時間 [ms]
  WatchdogMs,         // 通信期限 [ms]
  PwmFrequencyHz,     // PWMキャリア周波数 [Hz]
  Count,
};

constexpr std::size_t PARAM_COUNT = static_cast<std::size_t>(ParamId::Count);

class Parameters {
 public:
  Parameters() { reset(); }

  void reset() {
    values_[0] = 900.0f;
    values_[1] = 10.0f;
    values_[2] = 1.0f;
    values_[3] = 2000.0f;
    values_[4] = 250.0f;
    values_[5] = 20000.0f;
  }

  bool set(uint8_t id, float value) {
    if (!valid(id, value)) return false;
    values_[id] = value;
    return true;
  }

  float get(ParamId id) const { return values_[static_cast<std::size_t>(id)]; }
  uint32_t ms(ParamId id) const { return static_cast<uint32_t>(get(id)); }
  int16_t maxDuty() const { return static_cast<int16_t>(get(ParamId::MaxDuty)); }
  int16_t rampStep() const { return static_cast<int16_t>(get(ParamId::RampStep)); }

  // TIM2は8MHz（HSI直結、分周なし）。ARRはこの値-1になる。
  static constexpr uint32_t TIMER_CLOCK_HZ = 8000000;
  uint32_t pwmPeriodTicks() const {
    const uint32_t hz = static_cast<uint32_t>(get(ParamId::PwmFrequencyHz));
    return hz == 0 ? 400 : TIMER_CLOCK_HZ / hz;
  }

  // FWが壊れる値だけを弾く。上限やランプの速さは運用側の判断に任せる。
  static bool valid(uint8_t id, float value) {
    if (id >= PARAM_COUNT || !(value == value)) return false;
    switch (static_cast<ParamId>(id)) {
      case ParamId::MaxDuty: return value >= 0.0f && value <= 1000.0f;
      case ParamId::RampIntervalMs: return value >= 1.0f && value <= 10000.0f;
      case ParamId::RampStep: return value >= 1.0f && value <= 1000.0f;
      case ParamId::ReverseBrakeMs: return value >= 0.0f && value <= 60000.0f;
      case ParamId::WatchdogMs: return value >= 1.0f && value <= 60000.0f;
      // タイマは8MHz。分解能が落ちすぎない範囲に限る。
      case ParamId::PwmFrequencyHz: return value >= 500.0f && value <= 100000.0f;
      default: return false;
    }
  }

 private:
  float values_[PARAM_COUNT] = {};
};

}  // namespace dcmd

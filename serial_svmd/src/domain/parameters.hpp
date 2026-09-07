#pragma once
#include <cstddef>
#include <cstdint>

namespace domain {

// hostが実行時に変更できる基板パラメータ。RAM保持で、電源投入で既定値へ戻る。
//
// STS3215-C018の出荷時は1 Mbps。UART側の変更はサーボ本体の保存設定を変更しない。
enum class ServoParamId : uint8_t {
  ServoBaud = 0,       // USART1のボーレート
  ServoTimeoutMs,      // サーボ応答の待ち時間 [ms]
  WaitForWriteStatus,  // 書き込み命令のステータス応答を待つか（0/1）
  WatchdogMs,          // 通信期限 [ms]
  Count,
};

constexpr std::size_t SERVO_PARAM_COUNT = static_cast<std::size_t>(ServoParamId::Count);

class ServoParameters {
 public:
  ServoParameters() { reset(); }

  void reset() {
    values_[0] = 1000000.0f;
    values_[1] = 20.0f;
    values_[2] = 0.0f;
    values_[3] = 250.0f;
  }

  bool set(uint8_t id, float value) {
    if (!valid(id, value)) return false;
    values_[id] = value;
    return true;
  }

  float get(ServoParamId id) const { return values_[static_cast<std::size_t>(id)]; }
  uint32_t baud() const { return static_cast<uint32_t>(get(ServoParamId::ServoBaud)); }
  uint32_t timeoutMs() const { return static_cast<uint32_t>(get(ServoParamId::ServoTimeoutMs)); }
  bool waitForWriteStatus() const { return get(ServoParamId::WaitForWriteStatus) != 0.0f; }
  uint32_t watchdogMs() const { return static_cast<uint32_t>(get(ServoParamId::WatchdogMs)); }

  static bool valid(uint8_t id, float value) {
    if (id >= SERVO_PARAM_COUNT || !(value == value)) return false;
    switch (static_cast<ServoParamId>(id)) {
      case ServoParamId::ServoBaud: return value == 1000000 || value == 500000 || value == 250000 || value == 128000 || value == 115200 || value == 76800 || value == 57600 || value == 38400;
      case ServoParamId::ServoTimeoutMs: return value >= 1.0f && value <= 10000.0f;
      case ServoParamId::WaitForWriteStatus: return value == 0.0f || value == 1.0f;
      case ServoParamId::WatchdogMs: return value >= 1.0f && value <= 60000.0f;
      default: return false;
    }
  }

 private:
  float values_[SERVO_PARAM_COUNT] = {};
};

}  // namespace domain

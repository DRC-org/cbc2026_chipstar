#pragma once
#include <cstddef>
#include <cstdint>

namespace domain::servo_can {

// hostが実行時に変更できる基板パラメータ。サーボごとに使えるパルス幅が違うため、
// 書き込みなしで合わせられるようにしてある。RAM保持で、電源投入で既定値へ戻る。
enum class ParamId : uint8_t {
    MinPulseUs = 0,
    MaxPulseUs,
    WatchdogMs,
    Count,
};

constexpr std::size_t PARAM_COUNT = static_cast<std::size_t>(ParamId::Count);

class Parameters {
 public:
    Parameters() { reset(); }

    void reset() {
        values_[0] = 500.0f;
        values_[1] = 2500.0f;
        values_[2] = 250.0f;
    }

    bool set(uint8_t id, float value) {
        if (!valid(id, value)) return false;
        values_[id] = value;
        return true;
    }

    float get(ParamId id) const { return values_[static_cast<std::size_t>(id)]; }
    uint16_t minPulseUs() const { return static_cast<uint16_t>(get(ParamId::MinPulseUs)); }
    uint16_t maxPulseUs() const { return static_cast<uint16_t>(get(ParamId::MaxPulseUs)); }
    uint32_t watchdogMs() const { return static_cast<uint32_t>(get(ParamId::WatchdogMs)); }

    // サーボが焼き付く値かどうかは運用側の判断。ここではFWが壊れる値だけ弾く。
    static bool valid(uint8_t id, float value) {
        if (id >= PARAM_COUNT || !(value == value)) return false;
        switch (static_cast<ParamId>(id)) {
            case ParamId::MinPulseUs:
            case ParamId::MaxPulseUs:
                return value >= 100.0f && value <= 20000.0f;
            case ParamId::WatchdogMs:
                return value >= 1.0f && value <= 60000.0f;
            default:
                return false;
        }
    }

 private:
    float values_[PARAM_COUNT] = {};
};

}  // namespace domain::servo_can

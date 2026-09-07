#pragma once

#include "domain/can_frame.hpp"
#include "domain/dm_codec.hpp"
#include "domain/el05_codec.hpp"

namespace domain {
// 起動時に一度だけ識別情報を読む。Enable、モード変更、設定書込みは送らない。
class MotorDiscovery {
 public:
    bool next(uint32_t now, bool stopped, uint8_t host_id, CanFrame& frame) {
        if (!stopped) {
            cancelled_ |= cursor_ < COUNT;
            cursor_ = COUNT;
            return false;
        }
        if (cursor_ >= COUNT || now - last_ < 20) return false;
        last_ = now;
        frame = {};
        frame.length = 8;
        if (cursor_ < 256) {
            frame.extended = true;
            frame.id = el05::buildCanId(0, host_id, static_cast<uint8_t>(cursor_));
        } else {
            frame.id = dm::CONFIG_ID;
            dm::encodeConfig(static_cast<uint16_t>(cursor_ - 256), dm::CONFIG_READ,
                             dm::reg::ESC_ID, 0, frame.data);
        }
        ++cursor_;
        return true;
    }
    bool done() const { return cursor_ >= COUNT; }
    bool cancelled() const { return cancelled_; }
 private:
    static constexpr uint16_t COUNT = 256 + 2048;
    uint16_t cursor_ = 0;
    uint32_t last_ = 0;
    bool cancelled_ = false;
};
}  // namespace domain

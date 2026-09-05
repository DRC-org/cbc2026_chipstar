#pragma once

#include <cstdint>

namespace domain {

// CAN の 1 フレーム。HAL の型に依存せずに受信内容を扱うための表現。
struct CanFrame {
    uint32_t id = 0;
    bool extended = false;
    uint8_t length = 0;
    uint8_t data[8] = {};
};

}  // namespace domain

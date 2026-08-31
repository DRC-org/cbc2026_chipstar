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

// 受信フレームがどの軸のフィードバックかを表す。
enum class Axis {
    None,
    R,
    Theta,
    Z,
};

// 受信フレームの宛先軸を判定する。
// r(EL05) は拡張ID なので標準IDと空間が別。θ(C620) と z(DM) は標準IDで区別する。
// θ と z に同じ ID が設定されていた場合は θ を優先する。
Axis classifyFeedback(const CanFrame& frame, uint16_t theta_feedback_id,
                      uint16_t z_feedback_id);

}  // namespace domain

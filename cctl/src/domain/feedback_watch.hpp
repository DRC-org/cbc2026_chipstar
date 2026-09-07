#pragma once
#include "domain/run_state.hpp"

#include <cstdint>

namespace domain {

// モータからの応答が途絶えたslotを見つける。
//
// hostとの通信が生きていても、モータ側のCANが抜けたことは分からない。
// 実測値が凍ったまま位置ループが差を見続け、電流上限で押し続けるため、
// 一定時間応答がないslotは出力を落とす。
//
// 指令を送っていないslotは応答も返らないので、無効な間は時計を進めない。
// そうしないと、起動から時間が経ってから有効化したslotが、有効化した
// その場で途絶と判定されて落ちる。
class FeedbackWatch {
 public:
    void reset(uint32_t now) {
        for (auto& stamp : last_) stamp = now;
        stale_ = 0;
        reported_ = 0;
    }

    void markSeen(uint8_t slot, uint32_t now) {
        if (slot < SLOT_COUNT) {
            last_[slot] = now;
            stale_ = static_cast<uint8_t>(stale_ & ~(1U << slot));
            reported_ = static_cast<uint8_t>(reported_ & ~(1U << slot));
        }
    }

    // active は指令を送っているslotのbit mask。
    // 戻り値は出力を落とすべきslotのbit mask。
    uint8_t update(uint32_t now, uint8_t active, uint32_t limit_ms) {
        uint8_t dropped = 0;
        for (uint8_t slot = 0; slot < SLOT_COUNT; ++slot) {
            const uint8_t bit = static_cast<uint8_t>(1U << slot);
            if ((active & bit) == 0) {
                // 出力解除で停止原因を消さない。実際の応答が戻ればmarkSeenで解除する。
                reported_ = static_cast<uint8_t>(reported_ & ~bit);
                last_[slot] = now;
                continue;
            }
            if (now - last_[slot] <= limit_ms) {
                continue;
            }
            if ((reported_ & bit) == 0) dropped = static_cast<uint8_t>(dropped | bit);
            stale_ = static_cast<uint8_t>(stale_ | bit);
            reported_ = static_cast<uint8_t>(reported_ | bit);
        }
        return dropped;
    }

    uint8_t stale() const { return stale_; }

 private:
    uint32_t last_[SLOT_COUNT] = {};
    uint8_t stale_ = 0;
    uint8_t reported_ = 0;
};

}  // namespace domain

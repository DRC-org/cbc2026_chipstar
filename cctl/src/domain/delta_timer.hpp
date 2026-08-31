#pragma once

#include <cstdint>

namespace domain {

// ミリ秒 tick の系列から、前回呼び出しからの経過秒を求める。
// 初回は基準が無いので 0 を返す。差は符号なし演算なので tick の
// 32 ビット巻き戻りをそのまま跨げる。
class DeltaTimer {
public:
    float update(uint32_t now_ms);
    void reset();

private:
    bool has_previous_ = false;
    uint32_t previous_ms_ = 0;
};

}  // namespace domain

#pragma once

#include <cstddef>
#include <cstdint>

namespace domain {

// host が送るスティック入力のうち、手動操作に使う左スティックの 2 軸。
struct StickCommand {
    int8_t lx_percent;
    int8_t ly_percent;
};

// 1 周期あたりの目標値の変化量。
struct ManualDelta {
    float r_mm;
    float theta_deg;
};

// "LX+000 LY+000 ..." 形式の行を解釈する。
// 解釈できなければ false を返し、out は書き換えない。
bool parseStickCommand(const char* line, std::size_t length, StickCommand& out);

// 不感帯を適用し、その外側を [0, 1] へ引き伸ばす。
float applyDeadzone(float value, float deadzone);

// スティック入力と経過時間から、各軸の移動量を求める。
ManualDelta computeManualDelta(const StickCommand& command, float interval_s, float deadzone,
                               float r_speed_mm_s, float theta_speed_deg_s);

}  // namespace domain

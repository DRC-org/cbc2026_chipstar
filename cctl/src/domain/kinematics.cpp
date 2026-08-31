#include "domain/kinematics.hpp"

namespace domain::kinematics {
namespace {
constexpr float TWO_PI = 6.28318530718f;

// 0 除算を避ける。実測前の設定漏れで NaN が伝播すると原因が追いにくい。
bool usable(float divisor) { return divisor > 0.0f || divisor < 0.0f; }
}  // namespace

float mmToRad(float mm, float mm_per_rev, float sign) {
    if (!usable(mm_per_rev)) {
        return 0.0f;
    }
    return sign * mm / mm_per_rev * TWO_PI;
}

float radToMm(float rad, float mm_per_rev, float sign) {
    return sign * rad / TWO_PI * mm_per_rev;
}

float outputDegToMotorDeg(float output_deg, float gear_ratio, float sign) {
    return sign * output_deg * gear_ratio;
}

float motorDegToOutputDeg(float motor_deg, float gear_ratio, float sign) {
    if (!usable(gear_ratio)) {
        return 0.0f;
    }
    return sign * motor_deg / gear_ratio;
}

}  // namespace domain::kinematics

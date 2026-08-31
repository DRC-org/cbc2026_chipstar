#pragma once

namespace domain {

// 機構の換算。実測待ちの定数を引数で受けるので、値を変えても式は変わらない。
namespace kinematics {

// 直動 [mm] → 出力軸回転 [rad]。1 回転あたり mm_per_rev だけ進む機構に共通。
float mmToRad(float mm, float mm_per_rev, float sign);

// 出力軸回転 [rad] → 直動 [mm]。mmToRad の逆変換。
float radToMm(float rad, float mm_per_rev, float sign);

// 出力軸角 [deg] → モータ軸角 [deg]。
float outputDegToMotorDeg(float output_deg, float gear_ratio, float sign);

// モータ軸角 [deg] → 出力軸角 [deg]。
float motorDegToOutputDeg(float motor_deg, float gear_ratio, float sign);

}  // namespace kinematics
}  // namespace domain

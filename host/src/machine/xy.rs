//! θ=0の正面を+Y、右を+Xとする固定座標での手動速度操作。
use crate::input::ControllerState;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanarMode {
    Rtheta,
    #[default]
    Xy,
}
impl PlanarMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Rtheta => "r・θ移動",
            Self::Xy => "XY移動",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Rtheta => "rtheta",
            Self::Xy => "xy",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct XyProfile {
    pub speed_mm_per_second: f32,
    /// r=0のときの旋回中心からEEまでの水平距離。
    pub radius_offset_mm: f32,
}
impl Default for XyProfile {
    fn default() -> Self {
        Self {
            speed_mm_per_second: 100.0,
            radius_offset_mm: 0.0,
        }
    }
}
impl XyProfile {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.speed_mm_per_second.is_finite()
                && (0.0..=10000.0).contains(&self.speed_mm_per_second)
                && self.speed_mm_per_second > 0.0,
            "XY移動速度は0より大きく10000 mm/s以下の値にしてください"
        );
        anyhow::ensure!(
            self.radius_offset_mm.is_finite() && (0.0..=10000.0).contains(&self.radius_offset_mm),
            "r=0の旋回半径は0〜10000 mmで指定してください"
        );
        Ok(())
    }
}

/// gilrsのスティックは上・右が正。斜めでも最大速度を超えない。
pub fn stick(input: &ControllerState) -> [f32; 2] {
    let [x, y] = [input.axes[0], input.axes[1]];
    if !x.is_finite() || !y.is_finite() {
        return [0.0; 2];
    }
    let length = x.hypot(y);
    // 通常操縦の開始・モード切替と同じ中立範囲に揃える。
    if x.abs().max(y.abs()) < 0.1 {
        [0.0; 2]
    } else {
        [x / length.max(1.0), y / length.max(1.0)]
    }
}

/// x=-ρsinθ, y=ρcosθ の速度逆変換。θ速度はdeg/s。
pub(super) fn joint_velocity([x, y]: [f32; 2], radius: f32, theta_deg: f32) -> [f32; 2] {
    let (sin, cos) = theta_deg.to_radians().sin_cos();
    [
        -x * sin + y * cos,
        ((-x * cos - y * sin) / radius).to_degrees(),
    ]
}

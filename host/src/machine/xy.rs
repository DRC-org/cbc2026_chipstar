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
    /// フィールド基準のY可動域。未設定の側は制限しない。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y_min_mm: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y_max_mm: Option<f32>,
}
impl Default for XyProfile {
    fn default() -> Self {
        Self {
            speed_mm_per_second: 100.0,
            radius_offset_mm: 0.0,
            y_min_mm: None,
            y_max_mm: None,
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
        anyhow::ensure!(
            self.y_min_mm
                .into_iter()
                .chain(self.y_max_mm)
                .all(|value| value.is_finite() && (-10000.0..=10000.0).contains(&value)),
            "Y下限・上限は−10000〜10000 mmで指定してください"
        );
        if let (Some(min), Some(max)) = (self.y_min_mm, self.y_max_mm) {
            anyhow::ensure!(min < max, "Y下限はY上限より小さくしてください");
        }
        Ok(())
    }

    /// 遅延0.2秒とXYの加減速度を考慮し、範囲外へ向かう速度だけを抑える。
    pub(super) fn y_velocity_factor(&self, y: f32, velocity: f32, acceleration: f32) -> f32 {
        let distance = if velocity > 0.0 {
            self.y_max_mm.map(|max| max - y)
        } else if velocity < 0.0 {
            self.y_min_mm.map(|min| y - min)
        } else {
            None
        };
        let Some(distance) = distance else {
            return 1.0;
        };
        if !y.is_finite() || !velocity.is_finite() || acceleration <= 0.0 {
            return 0.0;
        }
        let distance = f64::from(distance.max(0.0));
        let cap = if acceleration.is_finite() {
            let acceleration = f64::from(acceleration);
            let delay_velocity = acceleration * 0.2;
            // 有理化して、境界の近くでも差し引きによる精度低下を避ける。
            2.0 * acceleration * distance
                / ((delay_velocity * delay_velocity + 2.0 * acceleration * distance).sqrt()
                    + delay_velocity)
        } else {
            distance / 0.2
        };
        (cap / f64::from(velocity.abs())).clamp(0.0, 1.0) as f32
    }
}

pub fn position(radius: f32, theta_deg: f32) -> [f32; 2] {
    let (sin, cos) = theta_deg.to_radians().sin_cos();
    [-radius * sin, radius * cos]
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

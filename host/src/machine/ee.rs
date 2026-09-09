//! EEの役割を既存のサーボプロファイルへ対応させる。
use super::MachineProfile;
use crate::{
    diagnostics::individual::Target,
    protocol::{serial_svmd, svmd},
};
use anyhow::{Result, ensure};
pub const ROLES: [(&str, &str); 5] = [
    ("ee_rotation", "EE全体回転"),
    ("ee_fold", "畳み"),
    ("ee_grip_1", "把持1"),
    ("ee_grip_2", "把持2"),
    ("ee_grip_3", "把持3"),
];
#[derive(Clone)]
pub struct Axis {
    pub name: String,
    pub label: &'static str,
    pub target: Target,
    pub min: f32,
    pub max: f32,
    pub initial: f32,
    pub enabled: bool,
    pub input_axis: Option<usize>,
    pub sign: f32,
    pub speed: f32,
    pub acceleration: u8,
    /// θ=0で先端がフィールド0°になるサーボカウント（取付オフセット）。
    pub zero0_count: f32,
    /// サーボの count/deg（符号込み）。フィールド角写像とθ補正に共用。
    pub counts_per_deg: f32,
}
impl Axis {
    fn sts_speed(&self) -> u16 {
        if self.speed == 0.0 {
            0
        } else {
            self.speed.round().clamp(1.0, 1000.0) as u16
        }
    }

    pub fn pad_value(&self, input: &crate::input::ControllerState) -> f32 {
        if let Some(index) = self.input_axis {
            return input.axes[index];
        }
        let pair = |positive: usize, negative: usize| {
            f32::from(u8::from(input.buttons[positive] != 0))
                - f32::from(u8::from(input.buttons[negative] != 0))
        };
        match self.name.as_str() {
            "ee_rotation" => input.axes[2],
            "ee_fold" => pair(11, 12),
            "ee_grip_1" | "ee_grip_2" | "ee_grip_3" => pair(14, 13),
            _ => 0.0,
        }
    }

    pub fn unit(&self) -> &'static str {
        if matches!(self.target, Target::Pwm(_)) {
            "µs"
        } else {
            "count（サーボ軸）"
        }
    }

    /// フィールド基準角[deg]と現在θ[deg]から、θ回転を打ち消した先端回転の
    /// STS位置カウントを求める。count = zero0 + cpds×(field − θ)。
    pub fn rotation_count(&self, field_deg: f32, theta_deg: f32) -> i16 {
        (self.zero0_count + self.counts_per_deg * (field_deg - theta_deg))
            .round()
            .clamp(-28672.0, 28672.0) as i16
    }

    /// 先端回転のSTS指令行。initialのときだけトルク有効化とRUNも添える。
    pub fn rotation_command(&self, count: i16, initial: bool) -> Vec<String> {
        let Target::Sts(id) = self.target else {
            return Vec::new();
        };
        let target = serial_svmd::Command::Target {
            id,
            position: count,
            speed: self.sts_speed(),
            acceleration: self.acceleration,
        }
        .to_cctl_line();
        if initial {
            vec![
                target,
                serial_svmd::Command::Enable { id, enabled: true }.to_cctl_line(),
                serial_svmd::Command::Run.to_cctl_line(),
            ]
        } else {
            vec![target]
        }
    }
    pub fn commands(&self, value: f32) -> Result<Vec<String>> {
        ensure!(self.enabled, "{}は出力未許可です", self.label);
        ensure!(
            value.is_finite() && value.fract() == 0.0 && (self.min..=self.max).contains(&value),
            "{}の指令範囲は{}〜{}です",
            self.label,
            self.min,
            self.max
        );
        Ok(match self.target {
            Target::Pwm(channel) => vec![
                svmd::Command::Set {
                    channel,
                    pulse_us: value as u16,
                }
                .to_cctl_line(),
                svmd::Command::Enable {
                    channel,
                    enabled: true,
                }
                .to_cctl_line(),
            ],
            Target::Sts(id) => vec![
                serial_svmd::Command::Target {
                    id,
                    position: value as i16,
                    // 操作速度とサーボ内部速度を同じ値にし、二重の速度設定を作らない。
                    speed: self.sts_speed(),
                    acceleration: self.acceleration,
                }
                .to_cctl_line(),
                serial_svmd::Command::Enable { id, enabled: true }.to_cctl_line(),
                serial_svmd::Command::Run.to_cctl_line(),
            ],
            _ => unreachable!(),
        })
    }
}
pub fn axes(profile: &MachineProfile) -> Vec<Axis> {
    ROLES
        .iter()
        .filter_map(|(name, label)| {
            if let Some(s) = profile.pwm_servos.iter().find(|s| s.name == *name) {
                Some(Axis {
                    name: s.name.clone(),
                    label,
                    target: Target::Pwm(s.channel),
                    min: s.minimum_us.into(),
                    max: s.maximum_us.into(),
                    initial: s.initial_us.into(),
                    enabled: s.enabled,
                    input_axis: s.input_axis,
                    sign: s.input_sign,
                    speed: s.speed_us_per_second,
                    acceleration: 0,
                    zero0_count: 0.0,
                    counts_per_deg: 0.0,
                })
            } else {
                profile
                    .serial_svmd
                    .as_ref()?
                    .servos
                    .iter()
                    .find(|s| s.name == *name)
                    .map(|s| Axis {
                        name: s.name.clone(),
                        label,
                        target: Target::Sts(s.id),
                        min: 0.0,
                        max: 4095.0,
                        initial: s.zero0_count,
                        enabled: s.enabled,
                        input_axis: s.input_axis,
                        sign: s.input_sign,
                        speed: s.speed_position_per_second,
                        acceleration: s.acceleration,
                        zero0_count: s.zero0_count,
                        counts_per_deg: s.counts_per_deg,
                    })
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard_pad_axes_and_custom_override_are_independent() {
        let mut axis = Axis {
            name: "ee_rotation".into(),
            label: "test",
            target: Target::Sts(1),
            min: 0.0,
            max: 4095.0,
            initial: 2048.0,
            enabled: true,
            input_axis: None,
            sign: 1.0,
            speed: 100.0,
            acceleration: 0,
            zero0_count: 2048.0,
            counts_per_deg: 11.377778,
        };
        let mut input = crate::input::ControllerState::default();
        input.axes[2] = -0.75;
        input.buttons[11] = 1;
        input.buttons[14] = 1;
        assert_eq!(axis.pad_value(&input), -0.75);
        assert!(axis.commands(2048.0).unwrap()[0].ends_with("08000064"));
        axis.name = "ee_fold".into();
        assert_eq!(axis.pad_value(&input), 1.0);
        input.buttons[12] = 1;
        assert_eq!(axis.pad_value(&input), 0.0);
        axis.name = "ee_grip_1".into();
        assert_eq!(axis.pad_value(&input), 1.0);
        input.buttons[13] = 1;
        assert_eq!(axis.pad_value(&input), 0.0);
        axis.input_axis = Some(2);
        assert_eq!(axis.pad_value(&input), -0.75);
        axis.speed = 0.0;
        assert!(axis.commands(2048.0).unwrap()[0].ends_with("08000000"));
    }

    #[test]
    fn rotation_count_maps_field_angle_and_cancels_theta() {
        let axis = Axis {
            name: "ee_rotation".into(),
            label: "test",
            target: Target::Sts(1),
            min: 0.0,
            max: 4095.0,
            initial: 1024.0,
            enabled: true,
            input_axis: None,
            sign: 1.0,
            speed: 1000.0,
            acceleration: 10,
            zero0_count: 1024.0,
            counts_per_deg: 10.0,
        };
        // θ=0では field角×cpds のオフセット。
        assert_eq!(axis.rotation_count(0.0, 0.0), 1024);
        assert_eq!(axis.rotation_count(180.0, 0.0), 2824);
        // θが+10degなら cpds×(−θ)= −100 ずれてフィールド基準を保つ。
        assert_eq!(axis.rotation_count(0.0, 10.0), 924);
        assert_eq!(axis.rotation_count(180.0, -3000.0), 28672);
        assert_eq!(axis.rotation_count(0.0, 3000.0), -28672);
    }

    #[test]
    fn three_to_one_rotation_uses_multi_turn_target() {
        let axis = Axis {
            name: "ee_rotation".into(),
            label: "test",
            target: Target::Sts(1),
            min: 0.0,
            max: 4095.0,
            initial: 1024.0,
            enabled: true,
            input_axis: None,
            sign: 1.0,
            speed: 0.0,
            acceleration: 50,
            zero0_count: 1024.0,
            counts_per_deg: 4096.0 * 3.0 / 360.0,
        };
        assert_eq!(axis.rotation_count(0.0, -82.5), 3840);
        assert_eq!(axis.rotation_count(180.0, -82.5), 9984);
        assert!(axis.rotation_command(9984, false)[0].contains("2700"));
    }
}

/// 2点で取付オフセットと符号付き減速比を求める。θを動かした場合も補正する。
pub fn calibrate(
    zero: crate::application::sts::TeachPoint,
    half: crate::application::sts::TeachPoint,
) -> Result<(f32, f32)> {
    let angle = (half.field_deg - zero.field_deg) - (half.theta_deg - zero.theta_deg);
    ensure!(
        angle.is_finite() && angle.abs() > 0.001,
        "2点の相対角が同じため較正できません"
    );
    let ratio = (half.count - zero.count) / angle;
    let offset = zero.count - ratio * (zero.field_deg - zero.theta_deg);
    ensure!(
        ratio.is_finite() && ratio.abs() > 0.001 && offset.is_finite(),
        "異なる向きで2点を取り込んでください"
    );
    Ok((offset, ratio))
}

#[cfg(test)]
mod calibration_tests {
    use super::*;
    use crate::application::sts::TeachPoint as P;
    #[test]
    fn two_point_calibration_supports_reduction_reverse_and_moving_theta() {
        let (offset, ratio) = calibrate(
            P {
                field_deg: 0.0,
                count: 3000.0,
                theta_deg: 10.0,
            },
            P {
                field_deg: 180.0,
                count: 1400.0,
                theta_deg: 30.0,
            },
        )
        .unwrap();
        assert_eq!((offset, ratio), (2900.0, -10.0));
        let (zero, geared) = calibrate(
            P {
                field_deg: 0.0,
                count: 100.0,
                theta_deg: 0.0,
            },
            P {
                field_deg: 90.0,
                count: 3172.0,
                theta_deg: 0.0,
            },
        )
        .unwrap();
        assert_eq!(zero, 100.0);
        assert!((geared - 4096.0 / 120.0).abs() < 0.001);
        assert_eq!(offset + ratio * (180.0 - 30.0), 1400.0);
        assert!(
            calibrate(
                P {
                    field_deg: 0.0,
                    count: 1.0,
                    theta_deg: 0.0
                },
                P {
                    field_deg: 180.0,
                    count: 2.0,
                    theta_deg: 180.0
                }
            )
            .is_err()
        );
        assert!(
            calibrate(
                P {
                    field_deg: 0.0,
                    count: 1.0,
                    theta_deg: 0.0
                },
                P {
                    field_deg: 0.0,
                    count: 1.0,
                    theta_deg: 0.0
                }
            )
            .is_err()
        );
    }
}

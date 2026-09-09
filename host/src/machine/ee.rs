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
}
impl Axis {
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
                    position: value as u16,
                    // 操作速度とサーボ内部速度を同じ値にし、二重の速度設定を作らない。
                    speed: self.speed.round().clamp(1.0, 1000.0) as u16,
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
                        min: s.minimum_position.into(),
                        max: s.maximum_position.into(),
                        initial: s.initial_position.into(),
                        enabled: s.enabled,
                        input_axis: s.input_axis,
                        sign: s.input_sign,
                        speed: s.speed_position_per_second,
                        acceleration: s.acceleration,
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
    }
}

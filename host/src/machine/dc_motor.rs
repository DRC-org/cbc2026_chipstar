//! DCモータの入力割当とDuty制限。
use crate::protocol::dcmd::line;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct MotorProfile {
    pub name: String,
    pub channel: u8,
    pub input_axis: usize,
    pub input_sign: f32,
    pub maximum_duty: u16,
}

pub fn validate(motors: &[MotorProfile]) -> Result<()> {
    let mut mask = 0u8;
    for motor in motors {
        if motor.channel != 0
            || mask & (1 << motor.channel) != 0
            || motor.input_axis >= 6
            || motor.input_sign.abs() != 1.0
            || motor.maximum_duty > 1000
            || motor.name.is_empty()
        {
            bail!("DCMDのchannel、入力、Duty上限が不正です");
        }
        mask |= 1 << motor.channel;
    }
    Ok(())
}

pub fn targets(motors: &[MotorProfile], axes: &[f32; 6]) -> Vec<String> {
    motors
        .iter()
        .map(|motor| {
            let input = axes[motor.input_axis];
            let input = if !input.is_finite() || input.abs() < 0.1 {
                0.0
            } else {
                input.clamp(-1.0, 1.0)
            };
            line(
                4,
                motor.channel,
                (input * motor.input_sign * f32::from(motor.maximum_duty)).round() as i16,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_limits_input_and_rejects_duplicate_channels() {
        let mut profile =
            crate::machine::MachineProfile::parse(include_str!("../../config/dcmd.toml")).unwrap();
        assert_eq!(
            targets(&profile.dc_motors, &[0.0, -2.0, 0.0, 0.05, 0.0, 0.0]),
            vec![line(4, 0, -100)]
        );
        profile.dc_motors[0].channel = 1;
        assert!(validate(&profile.dc_motors).is_err());
    }
}

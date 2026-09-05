//! 機体固有の軸構成と、コントローラ入力からデバイス指令への変換。
//!
//! FWはslotのネイティブ単位だけを扱い、軸名、機械換算、入力割当はこの層に閉じる。

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::controller::ControllerState;
use crate::svmd;

const EMBEDDED_PROFILE: &str = include_str!("../config/rtheta.toml");
const MAX_SLOTS: usize = 3;
const MAX_INPUT_INTERVAL_S: f32 = 0.1;
const STICK_DEADZONE: f32 = 0.1;
const PWM_CHANNEL_COUNT: usize = 4;
const PWM_MIN_US: u16 = 500;
const PWM_MAX_US: u16 = 2500;

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct AxisProfile {
    pub name: String,
    pub unit: String,
    pub slot: u8,
    pub input_axis: Option<usize>,
    #[serde(default = "one")]
    pub input_sign: f32,
    pub speed_per_second: f32,
    pub native_per_unit: f32,
    pub minimum: f32,
    pub maximum: f32,
    pub initial: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct PwmServoProfile {
    pub name: String,
    pub channel: u8,
    pub input_axis: Option<usize>,
    #[serde(default = "one")]
    pub input_sign: f32,
    pub speed_us_per_second: f32,
    pub minimum_us: u16,
    pub maximum_us: u16,
    pub initial_us: u16,
    #[serde(default)]
    pub enabled: bool,
}

fn one() -> f32 {
    1.0
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct MachineProfile {
    pub protocol_version: u8,
    #[serde(default)]
    pub axes: Vec<AxisProfile>,
    #[serde(default)]
    pub pwm_servos: Vec<PwmServoProfile>,
}

impl MachineProfile {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let source = match path {
            Some(path) => fs::read_to_string(path)
                .with_context(|| format!("機体プロファイルを読めません: {}", path.display()))?,
            None => EMBEDDED_PROFILE.to_owned(),
        };
        Self::parse(&source)
    }

    pub fn parse(source: &str) -> Result<Self> {
        let profile: Self = toml::from_str(source).context("機体プロファイルの形式が不正です")?;
        profile.validate()?;
        Ok(profile)
    }

    fn validate(&self) -> Result<()> {
        if self.protocol_version != 1 {
            bail!(
                "未対応のプロトコルバージョンです: {}",
                self.protocol_version
            );
        }
        if self.axes.len() > MAX_SLOTS {
            bail!("軸数は0..={MAX_SLOTS}で指定してください");
        }
        if self.pwm_servos.len() > PWM_CHANNEL_COUNT {
            bail!("PWMサーボ数は0..={PWM_CHANNEL_COUNT}で指定してください");
        }
        if self.axes.is_empty() && self.pwm_servos.is_empty() {
            bail!("軸またはPWMサーボを1つ以上指定してください");
        }

        let mut slots = HashSet::new();
        let mut names = HashSet::new();
        for axis in &self.axes {
            if axis.slot as usize >= MAX_SLOTS || !slots.insert(axis.slot) {
                bail!("slotは0..2で重複なく指定してください: {}", axis.slot);
            }
            if axis.name.is_empty() || !names.insert(axis.name.as_str()) {
                bail!("軸名は空でなく重複しない値にしてください: {}", axis.name);
            }
            if axis.input_axis.is_some_and(|index| index >= 6) {
                bail!("input_axisは0..5で指定してください: {}", axis.name);
            }
            let numbers = [
                axis.input_sign,
                axis.speed_per_second,
                axis.native_per_unit,
                axis.minimum,
                axis.maximum,
                axis.initial,
            ];
            if numbers.iter().any(|value| !value.is_finite()) {
                bail!("軸設定に有限でない値があります: {}", axis.name);
            }
            if axis.input_sign.abs() != 1.0
                || axis.speed_per_second < 0.0
                || axis.native_per_unit == 0.0
                || axis.minimum >= axis.maximum
                || !(axis.minimum..=axis.maximum).contains(&axis.initial)
            {
                bail!("軸設定の範囲が不正です: {}", axis.name);
            }
        }

        let mut channels = HashSet::new();
        for servo in &self.pwm_servos {
            if servo.channel as usize >= PWM_CHANNEL_COUNT || !channels.insert(servo.channel) {
                bail!(
                    "PWM channelは0..3で重複なく指定してください: {}",
                    servo.channel
                );
            }
            if servo.name.is_empty() || !names.insert(servo.name.as_str()) {
                bail!(
                    "デバイス名は空でなく重複しない値にしてください: {}",
                    servo.name
                );
            }
            if servo.input_axis.is_some_and(|index| index >= 6) {
                bail!("input_axisは0..5で指定してください: {}", servo.name);
            }
            if !servo.input_sign.is_finite()
                || !servo.speed_us_per_second.is_finite()
                || servo.input_sign.abs() != 1.0
                || servo.speed_us_per_second < 0.0
                || servo.minimum_us < PWM_MIN_US
                || servo.maximum_us > PWM_MAX_US
                || servo.minimum_us >= servo.maximum_us
                || !(servo.minimum_us..=servo.maximum_us).contains(&servo.initial_us)
            {
                bail!("PWMサーボ設定の範囲が不正です: {}", servo.name);
            }
        }
        Ok(())
    }

    pub fn requires_can_bus_2(&self) -> bool {
        !self.pwm_servos.is_empty()
    }
}

pub struct MachineController {
    profile: MachineProfile,
    targets: Vec<f32>,
    pwm_targets_us: Vec<f32>,
}

impl MachineController {
    pub fn new(profile: MachineProfile) -> Self {
        let targets = profile.axes.iter().map(|axis| axis.initial).collect();
        let pwm_targets_us = profile
            .pwm_servos
            .iter()
            .map(|servo| f32::from(servo.initial_us))
            .collect();
        Self {
            profile,
            targets,
            pwm_targets_us,
        }
    }

    pub fn hello_line(&self) -> String {
        format!("HELLO {}", self.profile.protocol_version)
    }

    pub fn update(&mut self, input: &ControllerState, elapsed_s: f32) -> Vec<String> {
        let dt = elapsed_s.clamp(0.0, MAX_INPUT_INTERVAL_S);
        let mut lines: Vec<String> = self
            .profile
            .axes
            .iter()
            .zip(&mut self.targets)
            .map(|(axis, target)| {
                if let Some(index) = axis.input_axis {
                    let raw = input.axes[index];
                    let value = if raw.abs() < STICK_DEADZONE { 0.0 } else { raw };
                    *target = (*target + value * axis.input_sign * axis.speed_per_second * dt)
                        .clamp(axis.minimum, axis.maximum);
                }
                let native = *target * axis.native_per_unit;
                format!("TARGET {} {native:.5}", axis.slot)
            })
            .collect();

        for (servo, target) in self.profile.pwm_servos.iter().zip(&mut self.pwm_targets_us) {
            if let Some(index) = servo.input_axis {
                let raw = input.axes[index];
                let value = if raw.abs() < STICK_DEADZONE { 0.0 } else { raw };
                *target = (*target + value * servo.input_sign * servo.speed_us_per_second * dt)
                    .clamp(f32::from(servo.minimum_us), f32::from(servo.maximum_us));
            }
            lines.push(
                svmd::Command::Set {
                    channel: servo.channel,
                    pulse_us: target.round() as u16,
                }
                .to_cctl_line(),
            );
            lines.push(
                svmd::Command::Enable {
                    channel: servo.channel,
                    enabled: servo.enabled,
                }
                .to_cctl_line(),
            );
        }
        lines
    }

    #[cfg(test)]
    fn target(&self, name: &str) -> Option<f32> {
        self.profile
            .axes
            .iter()
            .position(|axis| axis.name == name)
            .map(|index| self.targets[index])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn neutral_input() -> ControllerState {
        ControllerState {
            axes: [0.0; 6],
            buttons: [0; 17],
        }
    }

    #[test]
    fn embedded_profile_is_valid() {
        let profile = MachineProfile::load(None).unwrap();
        assert_eq!(profile.axes.len(), 3);
        assert_eq!(profile.axes[0].name, "r");
    }

    #[test]
    fn rejects_duplicate_slots() {
        let source = EMBEDDED_PROFILE.replace("slot = 1", "slot = 0");
        assert!(MachineProfile::parse(&source).is_err());
    }

    #[test]
    fn integrates_input_and_converts_to_native_units() {
        let profile = MachineProfile::load(None).unwrap();
        let mut machine = MachineController::new(profile);
        let mut input = neutral_input();
        input.axes[1] = 0.5;

        let lines = machine.update(&input, 0.1);

        assert!((machine.target("r").unwrap() - 5.0).abs() < 1e-5);
        assert_eq!(lines[0], "TARGET 0 0.50025");
        assert_eq!(lines[2], "TARGET 2 0.00000");
    }

    #[test]
    fn applies_deadzone_and_interval_limit() {
        let profile = MachineProfile::load(None).unwrap();
        let mut machine = MachineController::new(profile);
        let mut input = neutral_input();
        input.axes[0] = 0.05;
        machine.update(&input, 1.0);
        assert_eq!(machine.target("theta"), Some(0.0));

        input.axes[0] = 1.0;
        machine.update(&input, 1.0);
        assert_eq!(machine.target("theta"), Some(9.0));
    }

    #[test]
    fn validates_and_drives_pwm_servo_from_host_profile() {
        let source = format!(
            "{EMBEDDED_PROFILE}\n[[pwm_servos]]\nname = \"gripper\"\nchannel = 1\ninput_axis = 3\ninput_sign = -1.0\nspeed_us_per_second = 1000.0\nminimum_us = 900\nmaximum_us = 2100\ninitial_us = 1500\nenabled = true\n"
        );
        let profile = MachineProfile::parse(&source).unwrap();
        assert!(profile.requires_can_bus_2());
        let mut machine = MachineController::new(profile);
        let mut input = neutral_input();
        input.axes[3] = 1.0;

        let lines = machine.update(&input, 0.1);

        assert_eq!(lines[3], "CAN 2 768 0101010005780000");
        assert_eq!(lines[4], "CAN 2 768 0102010100000000");
    }
}

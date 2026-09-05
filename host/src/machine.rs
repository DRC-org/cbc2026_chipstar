//! 機体固有の軸構成と、コントローラ入力からデバイス指令への変換。
//!
//! FWはslotのネイティブ単位だけを扱い、軸名、機械換算、入力割当はこの層に閉じる。

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::controller::ControllerState;
use crate::telemetry::Telemetry;
use crate::svmd;

const EMBEDDED_PROFILE: &str = include_str!("../config/rtheta.toml");
const MAX_SLOTS: usize = 3;
const MAX_INPUT_INTERVAL_S: f32 = 0.1;
const STICK_DEADZONE: f32 = 0.1;
const PWM_CHANNEL_COUNT: usize = 4;
const PWM_MIN_US: u16 = 500;
const PWM_MAX_US: u16 = 2500;
const SERIAL_SERVO_MAX_COUNT: usize = 16;
const CONTACT_COUNT: usize = 3;

/// 軸の端にあるリミットスイッチ。接点はcctlのSW1..SW3に対応する。
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
pub struct AxisLimit {
    /// 接点のbit位置。SW1=0, SW2=1, SW3=2。
    pub input: u8,
    /// スイッチへ近づく機体単位の向き。`1.0` または `-1.0`。
    pub direction: f32,
    /// B接点（常閉）配線なら true。断線が「到達」側に倒れる。
    #[serde(default = "yes")]
    pub normally_closed: bool,
}

impl AxisLimit {
    /// リミットに到達しているか。接点が閉じているとき `contacts` のbitが1。
    fn reached(&self, contacts: u8) -> bool {
        let closed = contacts & (1 << self.input) != 0;
        closed != self.normally_closed
    }
}

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
    /// 原点採用時にこの軸へ与える機体単位の値。
    #[serde(default)]
    pub origin_position: f32,
    /// 省略するとスイッチを持たず、原点採用は手動操作だけになる。
    #[serde(default)]
    pub limit: Option<AxisLimit>,
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

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct SerialServoProfile {
    pub name: String,
    pub id: u8,
    pub input_axis: Option<usize>,
    #[serde(default = "one")]
    pub input_sign: f32,
    pub speed_position_per_second: f32,
    pub minimum_position: u16,
    pub maximum_position: u16,
    pub initial_position: u16,
    pub move_speed: u16,
    pub acceleration: u8,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct SerialSvmdProfile {
    pub device: String,
    #[serde(default = "serial_svmd_baud")]
    pub baud_rate: u32,
    #[serde(default)]
    pub servos: Vec<SerialServoProfile>,
}

fn serial_svmd_baud() -> u32 {
    38_400
}

fn one() -> f32 {
    1.0
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct MachineProfile {
    #[serde(default)]
    pub dc_motors: Vec<crate::dcmd::MotorProfile>,
    pub protocol_version: u8,
    #[serde(default)]
    pub axes: Vec<AxisProfile>,
    #[serde(default)]
    pub pwm_servos: Vec<PwmServoProfile>,
    pub serial_svmd: Option<SerialSvmdProfile>,
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
        crate::dcmd::validate(&self.dc_motors)?;
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
        if self.axes.is_empty()
            && self.pwm_servos.is_empty()
            && self.serial_svmd.is_none()
            && self.dc_motors.is_empty()
        {
            bail!("軸またはサーボを1つ以上指定してください");
        }

        let mut slots = HashSet::new();
        let mut names = HashSet::new();
        let mut limit_inputs = HashSet::new();
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
                axis.origin_position,
            ];
            if numbers.iter().any(|value| !value.is_finite()) {
                bail!("軸設定に有限でない値があります: {}", axis.name);
            }
            if axis.input_sign.abs() != 1.0
                || axis.speed_per_second < 0.0
                || axis.native_per_unit == 0.0
                || axis.minimum >= axis.maximum
                || !(axis.minimum..=axis.maximum).contains(&axis.initial)
                || !(axis.minimum..=axis.maximum).contains(&axis.origin_position)
            {
                bail!("軸設定の範囲が不正です: {}", axis.name);
            }
            if let Some(limit) = &axis.limit {
                if limit.input as usize >= CONTACT_COUNT || !limit_inputs.insert(limit.input) {
                    bail!(
                        "接点は0..={}で重複なく指定してください: {}",
                        CONTACT_COUNT - 1,
                        axis.name
                    );
                }
                if limit.direction.abs() != 1.0 {
                    bail!("limit.directionは1.0か-1.0で指定してください: {}", axis.name);
                }
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
        if let Some(board) = &self.serial_svmd {
            if board.device.trim().is_empty() || board.baud_rate == 0 {
                bail!("serial_svmdの接続設定が不正です");
            }
            if board.servos.is_empty() || board.servos.len() > SERIAL_SERVO_MAX_COUNT {
                bail!("serial_svmdのサーボ数は1..={SERIAL_SERVO_MAX_COUNT}で指定してください");
            }
            let mut ids = HashSet::new();
            for servo in &board.servos {
                if servo.id == 0 || servo.id > 253 || !ids.insert(servo.id) {
                    bail!("STS3215 IDは1..253で重複なく指定してください: {}", servo.id);
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
                    || !servo.speed_position_per_second.is_finite()
                    || servo.input_sign.abs() != 1.0
                    || servo.speed_position_per_second < 0.0
                    || servo.maximum_position > 4095
                    || servo.minimum_position >= servo.maximum_position
                    || !(servo.minimum_position..=servo.maximum_position)
                        .contains(&servo.initial_position)
                    || servo.move_speed > 1000
                    || servo.acceleration > 254
                {
                    bail!("STS3215設定の範囲が不正です: {}", servo.name);
                }
            }
        }
        Ok(())
    }

    pub fn requires_can_bus_2(&self) -> bool {
        !self.pwm_servos.is_empty() || !self.dc_motors.is_empty()
    }

    pub fn requires_serial_svmd(&self) -> bool {
        self.serial_svmd.is_some()
    }
}

/// 軸ごとの原点の状態。GUI 表示用。
#[derive(Clone, Debug, PartialEq)]
pub struct OriginState {
    pub name: String,
    pub unit: String,
    /// 原点を採用済みか。未採用の間は可動域のクランプを行わない。
    pub captured: bool,
    /// リミットスイッチに到達しているか。スイッチを持たない軸は None。
    pub at_limit: Option<bool>,
    pub position: f32,
}

pub struct MachineController {
    profile: MachineProfile,
    targets: Vec<f32>,
    /// 機体単位の0に対応するネイティブ値。原点採用でずらす。
    origins_native: Vec<f32>,
    origin_captured: Vec<bool>,
    /// 接点の前回値。立ち上がりの検出に使う。
    last_contacts: Option<u8>,
    pwm_targets_us: Vec<f32>,
    serial_targets: Vec<f32>,
}

impl MachineController {
    pub fn new(profile: MachineProfile) -> Self {
        let targets = profile.axes.iter().map(|axis| axis.initial).collect();
        let pwm_targets_us = profile
            .pwm_servos
            .iter()
            .map(|servo| f32::from(servo.initial_us))
            .collect();
        let serial_targets = profile
            .serial_svmd
            .iter()
            .flat_map(|board| &board.servos)
            .map(|servo| f32::from(servo.initial_position))
            .collect();
        Self {
            origins_native: vec![0.0; profile.axes.len()],
            origin_captured: vec![false; profile.axes.len()],
            last_contacts: None,
            profile,
            targets,
            pwm_targets_us,
            serial_targets,
        }
    }

    /// 軸ごとの原点と接点の状態。
    pub fn origin_states(&self, telemetry: Option<&Telemetry>) -> Vec<OriginState> {
        let contacts = telemetry.and_then(|telemetry| telemetry.contacts);
        self.profile
            .axes
            .iter()
            .zip(&self.targets)
            .zip(&self.origin_captured)
            .map(|((axis, target), captured)| OriginState {
                name: axis.name.clone(),
                unit: axis.unit.clone(),
                captured: *captured,
                at_limit: axis
                    .limit
                    .and_then(|limit| contacts.map(|contacts| limit.reached(contacts))),
                position: *target,
            })
            .collect()
    }

    /// いまの実測位置に `origin_position` を割り当てる。
    ///
    /// 目標値も同じ値へ置き直すので、採用の前後で軸は動かない。
    pub fn capture_origin(&mut self, index: usize, telemetry: Option<&Telemetry>) -> bool {
        let Some(axis) = self.profile.axes.get(index) else {
            return false;
        };
        let Some(telemetry) = telemetry else {
            return false;
        };
        let Some(slot) = telemetry.slots.get(axis.slot as usize) else {
            return false;
        };
        self.origins_native[index] = slot.measured - axis.origin_position * axis.native_per_unit;
        self.targets[index] = axis.origin_position;
        self.origin_captured[index] = true;
        true
    }

    pub fn hello_line(&self) -> String {
        format!("HELLO {}", self.profile.protocol_version)
    }

    pub fn serial_svmd_hello_line(&self) -> String {
        format!("HELLO {}", self.profile.protocol_version)
    }

    pub fn update(
        &mut self,
        input: &ControllerState,
        elapsed_s: f32,
        telemetry: Option<&Telemetry>,
    ) -> Vec<String> {
        let dt = elapsed_s.clamp(0.0, MAX_INPUT_INTERVAL_S);
        let contacts = telemetry.and_then(|telemetry| telemetry.contacts);

        // 接点の立ち上がりでその軸の原点を採る。ジョグで当てるだけで原点が決まる。
        if let (Some(contacts), Some(previous)) = (contacts, self.last_contacts) {
            for index in 0..self.profile.axes.len() {
                let Some(limit) = self.profile.axes[index].limit else {
                    continue;
                };
                if limit.reached(contacts) && !limit.reached(previous) {
                    self.capture_origin(index, telemetry);
                }
            }
        }
        if contacts.is_some() {
            self.last_contacts = contacts;
        }

        let mut lines = Vec::with_capacity(self.profile.axes.len());
        for index in 0..self.profile.axes.len() {
            let axis = &self.profile.axes[index];
            let (slot, native_per_unit) = (axis.slot, axis.native_per_unit);
            let (minimum, maximum) = (axis.minimum, axis.maximum);
            let (input_axis, input_sign) = (axis.input_axis, axis.input_sign);
            let speed_per_second = axis.speed_per_second;
            let limit = axis.limit;

            if let Some(axis_index) = input_axis {
                let raw = input.axes[axis_index];
                let mut value = if raw.abs() < STICK_DEADZONE { 0.0 } else { raw };
                // 到達している間はスイッチへ近づく向きだけを捨てる。逆向きには戻せる。
                if let (Some(limit), Some(contacts)) = (limit, contacts)
                    && limit.reached(contacts)
                    && value * input_sign * limit.direction > 0.0
                {
                    value = 0.0;
                }
                let target = &mut self.targets[index];
                *target += value * input_sign * speed_per_second * dt;
                // 可動域は原点が決まって初めて意味を持つ。採用前に効かせると
                // 暫定原点基準のクランプでスイッチまで届かなくなる。
                if self.origin_captured[index] {
                    *target = target.clamp(minimum, maximum);
                }
            }
            let native = self.targets[index] * native_per_unit + self.origins_native[index];
            lines.push(format!("TARGET {slot} {native:.5}"));
        }

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
        }
        lines.extend(crate::dcmd::targets(&self.profile.dc_motors, &input.axes));
        lines
    }

    pub fn update_serial_svmd(&mut self, input: &ControllerState, elapsed_s: f32) -> Vec<String> {
        let Some(board) = &self.profile.serial_svmd else {
            return Vec::new();
        };
        let dt = elapsed_s.clamp(0.0, MAX_INPUT_INTERVAL_S);
        let mut lines = Vec::with_capacity(board.servos.len() * 2);
        for (servo, target) in board.servos.iter().zip(&mut self.serial_targets) {
            if let Some(index) = servo.input_axis {
                let raw = input.axes[index];
                let value = if raw.abs() < STICK_DEADZONE { 0.0 } else { raw };
                *target = (*target
                    + value * servo.input_sign * servo.speed_position_per_second * dt)
                    .clamp(
                        f32::from(servo.minimum_position),
                        f32::from(servo.maximum_position),
                    );
            }
            lines.push(format!(
                "SERVO TARGET {} {} {} {}",
                servo.id,
                target.round() as u16,
                servo.move_speed,
                servo.acceleration
            ));
            lines.push(format!(
                "SERVO ENABLE {} {}",
                servo.id,
                u8::from(servo.enabled)
            ));
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

        let lines = machine.update(&input, 0.1, None);

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
        machine.update(&input, 1.0, None);
        assert_eq!(machine.target("theta"), Some(0.0));

        input.axes[0] = 1.0;
        machine.update(&input, 1.0, None);
        assert_eq!(machine.target("theta"), Some(9.0));
    }

    /// SW1が閉じた状態（B接点の平常時）のテレメトリ。
    fn telemetry_with(contacts: u8, measured: [f32; 3]) -> Telemetry {
        use crate::telemetry::{RunMode, SlotState};
        Telemetry {
            uptime_ms: 0,
            slots: [
                SlotState { target: 0.0, measured: measured[0] },
                SlotState { target: 0.0, measured: measured[1] },
                SlotState { target: 0.0, measured: measured[2] },
            ],
            enabled_slots: 7,
            mode: RunMode::Run,
            error_bits: 0,
            contacts: Some(contacts),
        }
    }

    #[test]
    fn captures_origin_on_the_limit_edge_without_moving_the_axis() {
        let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
        let mut input = neutral_input();
        input.axes[1] = 1.0;

        // 平常時はSW1もSW2も閉じている（B接点）。
        machine.update(&input, 0.1, Some(&telemetry_with(0b011, [0.0; 3])));
        assert!(!machine.origin_states(None)[0].captured);

        // rが前進しきってSW1が開く。その瞬間の実測値に最大位置を割り当てる。
        let reached = telemetry_with(0b010, [8.0, 0.0, 0.0]);
        let lines = machine.update(&input, 0.1, Some(&reached));
        let origins = machine.origin_states(Some(&reached));
        assert!(origins[0].captured);
        assert!((origins[0].position - 120.0).abs() < 1e-3);
        // 採用の前後で軸を動かさない。
        assert_eq!(lines[0], "TARGET 0 8.00000");
    }

    #[test]
    fn limit_blocks_only_the_direction_that_reaches_it() {
        let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
        let reached = telemetry_with(0b010, [8.0, 0.0, 0.0]);
        machine.update(&neutral_input(), 0.1, Some(&telemetry_with(0b011, [8.0, 0.0, 0.0])));
        machine.update(&neutral_input(), 0.1, Some(&reached));

        // 前進側は捨てる。
        let mut forward = neutral_input();
        forward.axes[1] = 1.0;
        machine.update(&forward, 0.1, Some(&reached));
        assert!((machine.target("r").unwrap() - 120.0).abs() < 1e-3);

        // 後退側は通し、スイッチから抜けられる。
        let mut back = neutral_input();
        back.axes[1] = -1.0;
        machine.update(&back, 0.1, Some(&reached));
        assert!(machine.target("r").unwrap() < 120.0);
    }

    #[test]
    fn clamps_travel_only_after_the_origin_is_known() {
        let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
        let mut input = neutral_input();
        input.axes[1] = -1.0;

        // 未採用の間は暫定原点基準のクランプを効かせない。
        for _ in 0..5 {
            machine.update(&input, 0.1, None);
        }
        assert!(machine.target("r").unwrap() < 0.0);

        // 手動採用の後は可動域が意味を持ち、最大側で頭打ちになる。
        let telemetry = telemetry_with(0b011, [0.0, 0.0, 0.0]);
        assert!(machine.capture_origin(0, Some(&telemetry)));
        assert!((machine.target("r").unwrap() - 120.0).abs() < 1e-3);
        input.axes[1] = 1.0;
        machine.update(&input, 0.1, Some(&telemetry));
        assert!((machine.target("r").unwrap() - 120.0).abs() < 1e-3);
    }

    #[test]
    fn treats_unknown_contacts_as_no_limit_information() {
        let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
        let mut input = neutral_input();
        input.axes[1] = 1.0;
        // sw= を持たないFWでは、接点を「全て到達」と誤解して止めてはいけない。
        let unknown = Telemetry { contacts: None, ..telemetry_with(0, [0.0; 3]) };
        machine.update(&input, 0.1, Some(&unknown));
        assert!(machine.target("r").unwrap() > 0.0);
        assert!(!machine.origin_states(Some(&unknown))[0].captured);
        assert_eq!(machine.origin_states(Some(&unknown))[0].at_limit, None);
    }

    #[test]
    fn axis_without_a_switch_is_captured_only_by_hand() {
        let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
        let telemetry = telemetry_with(0b011, [0.0, 3000.0, 0.0]);
        machine.update(&neutral_input(), 0.1, Some(&telemetry));
        let theta = 1;
        assert_eq!(machine.origin_states(Some(&telemetry))[theta].at_limit, None);
        assert!(!machine.origin_states(Some(&telemetry))[theta].captured);

        assert!(machine.capture_origin(theta, Some(&telemetry)));
        let states = machine.origin_states(Some(&telemetry));
        assert!(states[theta].captured);
        assert_eq!(states[theta].position, 0.0);
        // 採用直後は指令も実測に一致し、軸は動かない。
        let lines = machine.update(&neutral_input(), 0.1, Some(&telemetry));
        assert_eq!(lines[1], "TARGET 1 3000.00000");
    }

    #[test]
    fn rejects_a_limit_on_a_contact_the_board_does_not_have() {
        let source = EMBEDDED_PROFILE.replace("input = 0
direction = 1.0", "input = 3
direction = 1.0");
        assert!(MachineProfile::parse(&source).is_err());
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

        let lines = machine.update(&input, 0.1, None);

        assert_eq!(lines[3], "CAN 2 768 0101010005780000");
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn validates_and_drives_serial_servo_from_host_profile() {
        let source = format!(
            "{EMBEDDED_PROFILE}\n[serial_svmd]\ndevice = \"/dev/ttyUSB0\"\n\n[[serial_svmd.servos]]\nname = \"arm\"\nid = 12\ninput_axis = 4\ninput_sign = 1.0\nspeed_position_per_second = 500.0\nminimum_position = 1000\nmaximum_position = 3000\ninitial_position = 2000\nmove_speed = 400\nacceleration = 30\nenabled = true\n"
        );
        let profile = MachineProfile::parse(&source).unwrap();
        assert!(profile.requires_serial_svmd());
        assert_eq!(profile.serial_svmd.as_ref().unwrap().baud_rate, 38_400);
        let mut machine = MachineController::new(profile);
        let mut input = neutral_input();
        input.axes[4] = 1.0;

        assert_eq!(
            machine.update_serial_svmd(&input, 0.1),
            vec!["SERVO TARGET 12 2050 400 30", "SERVO ENABLE 12 1"]
        );
    }
}

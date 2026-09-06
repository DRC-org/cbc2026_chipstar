//! 機体固有の軸構成と、コントローラ入力からデバイス指令への変換。
//!
//! FWはslotのネイティブ単位だけを扱い、軸名、機械換算、入力割当はこの層に閉じる。

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::controller::ControllerState;
use crate::svmd;
use crate::telemetry::Telemetry;

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
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
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

/// serial_svmdはcctlのFDCAN2経由で繋ぐ。接続先はcctlのリンクなので持たない。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct SerialSvmdProfile {
    #[serde(default)]
    pub servos: Vec<SerialServoProfile>,
}

fn one() -> f32 {
    1.0
}

fn yes() -> bool {
    true
}

/// cctlの実行時パラメータ。名前とidの対応は device_protocol.md の表に従う。
/// FWを書き直さずに実機調整を終えるため、調整対象はすべてここへ書く。
/// パラメータ名から値への対応。名前は device_protocol.md の表に従う。
pub type ParameterMap = std::collections::BTreeMap<String, f32>;

pub const PARAMETER_NAMES: [&str; 33] = [
    "m3508_pos_kp",
    "m3508_pos_ki",
    "m3508_pos_kd",
    "m3508_max_rpm",
    "m3508_vel_kp",
    "m3508_vel_ki",
    "m3508_vel_kd",
    "m3508_max_current_ma",
    "el05_loc_kp",
    "el05_limit_spd",
    "el05_limit_cur",
    "dm_p_max",
    "dm_v_max",
    "dm_t_max",
    "dm_pos_vel_limit",
    "slot0_min",
    "slot0_max",
    "slot1_min",
    "slot1_max",
    "slot2_min",
    "slot2_max",
    "c620_esc_id",
    "dm_can_id",
    "dm_mst_id",
    "el05_motor_id",
    "el05_host_id",
    "m3508_period_ms",
    "dm_period_ms",
    "el05_period_ms",
    "telemetry_period_ms",
    "watchdog_ms",
    "feedback_timeout_ms",
    "m3508_max_temperature_c",
];

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct MachineProfile {
    #[serde(default)]
    pub dc_motors: Vec<crate::dcmd::MotorProfile>,
    pub protocol_version: u8,
    #[serde(default)]
    pub axes: Vec<AxisProfile>,
    #[serde(default)]
    pub pwm_servos: Vec<PwmServoProfile>,
    pub serial_svmd: Option<SerialSvmdProfile>,
    /// cctlへ起動時に送る実行時パラメータ。省略した項目はFWの既定値が残る。
    #[serde(default)]
    pub parameters: ParameterMap,
    /// CAN先の基板へ送る実行時パラメータ。
    #[serde(default)]
    pub svmd_parameters: ParameterMap,
    #[serde(default)]
    pub dcmd_parameters: ParameterMap,
    #[serde(default)]
    pub serial_svmd_parameters: ParameterMap,
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

    pub fn validate(&self) -> Result<()> {
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

        let tables: [(&ParameterMap, &[&str]); 4] = [
            (&self.parameters, &PARAMETER_NAMES),
            (&self.svmd_parameters, &crate::svmd::PARAMETER_NAMES),
            (&self.dcmd_parameters, &crate::dcmd::PARAMETER_NAMES),
            (
                &self.serial_svmd_parameters,
                &crate::serial_svmd::PARAMETER_NAMES,
            ),
        ];
        for (values, names) in tables {
            for (name, value) in values {
                if !names.contains(&name.as_str()) {
                    bail!("未対応のパラメータ名です: {name}");
                }
                if !value.is_finite() {
                    bail!("パラメータに有限でない値があります: {name}");
                }
            }
        }

        for (minimum, maximum) in [
            ("slot0_min", "slot0_max"),
            ("slot1_min", "slot1_max"),
            ("slot2_min", "slot2_max"),
        ] {
            if let (Some(low), Some(high)) =
                (self.parameters.get(minimum), self.parameters.get(maximum))
                && low >= high
            {
                bail!("基板の可動域が逆転しています: {minimum}/{maximum}");
            }
        }
        for (name, value) in &self.parameters {
            if (name.ends_with("_id") || name.ends_with("_ms")) && value.fract() != 0.0 {
                bail!("整数を指定してください: {name}");
            }
        }
        if self
            .parameters
            .get("telemetry_period_ms")
            .is_some_and(|v| *v > 100.0 || *v < 10.0)
        {
            bail!("telemetry_period_msは10..100で指定してください");
        }
        if self
            .parameters
            .get("watchdog_ms")
            .is_some_and(|v| *v < 150.0 || *v > 1000.0)
        {
            bail!("watchdog_msは150..1000で指定してください");
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
                    bail!(
                        "limit.directionは1.0か-1.0で指定してください: {}",
                        axis.name
                    );
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

    /// 実行時パラメータの行。能力確認が済んだ直後に一度だけ送る。
    /// cctlはASCII、CAN先の基板はゲートウェイ行になる。
    pub fn parameter_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .parameters
            .iter()
            .filter_map(|(name, value)| {
                let id = PARAMETER_NAMES.iter().position(|entry| entry == name)?;
                Some(format!("PARAM {id} {value:.5}"))
            })
            .collect();
        type BoardTable<'a> = (&'a ParameterMap, &'a [&'a str], fn(u8, f32) -> String);
        let boards: [BoardTable; 3] = [
            (
                &self.svmd_parameters,
                &crate::svmd::PARAMETER_NAMES,
                crate::svmd::parameter_line,
            ),
            (
                &self.dcmd_parameters,
                &crate::dcmd::PARAMETER_NAMES,
                crate::dcmd::parameter_line,
            ),
            (
                &self.serial_svmd_parameters,
                &crate::serial_svmd::PARAMETER_NAMES,
                crate::serial_svmd::parameter_line,
            ),
        ];
        for (values, names, encode) in boards {
            for (name, value) in values {
                if let Some(id) = names.iter().position(|entry| entry == name) {
                    lines.push(encode(id as u8, *value));
                }
            }
        }
        lines
    }

    pub fn requires_can_bus_2(&self) -> bool {
        !self.pwm_servos.is_empty() || !self.dc_motors.is_empty() || self.serial_svmd.is_some()
    }

    pub fn requires_serial_svmd(&self) -> bool {
        self.serial_svmd.is_some()
    }
}

/// 軸ごとの原点の状態。GUI 表示用。
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct OriginState {
    pub name: String,
    pub unit: String,
    /// 採用済みだった原点を、フィードバックの途切れや飛びで失ったか。
    pub lost: bool,
    /// 原点を採用済みか。未採用の間は可動域のクランプを行わない。
    pub captured: bool,
    /// リミットスイッチに到達しているか。スイッチを持たない軸は None。
    pub at_limit: Option<bool>,
    pub position: f32,
    pub target: f32,
}

pub struct MachineController {
    profile: MachineProfile,
    targets: Vec<f32>,
    /// 機体単位の0に対応するネイティブ値。原点採用でずらす。
    origins_native: Vec<f32>,
    origin_captured: Vec<bool>,
    soft_limits: bool,
    origin_lost: Vec<bool>,
    /// 実測値の前回値。電源再投入による飛びを見つけるために持つ。
    last_measured: Vec<Option<f32>>,
    /// 接点の前回値。立ち上がりの検出に使う。
    last_contacts: Option<u8>,
    pwm_targets_us: Vec<f32>,
    serial_targets: Vec<f32>,
    /// 位置読み出しの巡回位置。
    serial_read_cursor: usize,
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
            soft_limits: true,
            origin_lost: vec![false; profile.axes.len()],
            last_measured: vec![None; profile.axes.len()],
            last_contacts: None,
            profile,
            targets,
            pwm_targets_us,
            serial_targets,
            serial_read_cursor: 0,
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
            .enumerate()
            .map(|(index, ((axis, target), captured))| OriginState {
                name: axis.name.clone(),
                unit: axis.unit.clone(),
                captured: *captured,
                lost: self.origin_lost[index],
                at_limit: axis
                    .limit
                    .and_then(|limit| contacts.map(|contacts| limit.reached(contacts))),
                position: telemetry
                    .map(|t| {
                        (t.slots[axis.slot as usize].measured - self.origins_native[index])
                            / axis.native_per_unit
                    })
                    .unwrap_or(*target),
                target: *target,
            })
            .collect()
    }

    /// 原点が信用できなくなった軸を見つける。
    ///
    /// EL05もDMも、電源を入れ直すとその時点の姿勢が0になる。hostが持っている
    /// 機体座標との対応はそこで崩れるが、実測値は何事もなかったように0付近を
    /// 返すため、気づかないとソフトリミットが実際とずれたまま動いてしまう。
    ///
    /// 検出は2つ。RUN中の応答途絶（FWが `stale` で通知する）と、1周期では
    /// ありえない実測値の飛び。どちらも起きたら原点を捨て、採り直しを求める。
    fn check_feedback_continuity(&mut self, telemetry: Option<&Telemetry>) {
        let Some(telemetry) = telemetry else {
            return;
        };
        for index in 0..self.profile.axes.len() {
            let axis = &self.profile.axes[index];
            let Some(slot) = telemetry.slots.get(axis.slot as usize) else {
                continue;
            };
            let measured = slot.measured;
            // ジョグ0.2秒ぶんを超える移動は、テレメトリ1周期(50ms)では起こらない。
            // 通常のジョグは1周期あたり0.05秒ぶんしか進まないので4倍の余裕がある。
            let jump_limit = (axis.speed_per_second * axis.native_per_unit * 0.2).abs();
            let jumped = self.last_measured[index].is_some_and(|previous| {
                jump_limit > 0.0 && (measured - previous).abs() > jump_limit
            });
            let stale = telemetry.stale_slots & (1 << axis.slot) != 0;
            if (jumped || stale) && self.origin_captured[index] {
                self.origin_captured[index] = false;
                self.origin_lost[index] = true;
                self.origins_native[index] = measured;
                self.targets[index] = 0.0;
            }
            self.last_measured[index] = Some(measured);
        }
    }

    /// 機体座標の可動域で目標を止めるかを切り替える。
    ///
    /// 原点を採り直すときは、いまの原点から見た可動域の外へ動かす必要がある。
    /// 外しても基板側のslot絶対可動域は効いたままなので、機構は保護される。
    pub fn set_soft_limits(&mut self, enabled: bool) {
        self.soft_limits = enabled;
    }

    /// いまの実測位置を目標として取り込む。
    ///
    /// 目標を過去の値のまま RUN すると、機体がその位置まで戻ろうとして跳ねる。
    /// 非常停止で手動退避した後がとくに危ない。位置ループを有効にする直前に
    /// 目標と実測を揃えておけば、RUN してもその場を保持する。
    pub fn hold_at_measured(&mut self, telemetry: Option<&Telemetry>) {
        let Some(telemetry) = telemetry else {
            return;
        };
        for index in 0..self.profile.axes.len() {
            let axis = &self.profile.axes[index];
            let Some(slot) = telemetry.slots.get(axis.slot as usize) else {
                continue;
            };
            let value = (slot.measured - self.origins_native[index]) / axis.native_per_unit;
            if !value.is_finite() {
                continue;
            }
            self.targets[index] = value;
        }
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
        if !slot.measured.is_finite() || telemetry.stale_slots & (1 << axis.slot) != 0 {
            return false;
        }
        self.last_measured[index] = Some(slot.measured);
        self.origins_native[index] = slot.measured - axis.origin_position * axis.native_per_unit;
        self.targets[index] = axis.origin_position;
        self.origin_captured[index] = true;
        self.origin_lost[index] = false;
        true
    }

    pub fn update(
        &mut self,
        input: &ControllerState,
        elapsed_s: f32,
        telemetry: Option<&Telemetry>,
    ) -> Vec<String> {
        let dt = elapsed_s.clamp(0.0, MAX_INPUT_INTERVAL_S);
        let contacts = telemetry.and_then(|telemetry| telemetry.contacts);

        self.check_feedback_continuity(telemetry);

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
                // スイッチのある軸は、採用前にクランプするとスイッチまで届かない。
                // スイッチのない軸は届く先がないので、起動時姿勢を基準に最初から
                // 制限する。θのケーブル巻き込みを無制限にしないため。
                if self.soft_limits && (self.origin_captured[index] || limit.is_none()) {
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
        lines.extend(self.serial_svmd_lines(input, dt));
        lines
    }

    /// 原点・接点の観測は停止中にも行う。位置目標は積算しない。
    pub fn observe(&mut self, telemetry: &Telemetry) {
        let neutral = ControllerState {
            axes: [0.0; 6],
            buttons: [0; 17],
        };
        self.hold_at_measured(Some(telemetry));
        // 既存の接点エッジ処理を共有し、生成された出力は送信しない。
        self.update(&neutral, 0.0, Some(telemetry));
        self.hold_at_measured(Some(telemetry));
    }

    pub fn invalidate_origins(&mut self) {
        for i in 0..self.profile.axes.len() {
            self.origin_lost[i] |= self.origin_captured[i];
            self.origin_captured[i] = false;
            self.last_measured[i] = None;
        }
        self.last_contacts = None;
    }

    pub fn jog_lines(
        &self,
        input: &ControllerState,
        telemetry: &Telemetry,
        slow: bool,
    ) -> Vec<String> {
        self.profile
            .axes
            .iter()
            .enumerate()
            .map(|(i, axis)| {
                let raw = axis.input_axis.map(|n| input.axes[n]).unwrap_or(0.0);
                let raw = if raw.is_finite() && raw.abs() >= STICK_DEADZONE {
                    raw.clamp(-1.0, 1.0)
                } else {
                    0.0
                };
                let mut velocity =
                    raw * axis.input_sign * axis.speed_per_second * if slow { 0.2 } else { 1.0 };
                if let Some(limit) = axis.limit
                    && (telemetry.contacts.is_none()
                        || telemetry
                            .contacts
                            .is_some_and(|c| limit.reached(c) && velocity * limit.direction > 0.0))
                {
                    velocity = 0.0;
                }
                let measured = (telemetry.slots[axis.slot as usize].measured
                    - self.origins_native[i])
                    / axis.native_per_unit;
                if self.soft_limits {
                    if !self.origin_captured[i] {
                        velocity = 0.0;
                    }
                    // FWの先行距離100msと通信周期を含め、境界付近では減速する。
                    if velocity > 0.0 {
                        velocity = velocity.min(((axis.maximum - measured) / 0.2).max(0.0));
                    }
                    if velocity < 0.0 {
                        velocity = velocity.max(((axis.minimum - measured) / 0.2).min(0.0));
                    }
                }
                format!("JOG {} {:.5}", axis.slot, velocity * axis.native_per_unit)
            })
            .collect()
    }

    fn serial_svmd_lines(&mut self, input: &ControllerState, dt: f32) -> Vec<String> {
        let Some(board) = &self.profile.serial_svmd else {
            return Vec::new();
        };
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
            lines.push(
                crate::serial_svmd::Command::Target {
                    id: servo.id,
                    position: target.round() as u16,
                    speed: servo.move_speed,
                    acceleration: servo.acceleration,
                }
                .to_cctl_line(),
            );
            lines.push(
                crate::serial_svmd::Command::Enable {
                    id: servo.id,
                    enabled: servo.enabled,
                }
                .to_cctl_line(),
            );
        }
        // 実測位置は1周期に1IDずつ巡回して読む。目標送信を遅らせないため。
        if !board.servos.is_empty() {
            let index = self.serial_read_cursor % board.servos.len();
            self.serial_read_cursor = self.serial_read_cursor.wrapping_add(1);
            lines.push(
                crate::serial_svmd::Command::Read {
                    id: board.servos[index].id,
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
    fn every_axis_can_be_jogged_for_manual_checks() {
        // 配線後の手動確認とホーミングに必要。動かせない軸があると原点が採れない。
        let profile = MachineProfile::load(None).unwrap();
        for axis in &profile.axes {
            assert!(
                axis.input_axis.is_some() && axis.speed_per_second > 0.0,
                "{} を手動で動かせない",
                axis.name
            );
        }
    }

    /// `[parameters]` の検証用に、最小構成のプロファイルを組み立てる。
    fn profile_with_parameters(body: &str) -> String {
        format!(
            r#"
protocol_version = 1

[parameters]
{body}

[[axes]]
name = "r"
unit = "mm"
slot = 0
input_axis = 1
speed_per_second = 10.0
native_per_unit = 1.0
minimum = -10.0
maximum = 10.0
initial = 0.0
"#
        )
    }

    #[test]
    fn sends_named_parameters_as_numeric_ids() {
        let source = profile_with_parameters("m3508_vel_kp = 0.9\nwatchdog_ms = 300.0");
        let profile = MachineProfile::parse(&source).unwrap();
        let lines = profile.parameter_lines();
        assert!(lines.contains(&"PARAM 4 0.90000".to_owned()));
        assert!(lines.contains(&"PARAM 30 300.00000".to_owned()));
    }

    #[test]
    fn sends_can_board_parameters_through_the_gateway() {
        let source = format!(
            "{EMBEDDED_PROFILE}\n[dcmd_parameters]\nmax_duty = 1000.0\n\n[serial_svmd_parameters]\nservo_baud = 1000000.0\n"
        );
        let profile = MachineProfile::parse(&source).unwrap();
        let lines = profile.parameter_lines();
        // DCMD id 0 (max_duty) = 1000.0f = 0x447A0000
        assert!(lines.contains(&"CAN 2 784 01070000447A0000".to_owned()));
        // serial_svmd id 0 (servo_baud) = 1000000.0f = 0x49742400
        assert!(lines.contains(&"CAN 2 800 0109000049742400".to_owned()));
    }

    #[test]
    fn rejects_unknown_parameter_names() {
        let source = profile_with_parameters("no_such_gain = 1.0");
        assert!(MachineProfile::parse(&source).is_err());
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

        assert!((machine.target("r").unwrap() - 0.5).abs() < 1e-5);
        assert_eq!(lines[0], "TARGET 0 0.02000");
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
        assert_eq!(machine.target("theta"), Some(1.0));
    }

    /// SW1が閉じた状態（B接点の平常時）のテレメトリ。
    fn telemetry_with(contacts: u8, measured: [f32; 3]) -> Telemetry {
        use crate::telemetry::{RunMode, SlotState};
        Telemetry {
            uptime_ms: 0,
            slots: [
                SlotState {
                    target: 0.0,
                    measured: measured[0],
                },
                SlotState {
                    target: 0.0,
                    measured: measured[1],
                },
                SlotState {
                    target: 0.0,
                    measured: measured[2],
                },
            ],
            enabled_slots: 7,
            mode: RunMode::Run,
            error_bits: [0; 3],
            contacts: Some(contacts),
            stale_slots: 0,
            buses: 3,
        }
    }

    /// リミットスイッチを持つ機体のプロファイル。
    ///
    /// 実機はまだスイッチ未取付で `config/rtheta.toml` の `[axes.limit]` を
    /// 無効にしてあるため、リミットの検証はここで組み立てた構成で行う。
    fn profile_with_limits() -> MachineProfile {
        MachineProfile::parse(
            r#"
protocol_version = 1

[[axes]]
name = "r"
unit = "mm"
slot = 0
input_axis = 1
input_sign = 1.0
speed_per_second = 100.0
native_per_unit = 0.10005072
minimum = 0.0
maximum = 120.0
initial = 0.0
origin_position = 120.0

[axes.limit]
input = 0
direction = 1.0

[[axes]]
name = "theta"
unit = "deg"
slot = 1
input_axis = 0
input_sign = 1.0
speed_per_second = 90.0
native_per_unit = 140.82353
minimum = -180.0
maximum = 180.0
initial = 0.0

[[axes]]
name = "z"
unit = "mm"
slot = 2
input_axis = 3
input_sign = 1.0
speed_per_second = 30.0
native_per_unit = 0.15707964
minimum = 0.0
maximum = 75.0
initial = 0.0

[axes.limit]
input = 1
direction = -1.0
"#,
        )
        .unwrap()
    }

    #[test]
    fn power_cycling_a_motor_invalidates_the_origin() {
        let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
        let steady = telemetry_with(0, [5.0, 0.0, 0.0]);
        machine.update(&neutral_input(), 0.1, Some(&steady));
        assert!(machine.capture_origin(0, Some(&steady)));
        assert!(machine.origin_states(None)[0].captured);

        // モータが入り直して実測が0へ飛ぶ。1周期では起こりえない移動量。
        let restarted = telemetry_with(0, [0.0, 0.0, 0.0]);
        machine.update(&neutral_input(), 0.1, Some(&restarted));
        let origins = machine.origin_states(None);
        assert!(!origins[0].captured, "原点を捨てていない");
        assert!(origins[0].lost, "失ったことを伝えていない");
    }

    #[test]
    fn feedback_loss_reported_by_the_board_invalidates_the_origin() {
        let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
        let steady = telemetry_with(0, [5.0, 0.0, 0.0]);
        machine.update(&neutral_input(), 0.1, Some(&steady));
        assert!(machine.capture_origin(0, Some(&steady)));

        let lost = Telemetry {
            stale_slots: 0b001,
            ..telemetry_with(0, [5.0, 0.0, 0.0])
        };
        machine.update(&neutral_input(), 0.1, Some(&lost));
        assert!(!machine.origin_states(None)[0].captured);
        assert!(machine.origin_states(None)[0].lost);
    }

    #[test]
    fn normal_jogging_does_not_invalidate_the_origin() {
        let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
        let start = telemetry_with(0, [0.0, 0.0, 0.0]);
        machine.update(&neutral_input(), 0.1, Some(&start));
        assert!(machine.capture_origin(0, Some(&start)));

        // ジョグ相当の連続した移動では原点を捨てない。
        for step in 1..20 {
            let moving = telemetry_with(0, [step as f32 * 0.02, 0.0, 0.0]);
            machine.update(&neutral_input(), 0.05, Some(&moving));
            assert!(
                machine.origin_states(None)[0].captured,
                "step {step} で捨てた"
            );
        }
    }

    #[test]
    fn run_holds_the_current_position_instead_of_returning() {
        let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
        let mut input = neutral_input();

        // ジョグでrを進めてから、停止中に手で戻された状況をつくる。
        input.axes[1] = 1.0;
        for _ in 0..5 {
            machine.update(&input, 0.1, None);
        }
        assert!(machine.target("r").unwrap() > 4.0);

        // 実測は手で戻された位置。RUN前に取り込めば、その場を保持する。
        let moved = telemetry_with(0, [0.4, 0.0, 0.0]);
        machine.hold_at_measured(Some(&moved));
        assert!((machine.target("r").unwrap() - 10.0).abs() < 1e-3);

        // 直後の指令は実測と一致し、機体は動かない。
        let lines = machine.update(&neutral_input(), 0.1, Some(&moved));
        assert_eq!(lines[0], "TARGET 0 0.40000");
    }

    #[test]
    fn holding_needs_feedback() {
        let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
        let mut input = neutral_input();
        input.axes[1] = 1.0;
        machine.update(&input, 0.1, None);
        let before = machine.target("r").unwrap();
        // テレメトリが無ければ何もしない。勝手に0へ飛ばさない。
        machine.hold_at_measured(None);
        assert_eq!(machine.target("r"), Some(before));
    }

    #[test]
    fn captures_origin_on_the_limit_edge_without_moving_the_axis() {
        let mut machine = MachineController::new(profile_with_limits());
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
        let mut machine = MachineController::new(profile_with_limits());
        let reached = telemetry_with(0b010, [8.0, 0.0, 0.0]);
        machine.update(
            &neutral_input(),
            0.1,
            Some(&telemetry_with(0b011, [8.0, 0.0, 0.0])),
        );
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
        let mut machine = MachineController::new(profile_with_limits());
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
    fn clamps_switchless_axes_from_the_start() {
        let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
        let mut input = neutral_input();
        input.axes[0] = 1.0; // theta（スイッチなし）
        for _ in 0..200 {
            machine.update(&input, 0.1, None);
        }
        // 原点未採用でも可動域で頭打ちになる。ケーブルを巻き込ませない。
        assert!((machine.target("theta").unwrap() - 180.0).abs() < 1e-3);
    }

    #[test]
    fn treats_unknown_contacts_as_no_limit_information() {
        let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
        let mut input = neutral_input();
        input.axes[1] = 1.0;
        // sw= を持たないFWでは、接点を「全て到達」と誤解して止めてはいけない。
        let unknown = Telemetry {
            contacts: None,
            ..telemetry_with(0, [0.0; 3])
        };
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
        assert_eq!(
            machine.origin_states(Some(&telemetry))[theta].at_limit,
            None
        );
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
        // 接点は0..2しかない。3を指定した構成は受け付けない。
        let source = format!("{EMBEDDED_PROFILE}\n[axes.limit]\ninput = 3\ndirection = 1.0\n");
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
            "{EMBEDDED_PROFILE}\n[serial_svmd]\n\n[[serial_svmd.servos]]\nname = \"arm\"\nid = 12\ninput_axis = 4\ninput_sign = 1.0\nspeed_position_per_second = 500.0\nminimum_position = 1000\nmaximum_position = 3000\ninitial_position = 2000\nmove_speed = 400\nacceleration = 30\nenabled = true\n"
        );
        let profile = MachineProfile::parse(&source).unwrap();
        assert!(profile.requires_serial_svmd());
        assert!(profile.requires_can_bus_2());
        let mut machine = MachineController::new(profile);
        let mut input = neutral_input();
        input.axes[4] = 1.0;

        // 目標と有効化はcctlのFDCAN2経由で送る。
        let lines = machine.update(&input, 0.1, None);
        let servo: Vec<_> = lines
            .iter()
            .filter(|line| line.starts_with("CAN 2 800 "))
            .collect();
        // 目標・有効化に続けて、実測位置の読み出しを1IDぶん巡回する。
        assert_eq!(
            servo,
            [
                "CAN 2 800 01040C1E08020190",
                "CAN 2 800 01060C0100000000",
                "CAN 2 800 01070C0000000000"
            ]
        );
    }
}

#[cfg(test)]
mod manual_tests {
    use super::*;
    use crate::telemetry::{RunMode, SlotState};
    fn telemetry(native: f32) -> Telemetry {
        Telemetry {
            uptime_ms: 0,
            slots: [SlotState {
                measured: native,
                target: native,
            }; 3],
            enabled_slots: 7,
            mode: RunMode::Run,
            error_bits: [0; 3],
            contacts: Some(7),
            stale_slots: 0,
            buses: 3,
        }
    }
    #[test]
    fn frozen_feedback_does_not_accumulate_a_manual_position_target() {
        let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
        let t = telemetry(5.0);
        machine.observe(&t);
        assert!(machine.capture_origin(0, Some(&t)));
        let mut input = ControllerState::default();
        input.axes[1] = 0.5;
        for _ in 0..1000 {
            machine.observe(&t);
            assert_eq!(machine.jog_lines(&input, &t, false)[0], "JOG 0 0.20000");
        }
        assert_eq!(machine.origin_states(Some(&t))[0].position, 0.0);
        assert_eq!(machine.jog_lines(&input, &t, true)[0], "JOG 0 0.04000");
        input.axes[1] = 0.0;
        assert_eq!(machine.jog_lines(&input, &t, false)[0], "JOG 0 0.00000");
    }
    #[test]
    fn displayed_position_uses_the_captured_offset() {
        let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
        let start = telemetry(5.0);
        machine.observe(&start);
        machine.capture_origin(0, Some(&start));
        let moved = telemetry(5.04);
        machine.observe(&moved);
        assert!((machine.origin_states(Some(&moved))[0].position - 1.0).abs() < 0.001);
        let mut input = ControllerState::default();
        input.axes[1] = 1.0;
        let restarted = telemetry(0.0);
        machine.observe(&restarted);
        assert!(machine.origin_states(Some(&restarted))[0].lost);
        assert_eq!(
            machine.jog_lines(&input, &restarted, false)[0],
            "JOG 0 0.00000"
        );
    }
    #[test]
    fn holding_outside_a_soft_limit_does_not_command_a_return() {
        let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
        let start = telemetry(0.0);
        machine.capture_origin(0, Some(&start));
        machine.hold_at_measured(Some(&telemetry(8.0)));
        assert_eq!(machine.targets[0], 200.0);
    }
}

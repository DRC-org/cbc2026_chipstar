//! 機体固有の軸構成と、コントローラ入力からデバイス指令への変換。
//!
//! FWはslotのネイティブ単位だけを扱い、軸名、機械換算、入力割当はこの層に閉じる。

use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub(super) const EMBEDDED_PROFILE: &str = include_str!("../../config/rtheta.toml");
const MAX_SLOTS: usize = 3;
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
    pub fn reached(&self, contacts: u8) -> bool {
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
    /// 通常ジョグで停止から最高速度まで加速する時間。0は速度を即時変更。
    #[serde(default)]
    pub jog_ramp_seconds: f32,
    /// 自動ホーミングで使う通常最高速度に対する割合[%]。
    #[serde(
        default = "default_homing_speed_percent",
        skip_serializing_if = "is_default_homing_speed_percent"
    )]
    pub homing_speed_percent: f32,
    /// 原点採用後、リミットから離れる距離。省略時はr=100mm、z=50mm。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homing_retreat_mm: Option<f32>,
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

impl AxisProfile {
    pub fn homing_retreat_mm(&self) -> f32 {
        self.homing_retreat_mm.unwrap_or(match self.name.as_str() {
            "r" => 100.0,
            "z" => 50.0,
            _ => 0.0,
        })
    }
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

fn default_homing_speed_percent() -> f32 {
    20.0
}

fn is_default_homing_speed_percent(value: &f32) -> bool {
    *value == default_homing_speed_percent()
}

fn default_slow_speed_percent() -> f32 {
    20.0
}

fn is_default_slow_speed_percent(value: &f32) -> bool {
    *value == default_slow_speed_percent()
}

/// cctlの実行時パラメータ。名前とidの対応は device_protocol.md の表に従う。
/// FWを書き直さずに実機調整を終えるため、調整対象はすべてここへ書く。
/// パラメータ名から値への対応。名前は device_protocol.md の表に従う。
pub type ParameterMap = std::collections::BTreeMap<String, f32>;

pub const PARAMETER_NAMES: [&str; 43] = [
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
    "reserved_15",
    "reserved_16",
    "reserved_17",
    "reserved_18",
    "reserved_19",
    "reserved_20",
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
    "m3508_slot2_pos_kp",
    "m3508_slot2_pos_ki",
    "m3508_slot2_pos_kd",
    "m3508_slot2_max_rpm",
    "m3508_slot2_vel_kp",
    "m3508_slot2_vel_ki",
    "m3508_slot2_vel_kd",
    "m3508_slot2_max_current_ma",
    "c620_slot2_esc_id",
    "m3508_slot2_max_temperature_c",
];

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct MachineProfile {
    /// L1・画面操作・原点調整で使う通常最高速度に対する割合[%]。
    #[serde(
        default = "default_slow_speed_percent",
        skip_serializing_if = "is_default_slow_speed_percent"
    )]
    pub slow_speed_percent: f32,
    #[serde(default)]
    pub dc_motors: Vec<super::dc_motor::MotorProfile>,
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
    /// コンパイル時に同梱した既定プロファイル。
    pub fn embedded() -> Result<Self> {
        Self::parse(EMBEDDED_PROFILE)
    }

    pub fn parse(source: &str) -> Result<Self> {
        let mut profile: Self =
            toml::from_str(source).context("機体プロファイルの形式が不正です")?;
        profile.sync_motor_speed_limits();
        profile.validate()?;
        Ok(profile)
    }

    /// 操作・シーケンスで使う機体単位の最高速度。
    pub fn effective_axis_speed(&self, axis: &AxisProfile) -> f32 {
        axis.speed_per_second
    }

    /// CCTLが最終段で適用するモータ速度上限を、軸の最高速度から生成する。
    pub fn cctl_parameters(&self) -> ParameterMap {
        let mut values = self.parameters.clone();
        for axis in &self.axes {
            let native_per_second = axis.speed_per_second * axis.native_per_unit.abs();
            match axis.slot {
                0 => {
                    values.insert("el05_limit_spd".into(), native_per_second);
                }
                1 => {
                    values.insert("m3508_max_rpm".into(), native_per_second / 6.0);
                }
                2 => {
                    values.insert("m3508_slot2_max_rpm".into(), native_per_second / 6.0);
                }
                _ => {}
            }
        }
        values
    }

    /// 表示・保存されるCCTL設定も軸速度から生成した値へ揃える。
    pub fn sync_motor_speed_limits(&mut self) {
        self.parameters = self.cctl_parameters();
    }

    pub fn validate(&self) -> Result<()> {
        if !self.slow_speed_percent.is_finite() || !(1.0..=100.0).contains(&self.slow_speed_percent)
        {
            bail!("slow_speed_percentは1..100で指定してください");
        }
        super::dc_motor::validate(&self.dc_motors)?;
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
            (
                &self.svmd_parameters,
                &crate::protocol::svmd::PARAMETER_NAMES,
            ),
            (
                &self.dcmd_parameters,
                &crate::protocol::dcmd::PARAMETER_NAMES,
            ),
            (
                &self.serial_svmd_parameters,
                &crate::protocol::serial_svmd::PARAMETER_NAMES,
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

        if self
            .parameters
            .keys()
            .any(|name| name.starts_with("reserved_"))
        {
            bail!("予約パラメータは機体設定に指定できません");
        }
        if self.parameters.keys().any(|name| name.starts_with("dm_")) {
            bail!("旧DM用設定です。M3508×2台用の機体設定へ移行してください");
        }
        let first = self.parameters.get("c620_esc_id").copied().unwrap_or(1.0);
        let second = self
            .parameters
            .get("c620_slot2_esc_id")
            .copied()
            .unwrap_or(2.0);
        if !(1.0..=8.0).contains(&first) || !(1.0..=8.0).contains(&second) || first == second {
            bail!("C620のESC IDは1〜8で2台を重複なく指定してください");
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
                axis.jog_ramp_seconds,
                axis.homing_speed_percent,
                axis.homing_retreat_mm(),
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
                || axis.jog_ramp_seconds < 0.0
                || !(1.0..=100.0).contains(&axis.homing_speed_percent)
                || axis.homing_retreat_mm() < 0.0
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

    pub fn requires_can_bus_2(&self) -> bool {
        !self.pwm_servos.is_empty() || !self.dc_motors.is_empty() || self.serial_svmd.is_some()
    }

    pub fn requires_serial_svmd(&self) -> bool {
        self.serial_svmd.is_some()
    }
}

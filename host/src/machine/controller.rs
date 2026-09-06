use super::profile::*;
use crate::{
    input::controller::ControllerState,
    protocol::{svmd, telemetry::Telemetry},
};
use serde::Serialize;
const MAX_INPUT_INTERVAL_S: f32 = 0.1;
const STICK_DEADZONE: f32 = 0.1;

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
        lines.extend(crate::protocol::dcmd::targets(
            &self.profile.dc_motors,
            &input.axes,
        ));
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
                crate::protocol::serial_svmd::Command::Target {
                    id: servo.id,
                    position: target.round() as u16,
                    speed: servo.move_speed,
                    acceleration: servo.acceleration,
                }
                .to_cctl_line(),
            );
            lines.push(
                crate::protocol::serial_svmd::Command::Enable {
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
                crate::protocol::serial_svmd::Command::Read {
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
mod tests;

#[cfg(test)]
mod manual_tests;

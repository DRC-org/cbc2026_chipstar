use super::profile::*;
use crate::{input::ControllerState, protocol::telemetry::Telemetry};
use serde::Serialize;
use std::time::Instant;
const STICK_DEADZONE: f32 = 0.1;

/// 軸ごとの原点の状態。GUI 表示用。
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct OriginState {
    pub name: String,
    pub unit: String,
    /// 採用済みだった原点を、通信やフィードバックの途切れで失ったか。
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
    moving: Vec<bool>,
    jog_velocity: Vec<f32>,
    jog_tick: Option<Instant>,
}

impl MachineController {
    pub fn new(profile: MachineProfile) -> Self {
        let targets = profile.axes.iter().map(|axis| axis.initial).collect();
        Self {
            origins_native: vec![0.0; profile.axes.len()],
            origin_captured: vec![false; profile.axes.len()],
            soft_limits: true,
            origin_lost: vec![false; profile.axes.len()],
            moving: vec![false; profile.axes.len()],
            jog_velocity: vec![0.0; profile.axes.len()],
            jog_tick: None,
            profile,
            targets,
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

    /// CCTLが応答途絶を報告した軸だけ原点を失効する。
    /// CCTL再起動と通信断はworker側で全軸を失効する。
    fn check_feedback_continuity(&mut self, telemetry: Option<&Telemetry>) {
        let Some(telemetry) = telemetry else {
            return;
        };
        for index in 0..self.profile.axes.len() {
            let axis = &self.profile.axes[index];
            let stale = telemetry.stale_slots & (1 << axis.slot) != 0;
            if stale && self.origin_captured[index] {
                let measured = telemetry.slots[axis.slot as usize].measured;
                self.origin_captured[index] = false;
                self.origin_lost[index] = true;
                self.origins_native[index] = measured;
                self.targets[index] = 0.0;
            }
        }
    }

    /// 機体座標の可動域で目標を止めるかを切り替える。
    ///
    /// 原点を採り直すときは、いまの原点から見た可動域の外へ動かす必要がある。
    /// 無効化中は接点入力と人間の経路確認が機構保護の前提になる。
    pub fn set_soft_limits(&mut self, enabled: bool) {
        self.soft_limits = enabled;
    }

    /// 原点採用済みの機体座標を、cctlへ渡すネイティブ位置へ変換する。
    pub fn native_position(&self, slot: u8, position: f32) -> Option<f32> {
        let index = self
            .profile
            .axes
            .iter()
            .position(|axis| axis.slot == slot)?;
        self.origin_captured[index].then_some(
            self.origins_native[index] + position * self.profile.axes[index].native_per_unit,
        )
    }

    /// 機体座標の位置目標を保持目標として記録し、ネイティブ位置へ変換する。
    pub fn set_position_target(&mut self, slot: u8, position: f32) -> Option<f32> {
        let index = self
            .profile
            .axes
            .iter()
            .position(|axis| axis.slot == slot)?;
        if !self.origin_captured[index] || !position.is_finite() {
            return None;
        }
        self.targets[index] = position;
        self.moving[index] = false;
        Some(self.origins_native[index] + position * self.profile.axes[index].native_per_unit)
    }

    /// いまの実測位置を目標として取り込む。
    ///
    /// 目標を過去の値のまま RUN すると、機体がその位置まで戻ろうとして跳ねる。
    /// 非常停止で手動退避した後がとくに危ない。位置ループを有効にする直前に
    /// 目標と実測を揃えておけば、RUN してもその場を保持する。
    pub fn hold_at_measured(&mut self, telemetry: Option<&Telemetry>) {
        self.reset_jog();
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
            self.moving[index] = false;
        }
    }

    /// いまの実測位置に `origin_position` を割り当てる。
    ///
    /// 目標値も同じ値へ置き直すので、採用の前後で軸は動かない。
    pub fn capture_origin(&mut self, index: usize, telemetry: Option<&Telemetry>) -> bool {
        let Some(position) = self.profile.axes.get(index).map(|axis| axis.origin_position) else {
            return false;
        };
        self.capture_coordinate(index, telemetry, position)
    }

    /// いまの実測位置へ既知の機体座標を割り当てる。
    ///
    /// host再起動後に、停止直前に記録した座標を動かさず復元するために使う。
    pub fn capture_coordinate(
        &mut self,
        index: usize,
        telemetry: Option<&Telemetry>,
        position: f32,
    ) -> bool {
        let Some(axis) = self.profile.axes.get(index) else {
            return false;
        };
        if !position.is_finite() || !(axis.minimum..=axis.maximum).contains(&position) {
            return false;
        }
        let Some(telemetry) = telemetry else {
            return false;
        };
        let Some(slot) = telemetry.slots.get(axis.slot as usize) else {
            return false;
        };
        if !slot.measured.is_finite() || telemetry.stale_slots & (1 << axis.slot) != 0 {
            return false;
        }
        self.origins_native[index] = slot.measured - position * axis.native_per_unit;
        self.targets[index] = position;
        self.origin_captured[index] = true;
        self.origin_lost[index] = false;
        self.moving[index] = false;
        true
    }

    /// 実測値と接点から原点状態だけを更新する。出力指令は生成しない。
    ///
    /// 出力中の軸は保持目標を実測へ追従させない。負荷で位置がずれたときに
    /// 目標まで一緒にずれると、位置制御が落下を止められないためである。
    pub fn observe(&mut self, telemetry: &Telemetry) {
        self.check_feedback_continuity(Some(telemetry));
        for index in 0..self.profile.axes.len() {
            let axis = &self.profile.axes[index];
            if telemetry.mode == crate::protocol::telemetry::RunMode::Run
                && telemetry.enabled_slots & (1 << axis.slot) != 0
            {
                continue;
            }
            let slot = &telemetry.slots[axis.slot as usize];
            let value = (slot.measured - self.origins_native[index]) / axis.native_per_unit;
            if value.is_finite() {
                self.targets[index] = value;
                self.moving[index] = false;
            }
        }
    }

    pub fn invalidate_origins(&mut self) {
        for i in 0..self.profile.axes.len() {
            self.origin_lost[i] |= self.origin_captured[i];
            self.origin_captured[i] = false;
            self.moving[i] = false;
        }
    }

    pub fn invalidate_origin(&mut self, index: usize) {
        if index < self.origin_captured.len() {
            self.origin_lost[index] |= self.origin_captured[index];
            self.origin_captured[index] = false;
            self.moving[index] = false;
        }
    }

    /// 設定変更後も座標定義が同じ軸の原点オフセットを引き継ぐ。
    pub fn reconfigure(&mut self, profile: MachineProfile) {
        let mut targets = profile
            .axes
            .iter()
            .map(|axis| axis.initial)
            .collect::<Vec<_>>();
        let mut origins_native = vec![0.0; profile.axes.len()];
        let mut captured = vec![false; profile.axes.len()];
        let mut lost = vec![false; profile.axes.len()];
        for (new_index, new_axis) in profile.axes.iter().enumerate() {
            let Some(old_index) = self
                .profile
                .axes
                .iter()
                .position(|old_axis| old_axis.name == new_axis.name)
            else {
                continue;
            };
            let old_axis = &self.profile.axes[old_index];
            let compatible = old_axis.unit == new_axis.unit
                && old_axis.slot == new_axis.slot
                && old_axis.native_per_unit == new_axis.native_per_unit
                && old_axis.origin_position == new_axis.origin_position;
            if compatible {
                targets[new_index] = self.targets[old_index];
                origins_native[new_index] = self.origins_native[old_index];
                captured[new_index] = self.origin_captured[old_index];
                lost[new_index] = self.origin_lost[old_index];
            } else {
                lost[new_index] = self.origin_captured[old_index] || self.origin_lost[old_index];
            }
        }
        self.profile = profile;
        self.targets = targets;
        self.origins_native = origins_native;
        self.origin_captured = captured;
        self.origin_lost = lost;
        self.moving = vec![false; self.profile.axes.len()];
        self.jog_velocity = vec![0.0; self.profile.axes.len()];
        self.jog_tick = None;
    }

    fn constrain_velocity(
        &self,
        index: usize,
        mut velocity: f32,
        telemetry: &Telemetry,
        soft_limits: bool,
    ) -> f32 {
        let axis = &self.profile.axes[index];
        if let Some(limit) = axis.limit
            && (telemetry.contacts.is_none()
                || telemetry.contacts.is_some_and(|contacts| {
                    limit.reached(contacts) && velocity * limit.direction > 0.0
                }))
        {
            velocity = 0.0;
        }
        let measured = (telemetry.slots[axis.slot as usize].measured - self.origins_native[index])
            / axis.native_per_unit;
        if soft_limits {
            if !self.origin_captured[index] {
                return 0.0;
            }
            if velocity > 0.0 {
                velocity = velocity.min(((axis.maximum - measured) / 0.2).max(0.0));
            }
            if velocity < 0.0 {
                velocity = velocity.max(((axis.minimum - measured) / 0.2).min(0.0));
            }
        }
        velocity
    }

    /// 個別速度テストにも通常操縦と同じ接点・可動域制限を適用する。
    /// 原点未採用時は接点制限だけを適用し、原点調整用の診断を妨げない。
    pub fn constrain_test_velocity(
        &self,
        slot: u8,
        native_velocity: f32,
        telemetry: &Telemetry,
    ) -> Option<f32> {
        let index = self
            .profile
            .axes
            .iter()
            .position(|axis| axis.slot == slot)?;
        let axis = &self.profile.axes[index];
        let machine_velocity = native_velocity / axis.native_per_unit;
        let constrained = self.constrain_velocity(
            index,
            machine_velocity,
            telemetry,
            self.origin_captured[index],
        );
        Some(constrained * axis.native_per_unit)
    }

    /// 有効な軸を現在の実測座標で位置保持する指令を作る。
    pub fn hold_lines(&mut self, telemetry: &Telemetry, enabled_slots: u8) -> Vec<String> {
        self.reset_jog();
        let mut lines = Vec::new();
        for index in 0..self.profile.axes.len() {
            let axis = &self.profile.axes[index];
            if enabled_slots & (1 << axis.slot) == 0 {
                continue;
            }
            let measured = telemetry.slots[axis.slot as usize].measured;
            self.targets[index] = (measured - self.origins_native[index]) / axis.native_per_unit;
            self.moving[index] = false;
            lines.push(format!("TARGET {} {measured:.5}", axis.slot));
        }
        lines
    }

    /// 軌道生成を除いた単位変換・保持指令の検証用。
    #[cfg(test)]
    pub fn jog_lines(
        &mut self,
        input: &ControllerState,
        telemetry: &Telemetry,
        slow: bool,
    ) -> Vec<String> {
        self.jog_lines_step(input, telemetry, slow, None)
    }

    pub fn reset_jog(&mut self) {
        self.jog_velocity.fill(0.0);
        self.jog_tick = None;
    }

    pub fn ramped_jog_lines(
        &mut self,
        input: &ControllerState,
        telemetry: &Telemetry,
        slow: bool,
        now: Instant,
    ) -> Vec<String> {
        let dt = self.jog_tick.replace(now).map_or(0.0, |previous| {
            now.saturating_duration_since(previous)
                .as_secs_f32()
                .min(0.05)
        });
        self.jog_lines_step(input, telemetry, slow, Some(dt))
    }

    fn braking_velocity(&self, index: usize, velocity: f32, telemetry: &Telemetry) -> f32 {
        let axis = &self.profile.axes[index];
        let velocity = self.constrain_velocity(index, velocity, telemetry, false);
        if !self.soft_limits {
            return velocity;
        }
        if !self.origin_captured[index] {
            return 0.0;
        }
        let measured = (telemetry.slots[axis.slot as usize].measured - self.origins_native[index])
            / axis.native_per_unit;
        let mut lo = axis.minimum;
        let mut hi = axis.maximum;
        if let Some(limit) = axis.limit {
            if limit.direction > 0.0 {
                hi = hi.min(axis.origin_position);
            } else {
                lo = lo.max(axis.origin_position);
            }
        }
        let distance = if velocity > 0.0 {
            hi - measured
        } else {
            measured - lo
        };
        let acceleration = axis.speed_per_second / axis.jog_ramp_seconds;
        // 受信周期とCCTLのジョグ先行量に0.2秒分を見込んで制動を始める。
        let delay_velocity = acceleration * 0.2;
        let cap = ((delay_velocity * delay_velocity + 2.0 * acceleration * distance.max(0.0))
            .sqrt()
            - delay_velocity)
            .max(0.0);
        velocity.signum() * velocity.abs().min(cap)
    }

    fn jog_lines_step(
        &mut self,
        input: &ControllerState,
        telemetry: &Telemetry,
        slow: bool,
        dt: Option<f32>,
    ) -> Vec<String> {
        let mut lines = Vec::with_capacity(self.profile.axes.len());
        for i in 0..self.profile.axes.len() {
            let axis = &self.profile.axes[i];
            let raw = axis.input_axis.map(|n| input.axes[n]).unwrap_or(0.0);
            let raw = if raw.is_finite() && raw.abs() >= STICK_DEADZONE {
                raw.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            let slow_scale = self.profile.slow_speed_percent * 0.01;
            let mut velocity = raw
                * axis.input_sign
                * self.profile.effective_axis_speed(axis)
                * if slow { slow_scale } else { 1.0 };
            if let Some(dt) = dt.filter(|_| axis.jog_ramp_seconds > 0.0) {
                let desired = self.braking_velocity(i, velocity, telemetry);
                let delta = axis.speed_per_second / axis.jog_ramp_seconds * dt;
                let previous = self.jog_velocity[i];
                let ramped = previous + (desired - previous).clamp(-delta, delta);
                // スイッチ到達・接点不明・可動域外では減速途中でも進入指令を止める。
                velocity = self.braking_velocity(i, ramped, telemetry);
            } else {
                velocity = self.constrain_velocity(i, velocity, telemetry, self.soft_limits);
            }
            self.jog_velocity[i] = velocity;
            let measured = telemetry.slots[axis.slot as usize].measured;
            if velocity != 0.0 {
                self.targets[i] = (measured - self.origins_native[i]) / axis.native_per_unit;
                self.moving[i] = true;
                lines.push(format!(
                    "JOG {} {:.5}",
                    axis.slot,
                    velocity * axis.native_per_unit
                ));
            } else {
                if self.moving[i] {
                    self.targets[i] = (measured - self.origins_native[i]) / axis.native_per_unit;
                    self.moving[i] = false;
                }
                let target = if self.origin_captured[i] {
                    self.origins_native[i] + self.targets[i] * axis.native_per_unit
                } else {
                    measured
                };
                lines.push(format!("TARGET {} {target:.5}", axis.slot));
            }
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

#[cfg(test)]
mod ramp_tests;

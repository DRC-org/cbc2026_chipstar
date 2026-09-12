use super::*;
use crate::machine::{AxisProfile, motion_profile::PositionProfile};
#[cfg(test)]
#[path = "homing_front_tests.rs"]
mod front_tests;
const Z_CLEARANCE_TOLERANCE_MM: f32 = 1.0;
const THETA_TOLERANCE_DEG: f32 = 1.0;

#[derive(Clone, Copy)]
enum Phase {
    Prepare,
    AwaitRun,
    Seek,
    AwaitStop,
    PrepareClearance,
    AwaitClearanceRun,
    MoveClearance,
    AwaitRotationRun,
    Rotate,
    AwaitRadialStop,
    AwaitRetreatRun,
    MoveRadialRetreat,
    AwaitHold,
    AwaitFrontRun,
    ReturnFront,
}
pub(super) struct Homing {
    theta_target: f32,
    rotation_field: Option<f32>,
    stage: usize,
    phase: Phase,
    since: Instant,
    axis_started: Instant,
    pub label: String,
    timeout_seconds: f32,
    motion: Option<PositionProfile>,
}
impl Homing {
    pub(super) fn is_front_return(&self) -> bool {
        matches!(self.phase, Phase::AwaitFrontRun | Phase::ReturnFront)
    }
}
impl Runtime {
    pub(super) fn begin_front_return(&mut self, timeout_seconds: f32) {
        let now = Instant::now();
        self.homing = Some(Homing {
            theta_target: 0.0,
            rotation_field: None,
            stage: 0,
            phase: Phase::AwaitFrontRun,
            since: now,
            axis_started: now,
            label: "正面0°へ復帰・運転応答待ち".into(),
            timeout_seconds,
            motion: None,
        });
    }

    pub(super) fn homing_idle(&self) -> bool {
        !self.emergency
            && !self.drive.running()
            && self.drive.awaiting().is_none()
            && !self.test.enabled
            && !self.sts.active
            && !self.sts.busy()
            && self.homing.is_none()
    }

    pub(super) fn begin_homing(
        &mut self,
        human: bool,
        confirmed: bool,
        timeout_seconds: f32,
    ) -> Result<Reply> {
        anyhow::ensure!(
            timeout_seconds.is_finite() && (10.0..=1800.0).contains(&timeout_seconds),
            "制限時間は10〜1800秒です"
        );
        anyhow::ensure!(
            human && confirmed,
            "機体が真正面を向き、z下降・上昇・θ旋回・r前進・後退の経路に干渉がないことを確認してください"
        );
        anyhow::ensure!(
            self.homing_idle(),
            "全操作を停止してからホーミングしてください"
        );
        anyhow::ensure!(
            !self.preparation.locked() && self.sequence.is_none(),
            "準備画面で停止してから開始してください"
        );
        let theta_target = self
            .court
            .context("赤コートか青コートを選んでください")?
            .homing_theta(self.cfg.machine.homing_theta_deg);
        self.axes_ready(false)?;
        let theta_index = self
            .cfg
            .machine
            .axes
            .iter()
            .position(|a| a.name == "theta")
            .context("θ軸の設定が必要です")?;
        let theta = &self.cfg.machine.axes[theta_index];
        anyhow::ensure!(
            theta.unit == "deg" && theta.origin_position == 0.0,
            "θの単位をdeg、原点座標を0に設定してください"
        );
        anyhow::ensure!(
            (theta.minimum..=theta.maximum).contains(&theta_target)
                && (theta.minimum..=theta.maximum).contains(&0.0),
            "コート別の旋回位置がθの可動域外です"
        );
        anyhow::ensure!(
            self.cfg.machine.effective_axis_speed(theta) > 0.0,
            "θの旋回速度を設定してください"
        );
        for name in ["z", "r"] {
            let a = self
                .cfg
                .machine
                .axes
                .iter()
                .find(|a| a.name == name)
                .context("r・z軸の設定が必要です")?;
            let limit = a.limit.context("r・zのリミットスイッチ設定が必要です")?;
            anyhow::ensure!(
                limit.direction == if name == "z" { -1.0 } else { 1.0 },
                "zは負方向、rは正方向のリミット設定が必要です"
            );
            anyhow::ensure!(
                a.speed_per_second > 0.0,
                "ホーミングには正の軸速度が必要です"
            );
            if name == "r" {
                anyhow::ensure!(a.unit == "mm", "r軸の単位はmmにしてください");
                anyhow::ensure!(
                    a.homing_retreat_mm() > 0.0,
                    "rのホーミング戻し量を0より大きく設定してください"
                );
                anyhow::ensure!(
                    (a.minimum..=a.maximum).contains(&(a.origin_position - a.homing_retreat_mm())),
                    "r原点から設定量だけ後退した位置が可動域外です"
                );
            }
            if name == "z" {
                anyhow::ensure!(a.unit == "mm", "z軸の単位はmmにしてください");
                anyhow::ensure!(
                    a.homing_retreat_mm() > 0.0,
                    "θ旋回前のz上昇量を0より大きく設定してください"
                );
                let clearance = a.origin_position + a.homing_retreat_mm();
                anyhow::ensure!(
                    (a.minimum..=a.maximum).contains(&clearance),
                    "z原点から設定量だけ上昇した位置が可動域外です"
                );
            }
        }
        anyhow::ensure!(
            self.telemetry
                .as_ref()
                .is_some_and(|t| t.contacts.is_some()),
            "接点情報が取得できません"
        );
        anyhow::ensure!(
            self.machine
                .capture_origin(theta_index, self.telemetry.as_ref()),
            "正面のθ位置を原点に設定できません"
        );
        let rotation_field = self.measured_rotation_field(Instant::now())?;
        for name in ["r", "z"] {
            let index = self
                .cfg
                .machine
                .axes
                .iter()
                .position(|a| a.name == name)
                .unwrap();
            self.machine.invalidate_origin(index);
        }
        self.stop(true)?;
        self.send("SAFE")?;
        let homing_slots = self
            .cfg
            .machine
            .axes
            .iter()
            .filter(|axis| axis.name == "r" || axis.name == "z")
            .fold(0u8, |mask, axis| mask | 1 << axis.slot);
        self.send(&format!("REINIT {homing_slots}"))?;
        let now = Instant::now();
        self.homing = Some(Homing {
            theta_target,
            rotation_field,
            timeout_seconds,
            stage: 0,
            phase: Phase::Prepare,
            since: now,
            axis_started: now,
            label: "z下端へホーミング".into(),
            motion: None,
        });
        Ok(Reply::accepted())
    }

    fn begin_homing_move(
        &mut self,
        axis: &AxisProfile,
        target: f32,
        speed_scale: f32,
        now: Instant,
    ) -> Result<()> {
        let from = self
            .machine
            .axis_position(&axis.name, self.telemetry.as_ref())
            .context("ホーミング移動の開始位置を確認できません")?;
        let motion = PositionProfile::new(
            from,
            target,
            axis.speed_per_second * speed_scale,
            axis.jog_ramp_seconds * speed_scale,
        );
        let home = self.homing.as_mut().unwrap();
        home.motion = Some(motion);
        home.axis_started = now;
        self.send_homing_position(axis.slot, from)
    }

    fn send_homing_position(&mut self, slot: u8, position: f32) -> Result<()> {
        let native = self
            .machine
            .set_position_target(slot, position)
            .context("ホーミング移動中に原点を失いました")?;
        self.send(&format!("TARGET {slot} {native:.5}"))
    }

    fn advance_homing_move(&mut self, axis: &AxisProfile, now: Instant) -> Result<bool> {
        let home = self.homing.as_ref().unwrap();
        let elapsed = now
            .saturating_duration_since(home.axis_started)
            .as_secs_f32();
        anyhow::ensure!(
            elapsed < home.timeout_seconds,
            "{}が設定した位置に到達せず時間超過しました",
            axis.name
        );
        let motion = home.motion.context("ホーミング移動の軌道がありません")?;
        self.send_homing_position(axis.slot, motion.position(elapsed))?;
        Ok(elapsed >= motion.duration())
    }

    pub(super) fn tick_homing(&mut self, now: Instant) -> Result<()> {
        let Some(home) = &self.homing else {
            return Ok(());
        };
        anyhow::ensure!(
            !self.emergency && self.fresh() && self.settings.ready(),
            "ホーミング中に通信・設定状態を失いました"
        );
        let t = self.telemetry.as_ref().context("実測なし")?.clone();
        let contacts = t.contacts.context("接点情報を失いました")?;
        let stage = home.stage;
        let phase = home.phase;
        let elapsed = now.duration_since(home.since);
        let theta_index = self
            .cfg
            .machine
            .axes
            .iter()
            .position(|a| a.name == "theta")
            .context("θ軸の設定が必要です")?;
        let theta = self.cfg.machine.axes[theta_index].clone();
        let z = self
            .cfg
            .machine
            .axes
            .iter()
            .find(|a| a.name == "z")
            .context("z軸の設定が必要です")?
            .clone();
        let holding = (1 << theta.slot) | (1 << z.slot);
        if home.is_front_return() {
            if matches!(phase, Phase::AwaitFrontRun) {
                if self.drive.running() {
                    self.begin_homing_move(&theta, 0.0, 1.0, now)?;
                    let home = self.homing.as_mut().unwrap();
                    home.phase = Phase::ReturnFront;
                    home.label = "正面0°へ加減速して復帰中（r・z保持）".into();
                } else {
                    anyhow::ensure!(
                        elapsed < Duration::from_millis(500),
                        "正面復帰の運転応答がありません"
                    );
                }
                return Ok(());
            }
            self.ready()?;
            anyhow::ensure!(self.drive.running(), "正面復帰中に運転状態を失いました");
            let position = self
                .machine
                .axis_position("theta", Some(&t))
                .context("正面復帰中にθ原点を失いました")?;
            let finished = self.advance_homing_move(&theta, now)?;
            if finished && position.abs() <= THETA_TOLERANCE_DEG {
                self.homing = None;
                self.machine.reset_jog();
                self.manual_input = ControllerState::default();
                self.authority.clear_input();
                self.guide.reset_input();
                self.pad.ee_armed = false;
                self.reason = "正面復帰完了・運転中".into();
            }
            return Ok(());
        }
        if matches!(
            phase,
            Phase::AwaitHold | Phase::AwaitRotationRun | Phase::Rotate
        ) || stage == 1
        {
            anyhow::ensure!(
                t.stale_slots & holding == 0
                    && t.error_bits[usize::from(theta.slot)] == 0
                    && t.error_bits[usize::from(z.slot)] == 0,
                "θ・zの保持中にフィードバック異常を検出しました"
            );
        }
        if matches!(phase, Phase::AwaitRadialStop) {
            let index = self
                .cfg
                .machine
                .axes
                .iter()
                .position(|a| a.name == "r")
                .unwrap();
            let radial = self.cfg.machine.axes[index].clone();
            anyhow::ensure!(
                radial.limit.is_some_and(|l| l.reached(contacts))
                    && t.stale_slots & (1 << radial.slot) == 0
                    && t.error_bits[usize::from(radial.slot)] == 0,
                "r停止時にリミットまたはフィードバックを失いました"
            );
            if t.mode == RunMode::Run && t.enabled_slots == holding {
                anyhow::ensure!(
                    self.machine.capture_origin(index, Some(&t)),
                    "rの停止位置を原点に設定できません"
                );
                self.send(&format!("ENABLE {} 1", 1 << radial.slot))?;
                let h = self.homing.as_mut().unwrap();
                h.phase = Phase::AwaitRetreatRun;
                h.since = now;
                h.label = "rの原点を設定しました。設定した戻し位置へ後退します".into();
            } else {
                anyhow::ensure!(
                    elapsed < Duration::from_millis(500),
                    "ホーミング完了時の保持応答がありません"
                );
            }
            return Ok(());
        }
        if matches!(
            phase,
            Phase::AwaitRetreatRun | Phase::MoveRadialRetreat | Phase::AwaitHold
        ) {
            let index = self
                .cfg
                .machine
                .axes
                .iter()
                .position(|a| a.name == "r")
                .unwrap();
            let radial = self.cfg.machine.axes[index].clone();
            let enabled = holding | (1 << radial.slot);
            anyhow::ensure!(
                t.stale_slots & enabled == 0 && t.error_bits[usize::from(radial.slot)] == 0,
                "r後退中にフィードバック異常を検出しました"
            );
            if matches!(phase, Phase::AwaitHold) {
                if t.mode == RunMode::Run && t.enabled_slots == holding {
                    self.prepared_rotation_field = home.rotation_field;
                    self.front_return_pending = Some(home.timeout_seconds);
                    self.homing = None;
                    self.reason = "ホーミング完了。rを後退し、θ・zを保持しています".into();
                } else {
                    anyhow::ensure!(
                        elapsed < Duration::from_millis(500),
                        "ホーミング完了時の保持応答がありません"
                    );
                }
                return Ok(());
            }
            let target = radial.origin_position - radial.homing_retreat_mm();
            if matches!(phase, Phase::AwaitRetreatRun) {
                if t.mode == RunMode::Run && t.enabled_slots == enabled {
                    self.begin_homing_move(&radial, target, 1.0, now)?;
                    let h = self.homing.as_mut().unwrap();
                    h.phase = Phase::MoveRadialRetreat;
                    h.since = now;
                    h.axis_started = now;
                    h.label = "rを設定した戻し位置へ後退しています（θ・z保持中）".into();
                } else {
                    anyhow::ensure!(
                        elapsed < Duration::from_millis(500),
                        "r後退開始の応答がありません"
                    );
                }
                return Ok(());
            }
            anyhow::ensure!(
                t.mode == RunMode::Run && t.enabled_slots == enabled,
                "r後退中の出力状態が変化しました"
            );
            let origin = &self.machine.origin_states(Some(&t))[index];
            anyhow::ensure!(origin.captured, "r後退中に原点を失いました");
            let finished = self.advance_homing_move(&radial, now)?;
            if finished && (origin.position - target).abs() <= 1.0 {
                anyhow::ensure!(
                    !radial.limit.unwrap().reached(contacts),
                    "r後退後もリミットが作動しています"
                );
                self.send(&format!("ENABLE {} 0", 1 << radial.slot))?;
                let h = self.homing.as_mut().unwrap();
                h.phase = Phase::AwaitHold;
                h.since = now;
                h.label = "r後退完了。停止を確認しています（θ・z保持中）".into();
            }
            return Ok(());
        }
        if matches!(phase, Phase::AwaitRotationRun | Phase::Rotate) {
            let theta_target = home.theta_target;
            if matches!(phase, Phase::AwaitRotationRun) {
                if t.mode == RunMode::Run && t.enabled_slots == holding {
                    self.begin_homing_move(
                        &theta,
                        theta_target,
                        theta.homing_speed_percent * 0.01,
                        now,
                    )?;
                    let h = self.homing.as_mut().unwrap();
                    h.phase = Phase::Rotate;
                    h.since = now;
                    h.axis_started = now;
                } else {
                    anyhow::ensure!(
                        elapsed < Duration::from_millis(500),
                        "θ旋回開始の応答がありません"
                    );
                }
                return Ok(());
            }
            anyhow::ensure!(
                t.mode == RunMode::Run && t.enabled_slots == holding,
                "θ旋回中の出力状態が変化しました"
            );
            let origin = &self.machine.origin_states(Some(&t))[theta_index];
            anyhow::ensure!(origin.captured, "θ旋回中に原点を失いました");
            let error = theta_target - origin.position;
            let finished = self.advance_homing_move(&theta, now)?;
            if finished && error.abs() <= THETA_TOLERANCE_DEG {
                let radial = self
                    .cfg
                    .machine
                    .axes
                    .iter()
                    .find(|a| a.name == "r")
                    .unwrap();
                self.send(&format!("ENABLE {} 1", 1 << radial.slot))?;
                let h = self.homing.as_mut().unwrap();
                h.stage = 1;
                h.phase = Phase::AwaitRun;
                h.since = now;
                h.axis_started = now;
                h.label = "rを前端まで伸ばしています（θ・z保持中）".into();
            }
            return Ok(());
        }
        if matches!(
            phase,
            Phase::PrepareClearance | Phase::AwaitClearanceRun | Phase::MoveClearance
        ) {
            let index = self
                .cfg
                .machine
                .axes
                .iter()
                .position(|a| a.name == "z")
                .context("z軸の設定が必要です")?;
            let axis = self.cfg.machine.axes[index].clone();
            let bit = 1 << axis.slot;
            let target = axis.origin_position + axis.homing_retreat_mm();
            anyhow::ensure!(
                t.stale_slots & bit == 0 && t.error_bits[usize::from(axis.slot)] == 0,
                "z上昇中のフィードバック異常"
            );
            if matches!(phase, Phase::PrepareClearance) {
                anyhow::ensure!(t.mode != RunMode::Run, "z上昇前の停止を確認できません");
                self.send("ENABLE 7 0")?;
                self.send(&format!("ENABLE {bit} 1"))?;
                self.send("RUN")?;
                let h = self.homing.as_mut().unwrap();
                h.phase = Phase::AwaitClearanceRun;
                h.since = now;
                return Ok(());
            }
            if matches!(phase, Phase::AwaitClearanceRun) {
                if t.mode == RunMode::Run && t.enabled_slots == bit {
                    self.begin_homing_move(&axis, target, 1.0, now)?;
                    let h = self.homing.as_mut().unwrap();
                    h.phase = Phase::MoveClearance;
                    h.since = now;
                    h.axis_started = now;
                } else {
                    anyhow::ensure!(
                        elapsed < Duration::from_millis(500),
                        "z上昇開始の応答がありません"
                    );
                }
                return Ok(());
            }
            anyhow::ensure!(
                t.mode == RunMode::Run && t.enabled_slots == bit,
                "z上昇中の出力状態が変化しました"
            );
            let position = self.machine.origin_states(Some(&t))[index].position;
            let finished = self.advance_homing_move(&axis, now)?;
            if finished && (position - target).abs() <= Z_CLEARANCE_TOLERANCE_MM {
                self.send(&format!("ENABLE {} 1", 1 << theta.slot))?;
                let h = self.homing.as_mut().unwrap();
                h.phase = Phase::AwaitRotationRun;
                h.since = now;
                h.axis_started = now;
                h.label = format!("θを{:+.1}°へ旋回しています（z保持中）", h.theta_target);
            }
            return Ok(());
        }
        if matches!(phase, Phase::AwaitStop) {
            if t.mode != RunMode::Run {
                let name = if stage == 0 { "z" } else { "r" };
                let index = self
                    .cfg
                    .machine
                    .axes
                    .iter()
                    .position(|a| a.name == name)
                    .context("ホーミング対象なし")?;
                let axis = &self.cfg.machine.axes[index];
                anyhow::ensure!(
                    axis.limit.is_some_and(|l| l.reached(contacts))
                        && t.error_bits[usize::from(axis.slot)] == 0,
                    "{name}の停止時にリミットまたは実測状態が変化しました"
                );
                anyhow::ensure!(
                    self.machine.capture_origin(index, Some(&t)),
                    "{name}の停止位置を原点採用できません"
                );
                if stage == 1 {
                    self.homing = None;
                    self.reason = "r・zホーミング完了。停止を維持しています".into();
                    return Ok(());
                }
                let h = self.homing.as_mut().unwrap();
                h.phase = Phase::PrepareClearance;
                h.since = now;
                h.axis_started = now;
                h.label = "zを設定した戻し位置へ上昇".into();
            } else {
                anyhow::ensure!(
                    elapsed < Duration::from_millis(500),
                    "ホーミング停止の応答がありません"
                );
            }
            return Ok(());
        }
        let name = if stage == 0 { "z" } else { "r" };
        let index = self
            .cfg
            .machine
            .axes
            .iter()
            .position(|a| a.name == name)
            .context("ホーミング対象なし")?;
        let axis = self.cfg.machine.axes[index].clone();
        let limit = axis.limit.context("リミット未設定")?;
        let bit = 1 << axis.slot;
        let enabled = if stage == 0 {
            bit
        } else {
            let z = self
                .cfg
                .machine
                .axes
                .iter()
                .find(|axis| axis.name == "z")
                .context("z軸の設定が必要です")?;
            bit | (1 << z.slot) | (1 << theta.slot)
        };
        if stage == 1 {
            let z = self
                .cfg
                .machine
                .axes
                .iter()
                .find(|axis| axis.name == "z")
                .context("z軸の設定が必要です")?;
            let z_bit = 1 << z.slot;
            anyhow::ensure!(
                t.stale_slots & z_bit == 0 && t.error_bits[usize::from(z.slot)] == 0,
                "rホーミング中にz保持状態を失いました"
            );
        }
        anyhow::ensure!(
            t.stale_slots & bit == 0 && t.error_bits[usize::from(axis.slot)] == 0,
            "{name}のフィードバック異常"
        );
        if matches!(phase, Phase::Prepare) {
            // STOP/SAFEの送信直後は、z保持中のRUN応答が残っている。
            // 停止が観測されるまで次の駆動指令を送らない。
            if t.mode == RunMode::Run {
                anyhow::ensure!(
                    elapsed < Duration::from_millis(500),
                    "ホーミング開始前の停止応答がありません"
                );
                return Ok(());
            }
            if limit.reached(contacts) {
                anyhow::ensure!(
                    self.machine.capture_origin(index, Some(&t)),
                    "{name}原点を採用できません"
                );
                self.send("STOP")?;
                let h = self.homing.as_mut().unwrap();
                h.phase = Phase::AwaitStop;
                h.since = now;
                return Ok(());
            }
            self.send("ENABLE 7 0")?;
            self.send(&format!("ENABLE {bit} 1"))?;
            self.send("RUN")?;
            let h = self.homing.as_mut().unwrap();
            h.phase = Phase::AwaitRun;
            h.since = now;
            return Ok(());
        }
        if matches!(phase, Phase::AwaitRun) {
            if t.mode == RunMode::Run && t.enabled_slots == enabled {
                let h = self.homing.as_mut().unwrap();
                h.phase = Phase::Seek;
                h.since = now;
            } else {
                anyhow::ensure!(
                    elapsed < Duration::from_millis(500),
                    "{name}ホーミング開始の応答がありません"
                );
            }
            return Ok(());
        }
        anyhow::ensure!(
            t.mode == RunMode::Run && t.enabled_slots == enabled,
            "ホーミング中の出力状態が変化しました"
        );
        if limit.reached(contacts) {
            self.send(&format!("JOG {} 0", axis.slot))?;
            let next_phase = if stage == 0 {
                self.send("STOP")?;
                Phase::AwaitStop
            } else {
                self.send(&format!("ENABLE {bit} 0"))?;
                Phase::AwaitRadialStop
            };
            let h = self.homing.as_mut().unwrap();
            h.phase = next_phase;
            h.since = now;
            h.axis_started = now;
            if stage == 1 {
                h.label = "r前端で停止を確認しています（θ・z保持中）".into();
            }
        } else {
            let speed =
                self.cfg.machine.effective_axis_speed(&axis) * axis.homing_speed_percent * 0.01;
            let timeout = home.timeout_seconds;
            anyhow::ensure!(
                now.duration_since(home.axis_started).as_secs_f32() < timeout,
                "{name}のリミットに到達せず時間超過しました"
            );
            self.send(&format!(
                "JOG {} {:.5}",
                axis.slot,
                limit.direction * speed * axis.native_per_unit
            ))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rehoming_waits_for_stop_response_before_enabling_motors() {
        let mut r = crate::application::worker::tests::screen_runtime();
        let t = r.telemetry.as_mut().unwrap();
        t.mode = RunMode::Run;
        t.enabled_slots = 4;
        t.contacts = Some(0);
        r.begin_homing(true, true, 180.0).unwrap();
        let now = r.homing.as_ref().unwrap().since;
        let before = r.shared.status_snapshot().logs;
        r.tick_homing(now).unwrap();
        r.tick_homing(now + Duration::from_millis(499)).unwrap();
        assert_eq!(r.shared.status_snapshot().logs, before);
        assert!(matches!(r.homing.as_ref().unwrap().phase, Phase::Prepare));
        r.telemetry.as_mut().unwrap().mode = RunMode::Safe;
        r.tick_homing(now + Duration::from_millis(499)).unwrap();
        assert!(matches!(r.homing.as_ref().unwrap().phase, Phase::AwaitRun));
        let logs = r.shared.status_snapshot().logs;
        assert!(logs.iter().any(|line| line == "TX ENABLE 4 1"));
        assert!(logs.iter().any(|line| line == "TX RUN"));
    }

    #[test]
    fn rehoming_fails_if_stop_response_never_arrives() {
        let mut r = crate::application::worker::tests::screen_runtime();
        r.telemetry.as_mut().unwrap().mode = RunMode::Run;
        r.begin_homing(true, true, 180.0).unwrap();
        let now = r.homing.as_ref().unwrap().since;
        let error = r.tick_homing(now + Duration::from_millis(500)).unwrap_err();
        assert!(error.to_string().contains("停止応答がありません"));
        r.fault(error.to_string());
        assert!(r.homing.is_none());
        assert!(!r.drive.running());
        let logs = r.shared.status_snapshot().logs;
        assert!(!logs.iter().any(|line| line == "TX RUN"));
        assert_eq!(logs.iter().filter(|line| *line == "TX STOP").count(), 2);
    }
    #[test]
    fn homes_z_then_rotates_for_each_court_then_sets_r_at_limit() {
        for court in [Court::Red, Court::Blue] {
            check_homing_retreat(court, None, None, 90.0);
            check_homing_retreat(court, Some(25.0), Some(60.0), 90.0);
        }
    }

    #[test]
    fn homes_with_configured_rotation_angle_for_both_courts() {
        for court in [Court::Red, Court::Blue] {
            check_homing_retreat(court, Some(25.0), Some(60.0), 83.5);
        }
    }

    fn check_homing_retreat(
        court: Court,
        z_distance: Option<f32>,
        r_distance: Option<f32>,
        angle: f32,
    ) {
        let mut r = crate::application::worker::tests::screen_runtime();
        r.court = Some(court);
        r.cfg.machine.homing_theta_deg = angle;
        let expected_theta = if court == Court::Blue { angle } else { -angle };
        r.cfg.machine.serial_svmd = MachineProfile::embedded().unwrap().serial_svmd;
        let feedback_at = Instant::now();
        r.test.peers.insert("sts", feedback_at);
        r.servo_feedback.insert(
            1,
            ServoFeedback {
                absolute_position: true,
                seen: feedback_at,
                position: 1500,
                error: 0,
                detail: String::new(),
            },
        );
        let z = r
            .cfg
            .machine
            .axes
            .iter_mut()
            .find(|a| a.name == "z")
            .unwrap();
        z.homing_retreat_mm = z_distance;
        let z_distance = z.homing_retreat_mm();
        z.speed_per_second = 100.0;
        z.homing_speed_percent = 10.0;
        z.native_per_unit = 2.0;
        let radial = r
            .cfg
            .machine
            .axes
            .iter_mut()
            .find(|a| a.name == "r")
            .unwrap();
        radial.homing_retreat_mm = r_distance;
        let r_distance = radial.homing_retreat_mm();
        radial.speed_per_second = 100.0;
        radial.homing_speed_percent = 25.0;
        radial.native_per_unit = 0.5;
        r.cfg
            .machine
            .parameters
            .insert("el05_limit_spd".into(), 1000.0);
        r.cfg
            .machine
            .parameters
            .insert("m3508_slot2_max_rpm".into(), 1000.0);
        for slot in &mut r.telemetry.as_mut().unwrap().slots {
            slot.target = 0.0;
            slot.measured = 0.0;
        }
        r.machine = crate::machine::MachineController::new(r.cfg.machine.clone());
        for index in 0..r.cfg.machine.axes.len() {
            assert!(r.machine.capture_origin(index, r.telemetry.as_ref()));
        }
        assert!(r.begin_homing(false, true, 180.0).is_err());
        assert!(r.begin_homing(true, false, 180.0).is_err());
        r.screen_control = false;
        let pressed_at = Instant::now();
        let mut create = ControllerState::default();
        create.buttons[4] = 1;
        r.read_pad(ControllerState::default(), pressed_at).unwrap();
        r.read_pad(create.clone(), pressed_at).unwrap();
        r.read_pad(create.clone(), pressed_at + Duration::from_secs(1))
            .unwrap();
        assert!(r.homing.is_some());
        assert!(r.request(&Request::new("run"), true).is_err());
        let now = Instant::now();
        {
            let t = r.telemetry.as_mut().unwrap();
            t.mode = RunMode::Stop;
            t.contacts = Some(0);
        }
        r.tick_homing(now).unwrap();
        {
            let t = r.telemetry.as_mut().unwrap();
            t.mode = RunMode::Run;
            t.enabled_slots = 4;
        }
        r.tick_homing(now + Duration::from_millis(50)).unwrap();
        r.tick_homing(now + Duration::from_millis(100)).unwrap();
        r.telemetry.as_mut().unwrap().contacts = Some(2);
        r.tick_homing(now + Duration::from_millis(150)).unwrap();
        r.telemetry.as_mut().unwrap().mode = RunMode::Stop;
        r.tick_homing(now + Duration::from_millis(200)).unwrap();
        r.tick_homing(now + Duration::from_millis(250)).unwrap();
        {
            let t = r.telemetry.as_mut().unwrap();
            t.mode = RunMode::Run;
            t.enabled_slots = 4;
        }
        r.tick_homing(now + Duration::from_millis(300)).unwrap();
        let mut now = now + Duration::from_millis(300);
        {
            let t = r.telemetry.as_mut().unwrap();
            t.slots[2].measured = z_distance * 2.0;
            t.contacts = Some(0);
        }
        r.tick_homing(now + Duration::from_millis(50)).unwrap();
        // 実測だけが先に到達しても、減速を終えるまで次の軸へ進まない。
        assert!(matches!(
            r.homing.as_ref().unwrap().phase,
            Phase::MoveClearance
        ));
        now = finish_homing_move(&mut r);
        // z到達後もrは有効にせず、θだけを追加する。
        assert!(matches!(
            r.homing.as_ref().unwrap().phase,
            Phase::AwaitRotationRun
        ));
        assert!(
            !r.shared
                .status_snapshot()
                .logs
                .iter()
                .any(|line| line == "TX ENABLE 1 1")
        );
        r.telemetry.as_mut().unwrap().enabled_slots = 6;
        now += Duration::from_millis(50);
        r.tick_homing(now).unwrap();
        r.tick_homing(now + Duration::from_millis(50)).unwrap();
        assert!(
            !r.shared
                .status_snapshot()
                .logs
                .iter()
                .any(|line| line == "TX ENABLE 1 1")
        );
        let theta = r
            .cfg
            .machine
            .axes
            .iter()
            .find(|a| a.name == "theta")
            .unwrap();
        let expected_sign = (expected_theta * theta.native_per_unit).signum();
        let theta_native = theta.native_per_unit * expected_theta;
        let turn = r
            .shared
            .status_snapshot()
            .logs
            .iter()
            .find_map(|line| {
                line.strip_prefix("TX TARGET 1 ")
                    .and_then(|value| value.parse::<f32>().ok())
                    .filter(|value| value.abs() > 0.0001)
            })
            .unwrap();
        assert_eq!(turn.signum(), expected_sign);
        r.telemetry.as_mut().unwrap().slots[1].measured = theta_native;
        now = finish_homing_move(&mut r);
        r.telemetry.as_mut().unwrap().enabled_slots = 7;
        r.tick_homing(now + Duration::from_millis(50)).unwrap();
        r.tick_homing(now + Duration::from_millis(100)).unwrap();
        r.telemetry.as_mut().unwrap().contacts = Some(1);
        r.tick_homing(now + Duration::from_millis(150)).unwrap();
        assert!(!r.machine.origin_states(r.telemetry.as_ref())[0].captured);
        r.telemetry.as_mut().unwrap().enabled_slots = 6;
        r.tick_homing(now + Duration::from_millis(200)).unwrap();
        assert!(matches!(
            r.homing.as_ref().unwrap().phase,
            Phase::AwaitRetreatRun
        ));
        r.telemetry.as_mut().unwrap().enabled_slots = 7;
        r.tick_homing(now + Duration::from_millis(250)).unwrap();
        r.tick_homing(now + Duration::from_millis(300)).unwrap();
        assert!(r.homing.is_some());
        r.telemetry.as_mut().unwrap().slots[0].measured = -r_distance * 0.5;
        r.telemetry.as_mut().unwrap().contacts = Some(0);
        now = finish_homing_move(&mut r);
        assert!(matches!(r.homing.as_ref().unwrap().phase, Phase::AwaitHold));
        r.telemetry.as_mut().unwrap().enabled_slots = 6;
        r.tick_homing(now + Duration::from_millis(50)).unwrap();
        assert!(r.homing.is_none());
        let axis = crate::machine::ee::axes(&r.cfg.machine)
            .into_iter()
            .find(|axis| axis.name == "ee_rotation")
            .unwrap();
        let expected_field = (1500.0 - axis.zero0_count) / axis.counts_per_deg;
        assert!((r.prepared_rotation_field.unwrap() - expected_field).abs() < f32::EPSILON);
        assert!(!r.drive.running());
        assert_eq!(r.front_return_pending, Some(180.0));
        let logs = r.shared.status_snapshot().logs;
        assert!(logs.iter().any(|l| l.contains("JOG 2 -20.00000")));
        assert!(logs.iter().any(|l| l.contains("TX REINIT 5")));
        assert!(
            logs.iter()
                .any(|l| l.contains(&format!("TARGET 2 {:.5}", z_distance * 2.0)))
        );
        assert!(logs.iter().any(|l| l.contains("JOG 0 12.50000")));
        assert!(
            logs.iter()
                .any(|l| l == &format!("TX TARGET 0 {:.5}", -r_distance * 0.5))
        );
        assert!(logs.iter().any(|l| l.contains("ENABLE 1 0")));
        assert!(
            logs.iter()
                .any(|l| l == &format!("TX TARGET 1 {theta_native:.5}"))
        );
        let origins = r.machine.origin_states(r.telemetry.as_ref());
        let r_origin = r
            .cfg
            .machine
            .axes
            .iter()
            .find(|a| a.name == "r")
            .unwrap()
            .origin_position;
        assert!(
            (origins.iter().find(|a| a.name == "r").unwrap().position - (r_origin - r_distance))
                .abs()
                < 0.001
        );
        assert_eq!(
            origins.iter().find(|a| a.name == "z").unwrap().position,
            z_distance
        );
        assert_eq!(
            origins.iter().find(|a| a.name == "z").unwrap().target,
            z_distance
        );
        assert_eq!(
            origins.iter().find(|a| a.name == "theta").unwrap().position,
            expected_theta
        );
        assert_eq!(
            origins.iter().find(|a| a.name == "theta").unwrap().target,
            expected_theta
        );
        r.publish();
        assert!(r.shared.status_snapshot().homing_ready);
        let before = r.shared.status_snapshot().logs;
        r.read_pad(create.clone(), pressed_at + Duration::from_secs(5))
            .unwrap();
        r.read_pad(create, pressed_at + Duration::from_secs(7))
            .unwrap();
        assert!(r.homing.is_none());
        assert_eq!(r.shared.status_snapshot().logs, before);
        assert_eq!(r.telemetry.as_ref().unwrap().mode, RunMode::Run);
        assert_eq!(r.telemetry.as_ref().unwrap().enabled_slots, 6);
        r.stop(false).unwrap();
        r.begin_homing(true, true, 180.0).unwrap();
        assert!(r.homing.is_some());
    }

    fn finish_homing_move(runtime: &mut Runtime) -> Instant {
        let home = runtime.homing.as_ref().unwrap();
        let at = home.axis_started
            + Duration::from_secs_f32(home.motion.unwrap().duration())
            + Duration::from_millis(1);
        runtime.tick_homing(at).unwrap();
        at
    }

    #[test]
    fn homing_position_commands_ramp_and_wait_for_actual_arrival() {
        for (name, phase, enabled, stage) in [
            ("r", Phase::AwaitRetreatRun, 7, 1),
            ("z", Phase::AwaitClearanceRun, 4, 0),
            ("theta", Phase::AwaitRotationRun, 6, 0),
        ] {
            let mut r = crate::application::worker::tests::screen_runtime();
            let index = r
                .cfg
                .machine
                .axes
                .iter()
                .position(|axis| axis.name == name)
                .unwrap();
            let axis = &mut r.cfg.machine.axes[index];
            axis.speed_per_second = 80.0;
            axis.jog_ramp_seconds = 0.8;
            axis.homing_speed_percent = 50.0;
            axis.native_per_unit = -2.0;
            if name != "theta" {
                axis.homing_retreat_mm = Some(100.0);
            }
            let axis = axis.clone();
            r.machine = crate::machine::MachineController::new(r.cfg.machine.clone());
            r.begin_homing(true, true, 180.0).unwrap();
            for index in 0..3 {
                assert!(r.machine.capture_origin(index, r.telemetry.as_ref()));
            }
            let now = Instant::now();
            let home = r.homing.as_mut().unwrap();
            home.phase = phase;
            home.stage = stage;
            home.since = now;
            let t = r.telemetry.as_mut().unwrap();
            t.mode = RunMode::Run;
            t.enabled_slots = enabled;
            t.contacts = Some(0);
            let from_native = t.slots[axis.slot as usize].measured;
            r.tick_homing(now).unwrap();
            let last_target = |r: &Runtime| -> f32 {
                let prefix = format!("TX TARGET {} ", axis.slot);
                r.shared
                    .status_snapshot()
                    .logs
                    .iter()
                    .rev()
                    .find_map(|line| {
                        line.strip_prefix(&prefix)
                            .and_then(|value| value.parse().ok())
                    })
                    .unwrap()
            };
            assert_eq!(last_target(&r), from_native);
            let duration = r.homing.as_ref().unwrap().motion.unwrap().duration();
            let max_speed = if name == "theta" { 40.0 } else { 80.0 };
            let mut previous_position = from_native;
            let mut previous_speed = 0.0_f32;
            let mut peak_speed = 0.0_f32;
            // 実測を始点に固定し、送信されるnative指令の速度・加速度を確認する。
            for step in 1..=(duration / 0.05).ceil() as u64 + 2 {
                r.tick_homing(now + Duration::from_millis(step * 50))
                    .unwrap();
                let position = last_target(&r);
                let speed = (position - previous_position) / axis.native_per_unit / 0.05;
                assert!(speed.abs() <= max_speed + 0.01, "{name}: {speed}");
                assert!(
                    (speed - previous_speed).abs() / 0.05 <= 100.1,
                    "{name}: {speed} after {previous_speed}"
                );
                peak_speed = peak_speed.max(speed.abs());
                previous_position = position;
                previous_speed = speed;
            }
            assert!((peak_speed - max_speed).abs() < 0.01);
            assert!(previous_speed.abs() < 0.001);
            assert!(matches!(
                r.homing.as_ref().unwrap().phase,
                Phase::MoveRadialRetreat | Phase::MoveClearance | Phase::Rotate
            ));
            assert!(
                r.tick_homing(now + Duration::from_secs(181))
                    .unwrap_err()
                    .to_string()
                    .contains("時間超過")
            );
            r.stop(false).unwrap();
            let stopped = r.shared.status_snapshot().logs;
            r.tick_homing(now + Duration::from_secs(182)).unwrap();
            assert_eq!(r.shared.status_snapshot().logs, stopped);
        }
    }

    #[test]
    fn stop_cancels_and_stale_or_timeout_fails() {
        let mut r = crate::application::worker::tests::screen_runtime();
        r.setup_error = true;
        assert!(r.begin_homing(true, true, 180.0).is_err());
        r.setup_error = false;
        r.telemetry.as_mut().unwrap().buses = 2;
        assert!(r.begin_homing(true, true, 180.0).is_err());
        r.telemetry.as_mut().unwrap().buses = 3;
        r.begin_homing(true, true, 180.0).unwrap();
        r.stop(false).unwrap();
        assert!(r.homing.is_none());
        r.begin_homing(true, true, 180.0).unwrap();
        r.telemetry.as_mut().unwrap().stale_slots = 4;
        assert!(r.tick_homing(Instant::now()).is_err());
        r.stop(true).unwrap();
        r.telemetry.as_mut().unwrap().stale_slots = 0;
        r.begin_homing(true, true, 180.0).unwrap();
        {
            let h = r.homing.as_mut().unwrap();
            h.phase = Phase::Seek;
            h.axis_started = Instant::now() - Duration::from_secs(181);
        }
        {
            let t = r.telemetry.as_mut().unwrap();
            t.mode = RunMode::Run;
            t.enabled_slots = 4;
            t.contacts = Some(0);
        }
        assert!(
            r.tick_homing(Instant::now())
                .unwrap_err()
                .to_string()
                .contains("時間超過")
        );
    }
    #[test]
    fn rejects_missing_court_and_invalid_theta_before_output() {
        let mut r = crate::application::worker::tests::screen_runtime();
        r.court = None;
        let count = r.shared.status_snapshot().tx_count;
        assert!(r.begin_homing(true, true, 180.0).is_err());
        r.court = Some(Court::Red);
        r.cfg
            .machine
            .axes
            .iter_mut()
            .find(|a| a.name == "theta")
            .unwrap()
            .minimum = -45.0;
        assert!(r.begin_homing(true, true, 180.0).is_err());
        assert_eq!(r.shared.status_snapshot().tx_count, count);
    }

    #[test]
    fn configured_rotation_outside_travel_is_rejected_before_output() {
        for court in [Court::Red, Court::Blue] {
            let mut r = crate::application::worker::tests::screen_runtime();
            r.court = Some(court);
            r.cfg.machine.homing_theta_deg = 100.0;
            let theta = r
                .cfg
                .machine
                .axes
                .iter_mut()
                .find(|a| a.name == "theta")
                .unwrap();
            theta.minimum = -95.0;
            theta.maximum = 95.0;
            let count = r.shared.status_snapshot().tx_count;
            assert!(
                r.begin_homing(true, true, 180.0)
                    .unwrap_err()
                    .to_string()
                    .contains("可動域外")
            );
            assert_eq!(r.shared.status_snapshot().tx_count, count);
        }
    }

    #[test]
    fn rotation_failure_never_starts_radial_motion() {
        for stale in [false, true] {
            let mut r = crate::application::worker::tests::screen_runtime();
            r.begin_homing(true, true, 10.0).unwrap();
            let now = Instant::now();
            let h = r.homing.as_mut().unwrap();
            h.phase = Phase::Rotate;
            h.axis_started = now - Duration::from_secs(11);
            let t = r.telemetry.as_mut().unwrap();
            t.mode = RunMode::Run;
            t.enabled_slots = 6;
            t.contacts = Some(0);
            if stale {
                t.stale_slots = 2;
            }
            let error = r.tick_homing(now).unwrap_err();
            r.fault(error.to_string());
            assert!(r.homing.is_none());
            assert!(
                !r.shared
                    .status_snapshot()
                    .logs
                    .iter()
                    .any(|line| line == "TX ENABLE 1 1")
            );
        }
    }
    #[test]
    fn radial_retreat_rejects_bad_feedback_timeout_and_stuck_limit() {
        for failure in 0..3 {
            let mut r = crate::application::worker::tests::screen_runtime();
            r.begin_homing(true, true, 180.0).unwrap();
            for index in 0..3 {
                assert!(r.machine.capture_origin(index, r.telemetry.as_ref()));
            }
            let now = Instant::now();
            let h = r.homing.as_mut().unwrap();
            h.stage = 1;
            h.phase = Phase::MoveRadialRetreat;
            h.axis_started = now;
            h.since = now;
            let radial = r.cfg.machine.axes[0].clone();
            r.begin_homing_move(
                &radial,
                radial.origin_position - radial.homing_retreat_mm(),
                1.0,
                now,
            )
            .unwrap();
            let arrival = now
                + Duration::from_secs_f32(r.homing.as_ref().unwrap().motion.unwrap().duration())
                + Duration::from_millis(1);
            let retreat_native = radial.homing_retreat_mm() * radial.native_per_unit;
            let t = r.telemetry.as_mut().unwrap();
            t.mode = RunMode::Run;
            t.enabled_slots = 7;
            t.contacts = Some(0);
            let check_at = match failure {
                0 => {
                    t.stale_slots = 2;
                    now
                }
                1 => now + Duration::from_secs(181),
                _ => {
                    t.slots[0].measured -= retreat_native;
                    t.contacts = Some(1);
                    arrival
                }
            };
            let error = r.tick_homing(check_at).unwrap_err().to_string();
            assert!(error.contains(match failure {
                0 => "フィードバック",
                1 => "時間超過",
                _ => "リミット",
            }));
            r.fault(error);
            assert!(r.homing.is_none());
            assert!(!r.drive.running());
        }
    }
}

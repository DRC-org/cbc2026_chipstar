use super::*;
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
}
pub(super) struct Homing {
    theta_target: f32,
    stage: usize,
    phase: Phase,
    since: Instant,
    axis_started: Instant,
    pub label: String,
    timeout_seconds: f32,
}
impl Runtime {
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
            .homing_theta();
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
            timeout_seconds,
            stage: 0,
            phase: Phase::Prepare,
            since: now,
            axis_started: now,
            label: "z下端へホーミング".into(),
        });
        Ok(Reply::accepted())
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
                    let native_target = self
                        .machine
                        .set_position_target(radial.slot, target)
                        .context("r原点が確認されていません")?;
                    self.send(&format!("TARGET {} {native_target:.5}", radial.slot))?;
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
            if (origin.position - target).abs() <= 1.0 {
                anyhow::ensure!(
                    !radial.limit.unwrap().reached(contacts),
                    "r後退後もリミットが作動しています"
                );
                self.send(&format!("ENABLE {} 0", 1 << radial.slot))?;
                let h = self.homing.as_mut().unwrap();
                h.phase = Phase::AwaitHold;
                h.since = now;
                h.label = "r後退完了。停止を確認しています（θ・z保持中）".into();
            } else {
                anyhow::ensure!(
                    now.duration_since(home.axis_started).as_secs_f32() < home.timeout_seconds,
                    "rが設定した後退位置に到達せず時間超過しました"
                );
            }
            return Ok(());
        }
        if matches!(phase, Phase::AwaitRotationRun | Phase::Rotate) {
            if matches!(phase, Phase::AwaitRotationRun) {
                if t.mode == RunMode::Run && t.enabled_slots == holding {
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
            let error = home.theta_target - origin.position;
            if error.abs() <= THETA_TOLERANCE_DEG {
                self.send(&format!("JOG {} 0", theta.slot))?;
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
            } else {
                anyhow::ensure!(
                    now.duration_since(home.axis_started).as_secs_f32() < home.timeout_seconds,
                    "θが旋回先に到達せず時間超過しました"
                );
                // 到達手前で減速し、設定したホーミング速度を超えない。
                let speed = (self.cfg.machine.effective_axis_speed(&theta)
                    * theta.homing_speed_percent
                    * 0.01)
                    .min(error.abs() * 2.0);
                self.send(&format!(
                    "JOG {} {:.5}",
                    theta.slot,
                    error.signum() * speed * theta.native_per_unit
                ))?;
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
                    let native_target = self
                        .machine
                        .set_position_target(axis.slot, target)
                        .context("z原点が確認されていません")?;
                    self.send(&format!("TARGET {} {native_target:.5}", axis.slot))?;
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
            if (position - target).abs() <= Z_CLEARANCE_TOLERANCE_MM {
                self.send(&format!("ENABLE {} 1", 1 << theta.slot))?;
                let h = self.homing.as_mut().unwrap();
                h.phase = Phase::AwaitRotationRun;
                h.since = now;
                h.axis_started = now;
                h.label = format!("θを{:+.0}°へ旋回しています（z保持中）", h.theta_target);
            } else {
                anyhow::ensure!(
                    now.duration_since(home.axis_started).as_secs_f32() < home.timeout_seconds,
                    "zが設定した上昇位置に到達せず時間超過しました"
                );
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
            check_homing_retreat(court, None, None);
            check_homing_retreat(court, Some(25.0), Some(60.0));
        }
    }

    fn check_homing_retreat(court: Court, z_distance: Option<f32>, r_distance: Option<f32>) {
        let mut r = crate::application::worker::tests::screen_runtime();
        r.court = Some(court);
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
        {
            let t = r.telemetry.as_mut().unwrap();
            t.slots[2].measured = z_distance * 2.0;
            t.contacts = Some(0);
        }
        r.tick_homing(now + Duration::from_millis(350)).unwrap();
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
        r.tick_homing(now + Duration::from_millis(400)).unwrap();
        r.tick_homing(now + Duration::from_millis(450)).unwrap();
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
        let expected_sign = (court.homing_theta() * theta.native_per_unit).signum();
        let theta_native = theta.native_per_unit * court.homing_theta();
        let turn = r
            .shared
            .status_snapshot()
            .logs
            .iter()
            .find_map(|line| {
                line.strip_prefix("TX JOG 1 ")
                    .and_then(|value| value.parse::<f32>().ok())
            })
            .unwrap();
        assert_eq!(turn.signum(), expected_sign);
        r.telemetry.as_mut().unwrap().slots[1].measured = theta_native;
        r.tick_homing(now + Duration::from_millis(500)).unwrap();
        r.telemetry.as_mut().unwrap().enabled_slots = 7;
        r.tick_homing(now + Duration::from_millis(550)).unwrap();
        r.tick_homing(now + Duration::from_millis(600)).unwrap();
        r.telemetry.as_mut().unwrap().contacts = Some(1);
        r.tick_homing(now + Duration::from_millis(650)).unwrap();
        assert!(!r.machine.origin_states(r.telemetry.as_ref())[0].captured);
        r.telemetry.as_mut().unwrap().enabled_slots = 6;
        r.tick_homing(now + Duration::from_millis(700)).unwrap();
        assert!(matches!(
            r.homing.as_ref().unwrap().phase,
            Phase::AwaitRetreatRun
        ));
        r.telemetry.as_mut().unwrap().enabled_slots = 7;
        r.tick_homing(now + Duration::from_millis(750)).unwrap();
        r.tick_homing(now + Duration::from_millis(800)).unwrap();
        assert!(r.homing.is_some());
        r.telemetry.as_mut().unwrap().slots[0].measured = -r_distance * 0.5;
        r.telemetry.as_mut().unwrap().contacts = Some(0);
        r.tick_homing(now + Duration::from_millis(850)).unwrap();
        assert!(matches!(r.homing.as_ref().unwrap().phase, Phase::AwaitHold));
        r.telemetry.as_mut().unwrap().enabled_slots = 6;
        r.tick_homing(now + Duration::from_millis(900)).unwrap();
        assert!(r.homing.is_none());
        assert!(!r.drive.running());
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
        assert!(logs.iter().any(|l| l == "TX JOG 1 0"));
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
            court.homing_theta()
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
            let radial = &r.cfg.machine.axes[0];
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
                    now
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

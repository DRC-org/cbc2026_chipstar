use super::*;
const Z_CLEARANCE_MM: f32 = 50.0;
const Z_CLEARANCE_TOLERANCE_MM: f32 = 1.0;
const R_RETREAT_MM: f32 = 100.0;
const R_RETREAT_TOLERANCE_MM: f32 = 1.0;

#[derive(Clone, Copy)]
enum Phase {
    Prepare,
    AwaitRun,
    Seek,
    AwaitStop,
    PrepareClearance,
    AwaitClearanceRun,
    MoveClearance,
    MoveRadialRetreat,
    AwaitHold,
}
pub(super) struct Homing {
    stage: usize,
    phase: Phase,
    since: Instant,
    axis_started: Instant,
    pub label: String,
    timeout_seconds: f32,
}
impl Runtime {
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
            "人間がθ姿勢とz下降・r前進経路の干渉を確認してください"
        );
        anyhow::ensure!(
            !self.emergency
                && !self.drive.running()
                && self.drive.awaiting().is_none()
                && !self.test.enabled
                && !self.sts.active
                && !self.sts.busy()
                && self.homing.is_none(),
            "全操作を停止してからホーミングしてください"
        );
        self.axes_ready(false)?;
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
            if name == "z" {
                anyhow::ensure!(a.unit == "mm", "z軸の単位はmmにしてください");
                let clearance = a.origin_position + Z_CLEARANCE_MM;
                anyhow::ensure!(
                    (a.minimum..=a.maximum).contains(&clearance),
                    "z原点から50mm上昇した位置が可動域外です"
                );
            } else {
                let retreat = a.origin_position - limit.direction * R_RETREAT_MM;
                anyhow::ensure!(
                    (a.minimum..=a.maximum).contains(&retreat),
                    "r原点から100mm後退した位置が可動域外です"
                );
            }
        }
        anyhow::ensure!(
            self.telemetry
                .as_ref()
                .is_some_and(|t| t.contacts.is_some()),
            "接点情報が取得できません"
        );
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
        if matches!(phase, Phase::AwaitHold) {
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
                "z保持中のフィードバック異常"
            );
            if t.mode == RunMode::Run && t.enabled_slots == z_bit {
                self.homing = None;
                self.reason = "r・zホーミング完了。z位置を保持しています".into();
            } else {
                anyhow::ensure!(
                    elapsed < Duration::from_millis(500),
                    "ホーミング完了後のz保持応答がありません"
                );
            }
            return Ok(());
        }
        if matches!(phase, Phase::MoveRadialRetreat) {
            let index = self
                .cfg
                .machine
                .axes
                .iter()
                .position(|axis| axis.name == "r")
                .context("r軸の設定が必要です")?;
            let axis = self.cfg.machine.axes[index].clone();
            let z = self
                .cfg
                .machine
                .axes
                .iter()
                .find(|axis| axis.name == "z")
                .context("z軸の設定が必要です")?;
            let bit = 1 << axis.slot;
            let enabled = bit | (1 << z.slot);
            let limit = axis.limit.context("rのリミットスイッチ設定が必要です")?;
            let target = axis.origin_position - limit.direction * R_RETREAT_MM;
            anyhow::ensure!(
                t.mode == RunMode::Run && t.enabled_slots == enabled,
                "r後退中の出力状態が変化しました"
            );
            anyhow::ensure!(
                t.stale_slots & enabled == 0
                    && t.error_bits[usize::from(axis.slot)] == 0
                    && t.error_bits[usize::from(z.slot)] == 0,
                "r後退中のフィードバック異常"
            );
            let position = self.machine.origin_states(Some(&t))[index].position;
            if (position - target).abs() <= R_RETREAT_TOLERANCE_MM {
                self.send(&format!("ENABLE {bit} 0"))?;
                let h = self.homing.as_mut().unwrap();
                h.phase = Phase::AwaitHold;
                h.since = now;
                h.label = "r後退完了（z位置保持中）".into();
            } else {
                anyhow::ensure!(
                    now.duration_since(home.axis_started).as_secs_f32() < home.timeout_seconds,
                    "rが原点から100mm後退せず時間超過しました"
                );
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
            let target = axis.origin_position + Z_CLEARANCE_MM;
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
                let radial = self
                    .cfg
                    .machine
                    .axes
                    .iter()
                    .find(|axis| axis.name == "r")
                    .context("r軸の設定が必要です")?;
                self.send(&format!("ENABLE {} 1", 1 << radial.slot))?;
                let h = self.homing.as_mut().unwrap();
                h.stage = 1;
                h.phase = Phase::AwaitRun;
                h.since = now;
                h.axis_started = now;
                h.label = "r前端へホーミング（z位置保持中）".into();
            } else {
                anyhow::ensure!(
                    now.duration_since(home.axis_started).as_secs_f32() < home.timeout_seconds,
                    "zが原点から50mmの位置に到達せず時間超過しました"
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
                h.label = "zを原点から50mm上昇".into();
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
            bit | (1 << z.slot)
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
            anyhow::ensure!(
                t.mode != RunMode::Run,
                "ホーミング開始前の停止を確認できません"
            );
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
            anyhow::ensure!(
                self.machine.capture_origin(index, Some(&t)),
                "{name}原点を採用できません"
            );
            let next_phase = if stage == 0 {
                self.send("STOP")?;
                Phase::AwaitStop
            } else {
                let target = axis.origin_position - limit.direction * R_RETREAT_MM;
                let native_target = self
                    .machine
                    .set_position_target(axis.slot, target)
                    .context("r原点が確認されていません")?;
                self.send(&format!("TARGET {} {native_target:.5}", axis.slot))?;
                Phase::MoveRadialRetreat
            };
            let h = self.homing.as_mut().unwrap();
            h.phase = next_phase;
            h.since = now;
            h.axis_started = now;
            if stage == 1 {
                h.label = "rを原点から100mm後退（z位置保持中）".into();
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
    fn homes_z_raises_and_holds_it_while_homing_r() {
        let mut r = crate::application::worker::tests::screen_runtime();
        let z = r
            .cfg
            .machine
            .axes
            .iter_mut()
            .find(|a| a.name == "z")
            .unwrap();
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
        r.begin_homing(true, true, 180.0).unwrap();
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
            t.slots[2].measured = 100.0;
            t.contacts = Some(0);
        }
        r.tick_homing(now + Duration::from_millis(350)).unwrap();
        r.telemetry.as_mut().unwrap().enabled_slots = 5;
        r.tick_homing(now + Duration::from_millis(400)).unwrap();
        r.tick_homing(now + Duration::from_millis(450)).unwrap();
        r.telemetry.as_mut().unwrap().contacts = Some(1);
        r.tick_homing(now + Duration::from_millis(500)).unwrap();
        {
            let t = r.telemetry.as_mut().unwrap();
            t.contacts = Some(0);
            t.slots[0].measured = -50.0;
        }
        r.tick_homing(now + Duration::from_millis(550)).unwrap();
        r.telemetry.as_mut().unwrap().enabled_slots = 4;
        r.tick_homing(now + Duration::from_millis(600)).unwrap();
        assert!(r.homing.is_none());
        assert!(!r.drive.running());
        let logs = r.shared.status_snapshot().logs;
        assert!(logs.iter().any(|l| l.contains("JOG 2 -20.00000")));
        assert!(logs.iter().any(|l| l.contains("TX REINIT 5")));
        assert!(logs.iter().any(|l| l.contains("TARGET 2 100.00000")));
        assert!(logs.iter().any(|l| l.contains("JOG 0 12.50000")));
        assert!(logs.iter().any(|l| l.contains("TARGET 0 -50.00000")));
        assert!(logs.iter().any(|l| l.contains("ENABLE 1 0")));
        assert!(
            !logs
                .iter()
                .any(|l| l.contains("JOG 1 ") || l.contains("TARGET 1 "))
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
            (origins.iter().find(|a| a.name == "r").unwrap().position - (r_origin - 100.0)).abs()
                < 0.001
        );
        assert_eq!(
            origins.iter().find(|a| a.name == "z").unwrap().position,
            50.0
        );
        assert_eq!(origins.iter().find(|a| a.name == "z").unwrap().target, 50.0);
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
}

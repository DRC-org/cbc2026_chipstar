use super::*;
#[derive(Clone, Copy)]
enum Phase {
    Prepare,
    AwaitRun,
    Seek,
    AwaitStop,
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
        anyhow::ensure!(
            self.fresh() && self.settings.ready() && self.device.is_some(),
            "接続・設定照合を完了してください"
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
        }
        anyhow::ensure!(
            self.telemetry
                .as_ref()
                .is_some_and(|t| t.contacts.is_some()),
            "接点情報が取得できません"
        );
        self.stop(true)?;
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
                h.stage += 1;
                h.phase = Phase::Prepare;
                h.since = now;
                h.axis_started = now;
                h.label = "r前端へホーミング".into();
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
            if t.mode == RunMode::Run && t.enabled_slots == bit {
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
            t.mode == RunMode::Run && t.enabled_slots == bit,
            "ホーミング中の出力状態が変化しました"
        );
        if limit.reached(contacts) {
            self.send(&format!("JOG {} 0", axis.slot))?;
            self.send("STOP")?;
            anyhow::ensure!(
                self.machine.capture_origin(index, Some(&t)),
                "{name}原点を採用できません"
            );
            let h = self.homing.as_mut().unwrap();
            h.phase = Phase::AwaitStop;
            h.since = now;
        } else {
            let speed = (axis.speed_per_second * axis.homing_speed_percent * 0.01)
                .min(if name == "z" { 5.0 } else { 10.0 });
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
    fn homes_z_then_r_without_theta_and_keeps_stop() {
        let mut r = crate::application::worker::tests::screen_runtime();
        let z = r
            .cfg
            .machine
            .axes
            .iter_mut()
            .find(|a| a.name == "z")
            .unwrap();
        z.speed_per_second = 20.0;
        z.homing_speed_percent = 10.0;
        z.native_per_unit = 2.0;
        let radial = r
            .cfg
            .machine
            .axes
            .iter_mut()
            .find(|a| a.name == "r")
            .unwrap();
        radial.speed_per_second = 40.0;
        radial.homing_speed_percent = 25.0;
        radial.native_per_unit = 0.5;
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
            t.enabled_slots = 1;
        }
        r.tick_homing(now + Duration::from_millis(300)).unwrap();
        r.tick_homing(now + Duration::from_millis(350)).unwrap();
        r.telemetry.as_mut().unwrap().contacts = Some(3);
        r.tick_homing(now + Duration::from_millis(400)).unwrap();
        r.telemetry.as_mut().unwrap().mode = RunMode::Stop;
        r.tick_homing(now + Duration::from_millis(450)).unwrap();
        assert!(r.homing.is_none());
        assert!(!r.drive.running());
        let logs = r.shared.status_snapshot().logs;
        assert!(logs.iter().any(|l| l.contains("JOG 2 -4.00000")));
        assert!(logs.iter().any(|l| l.contains("JOG 0 5.00000")));
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
            (origins.iter().find(|a| a.name == "r").unwrap().position - r_origin).abs() < 0.001
        );
        assert_eq!(
            origins.iter().find(|a| a.name == "z").unwrap().position,
            0.0
        );
    }
    #[test]
    fn stop_cancels_and_stale_or_timeout_fails() {
        let mut r = crate::application::worker::tests::screen_runtime();
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

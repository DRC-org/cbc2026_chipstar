//! セッティング画面で、手で接点へ当てたr・zと正面のθを原点として記録する。
use super::*;

impl Runtime {
    pub(super) fn manual_origin_ready(&self) -> Result<()> {
        anyhow::ensure!(self.court.is_some(), "赤コートか青コートを選んでください");
        anyhow::ensure!(
            self.preparation == PreparationPhase::Setting
                && !self.authority.active()
                && self.homing_idle()
                && self.sequence.is_none()
                && self.sts.teach_id.is_none(),
            "準備画面で操作を停止してください"
        );
        self.axes_ready(false)?;
        for name in ["r", "z"] {
            anyhow::ensure!(
                self.cfg
                    .machine
                    .axes
                    .iter()
                    .any(|axis| axis.name == name && axis.limit.is_some()),
                "{name}のリミットスイッチを設定してください"
            );
        }
        anyhow::ensure!(
            self.cfg.machine.axes.iter().any(|axis| axis.name == "theta"
                && axis.unit == "deg"
                && axis.origin_position == 0.0
                && axis.minimum <= 0.0
                && axis.maximum >= 0.0),
            "θの単位をdeg、原点座標を0に設定してください"
        );
        if self.manual_origins {
            anyhow::ensure!(
                self.telemetry
                    .as_ref()
                    .is_some_and(|t| self.manual_origins_idle(t)),
                "全トルク解除の応答を待っています"
            );
        }
        Ok(())
    }

    pub(super) fn manual_origins_idle(&self, telemetry: &Telemetry) -> bool {
        self.preparation == PreparationPhase::Setting
            && !self.authority.active()
            && self.homing_idle()
            && self.sequence.is_none()
            && self.sts.teach_id.is_none()
            && telemetry.mode != RunMode::Run
            && telemetry.held_slots == Some(0)
    }

    pub(super) fn begin_manual_origins(&mut self) -> Result<Reply> {
        self.manual_origin_ready()?;
        self.stop(true)?;
        self.adjustment = false;
        self.machine.set_soft_limits(true);
        self.machine.invalidate_origins();
        self.limit_origin_captured = 0;
        self.manual_origins = true;
        self.guide.reset_input();
        Ok(Reply::data(
            "手動原点設定。全トルクを解除し、r・zのリミット到達とθの正面記録を待っています".into(),
        ))
    }

    pub(super) fn capture_manual_theta(&mut self) -> Result<Reply> {
        anyhow::ensure!(self.manual_origins, "手動原点設定を開始してください");
        self.manual_origin_ready()?;
        let index = self
            .cfg
            .machine
            .axes
            .iter()
            .position(|axis| axis.name == "theta")
            .unwrap();
        anyhow::ensure!(
            self.machine
                .capture_coordinate(index, self.telemetry.as_ref(), 0.0),
            "θの最新の実測位置を確認できません"
        );
        self.shared
            .log("手動原点設定: 正面のθを0°として記録".into());
        self.finish_manual_origins(Instant::now());
        Ok(Reply::data("正面のθを0°として記録しました".into()))
    }

    pub(super) fn finish_manual_origins(&mut self, now: Instant) {
        if !self.manual_origins
            || !self.guide.enabled
            || self.manual_origin_ready().is_err()
            || self
                .machine
                .origin_states(self.telemetry.as_ref())
                .iter()
                .any(|axis| !axis.captured)
        {
            return;
        }
        self.manual_origins = false;
        self.front_return_pending = None;
        // 手動設定後は現在のEE姿勢を採用する。自動ホーミングのθ正面復帰は予約しない。
        if crate::machine::ee::axes(&self.cfg.machine)
            .iter()
            .any(|axis| axis.name == "ee_rotation" && axis.enabled)
        {
            self.ee.prepared_rotation_field = None;
            self.ee.preparation_hold_since = Some(now);
        }
        self.shared
            .log("手動原点設定完了: 開始姿勢に合わせてください".into());
        self.reason = "手動原点設定完了。開始姿勢に合わせてください".into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime() -> Runtime {
        let mut r = super::super::tests::screen_runtime();
        r.guide.enabled = true;
        for axis in &mut r.cfg.machine.axes {
            axis.origin_position = if axis.name == "r" { 120.0 } else { 0.0 };
            axis.limit = match axis.name.as_str() {
                "r" => Some(crate::machine::AxisLimit {
                    input: 0,
                    direction: 1.0,
                    normally_closed: false,
                }),
                "z" => Some(crate::machine::AxisLimit {
                    input: 1,
                    direction: -1.0,
                    normally_closed: false,
                }),
                _ => None,
            };
        }
        r.machine.reconfigure(r.cfg.machine.clone());
        r
    }

    fn begin(r: &mut Runtime) {
        r.request(&Request::new("preparation_manual_begin"), true)
            .unwrap();
        let t = r.telemetry.as_mut().unwrap();
        t.mode = RunMode::Stop;
        t.held_slots = Some(0);
        t.contacts = Some(0);
    }

    fn contacts(r: &mut Runtime, contacts: u8) {
        let before = r.telemetry.clone().unwrap();
        let mut current = before.clone();
        current.contacts = Some(contacts);
        r.machine.observe(&current);
        r.capture_limit_origins(Some(&before), &current);
        r.telemetry = Some(current);
        r.finish_manual_origins(Instant::now());
    }

    fn theta(r: &mut Runtime) {
        r.request(&Request::new("preparation_manual_theta"), true)
            .unwrap();
    }

    #[test]
    fn manual_origins_accept_either_order_and_start_without_automatic_arm_motion() {
        for theta_first in [false, true] {
            for first in [1, 2] {
                let mut r = runtime();
                begin(&mut r);
                assert_eq!(r.preparation_step(), PreparationStep::ManualHome);
                let log_start = r.shared.status_snapshot().logs.len();
                if theta_first {
                    theta(&mut r);
                }
                contacts(&mut r, first);
                contacts(&mut r, 0); // 採用後に離してから、他方を当てる。
                contacts(&mut r, 3 ^ first);
                if !theta_first {
                    theta(&mut r);
                }
                let origins = r.machine.origin_states(r.telemetry.as_ref());
                assert!(origins.iter().all(|axis| axis.captured));
                assert!((origins[0].position - 120.0).abs() < 0.001);
                assert!(origins[1].position.abs() < 0.001);
                assert!(origins[2].position.abs() < 0.001);
                assert_eq!(r.preparation_step(), PreparationStep::Position);
                assert!(
                    !r.manual_origins && r.homing.is_none() && r.front_return_pending.is_none()
                );
                assert!(!r.drive.running() && r.drive.awaiting().is_none());
                assert!(
                    r.shared
                        .status_snapshot()
                        .logs
                        .iter()
                        .skip(log_start)
                        .all(|line| !line.starts_with("TX ") || line == "TX TONE 1")
                );
                r.request(&Request::new("preparation_wait"), true).unwrap();
                r.request(&Request::new("preparation_start"), true).unwrap();
                assert!(r.drive.awaiting().is_some() && r.homing.is_none());
            }
        }
    }

    #[test]
    fn manual_origins_wait_for_torque_release_and_fresh_feedback() {
        let mut r = runtime();
        begin(&mut r);
        r.telemetry.as_mut().unwrap().mode = RunMode::Run;
        contacts(&mut r, 3);
        assert!(
            r.machine
                .origin_states(r.telemetry.as_ref())
                .iter()
                .all(|axis| !axis.captured)
        );
        assert!(
            r.request(&Request::new("preparation_manual_theta"), true)
                .is_err()
        );
        r.telemetry.as_mut().unwrap().mode = RunMode::Stop;
        r.telemetry.as_mut().unwrap().held_slots = Some(4);
        assert!(
            r.request(&Request::new("preparation_manual_theta"), true)
                .is_err()
        );
        r.telemetry.as_mut().unwrap().held_slots = Some(0);
        r.telemetry.as_mut().unwrap().stale_slots = 1;
        contacts(&mut r, 3);
        assert!(!r.machine.origin_states(r.telemetry.as_ref())[0].captured);
        assert!(r.machine.origin_states(r.telemetry.as_ref())[2].captured);
        r.telemetry.as_mut().unwrap().stale_slots = 0;
        contacts(&mut r, 3); // 押し直さずに復帰を拾う。
        theta(&mut r);
        assert_eq!(r.preparation_step(), PreparationStep::Position);
    }

    #[test]
    fn manual_origins_stop_on_cancel_and_pause_when_guide_is_hidden() {
        for action in ["stop", "preparation_restart", "estop"] {
            let mut r = runtime();
            begin(&mut r);
            r.request(&Request::new(action), true).unwrap();
            contacts(&mut r, 3);
            assert!(!r.manual_origins);
            assert!(
                r.machine
                    .origin_states(r.telemetry.as_ref())
                    .iter()
                    .all(|axis| !axis.captured)
            );
            assert!(r.ee.preparation_hold_since.is_none());
        }
        let mut r = runtime();
        begin(&mut r);
        r.request(
            &Request {
                flag: Some(false),
                ..Request::new("preparation_guide")
            },
            true,
        )
        .unwrap();
        contacts(&mut r, 3);
        assert!(
            r.machine
                .origin_states(r.telemetry.as_ref())
                .iter()
                .all(|axis| !axis.captured)
        );
        r.request(
            &Request {
                flag: Some(true),
                ..Request::new("preparation_guide")
            },
            true,
        )
        .unwrap();
        contacts(&mut r, 3);
        theta(&mut r);
        assert_eq!(r.preparation_step(), PreparationStep::Position);
    }

    #[test]
    fn manual_origins_reject_invalid_context_before_changing_coordinates_or_output() {
        for case in 0..6 {
            let mut r = runtime();
            match case {
                0 => r.court = None,
                1 => r.cfg.machine.axes[0].limit = None,
                2 => r.preparation = PreparationPhase::Waiting,
                3 => r.drive = DriveState::Running,
                4 => r.telemetry.as_mut().unwrap().stale_slots = 2,
                _ => r.cfg.machine.axes[1].origin_position = 10.0,
            }
            let origins = r.machine.origin_states(r.telemetry.as_ref());
            let before = r.shared.status_snapshot().logs.len();
            assert!(
                r.request(&Request::new("preparation_manual_begin"), true)
                    .is_err(),
                "case={case}"
            );
            assert_eq!(origins, r.machine.origin_states(r.telemetry.as_ref()));
            assert!(!r.manual_origins);
            assert!(
                r.shared
                    .status_snapshot()
                    .logs
                    .iter()
                    .skip(before)
                    .all(|line| !line.starts_with("TX ") || line == "TX TONE 2")
            );
        }
        let mut r = runtime();
        assert!(
            r.request(&Request::new("preparation_manual_begin"), false)
                .is_err()
        );
    }

    #[test]
    fn manual_origins_prepare_ee_hold_at_the_measured_orientation() {
        let mut r = runtime();
        r.cfg.machine.serial_svmd = MachineProfile::embedded().unwrap().serial_svmd;
        begin(&mut r);
        contacts(&mut r, 3);
        theta(&mut r);
        let now = Instant::now();
        r.test.peers.insert("sts", now);
        r.servo_feedback.insert(
            1,
            ServoFeedback {
                seen: now,
                position: 1500,
                absolute_position: true,
                error: 0,
                detail: String::new(),
            },
        );
        assert!(r.ee.preparation_hold_since.is_some());
        r.tick_ee(now).unwrap();
        assert_eq!(r.ee.targets.len(), 1);
        assert_eq!(r.ee.targets["ee_rotation"], 1500.0);
        assert!(r.homing.is_none() && r.front_return_pending.is_none());
        assert!(!r.drive.running());
        let field = r.ee.rotation_field;
        let theta_axis = &r.cfg.machine.axes[1];
        r.telemetry.as_mut().unwrap().slots[theta_axis.slot as usize].measured +=
            10.0 * theta_axis.native_per_unit;
        r.tick_ee(now + Duration::from_millis(60)).unwrap();
        assert_eq!(r.ee.rotation_field, field);
        assert_ne!(r.ee.targets["ee_rotation"], 1500.0);
    }
}

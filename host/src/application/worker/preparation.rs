use super::*;

impl Runtime {
    // 正面合わせではθを手で回せるようにする。既に保持しているzだけを残し、
    // それまで無効だったzを新たに有効化しない。
    fn stop_for_homing_setup(&mut self) -> Result<()> {
        let z = self.cfg.machine.axes.iter().find(|a| a.name == "z");
        let holding_z = self.fresh()
            && self.telemetry.as_ref().zip(z).is_some_and(|(t, z)| {
                let active = if t.mode == RunMode::Run {
                    t.enabled_slots
                } else {
                    t.held_slots.unwrap_or(0)
                };
                active & (1 << z.slot) != 0
            });
        if !self.emergency && holding_z {
            self.stop_with_z_hold()?;
        } else {
            self.stop(true)?;
        }
        Ok(())
    }

    pub(super) fn preparation_ready(&self) -> Result<()> {
        anyhow::ensure!(self.court.is_some(), "赤コートか青コートを選んでください");
        anyhow::ensure!(
            !self.emergency,
            "ソフト緊停を解除し、機体を再確認してください"
        );
        anyhow::ensure!(!self.adjustment, "原点調整を終了してください");
        anyhow::ensure!(!self.authority.active(), "人間の操作へ戻してください");
        anyhow::ensure!(
            self.homing.is_none() && self.sequence.is_none(),
            "準備動作の完了を待ってください"
        );
        anyhow::ensure!(
            !self.test.enabled
                && !self.sts.active
                && !self.sts.busy()
                && self.sts.teach_id.is_none(),
            "個別テスト・STS管理を終了してください"
        );
        anyhow::ensure!(
            !self.drive.running() && self.drive.awaiting().is_none(),
            "停止・保持してから開始姿勢を確認してください"
        );
        self.ready()?;
        anyhow::ensure!(
            self.screen_control || self.cfg.simulate || !self.gamepad_name.is_empty(),
            "DualSenseを接続してください"
        );
        let input = if self.screen_control {
            &self.manual_input
        } else {
            self.gamepad_input.as_ref().unwrap_or(&self.manual_input)
        };
        anyhow::ensure!(
            input.axes.iter().all(|v| v.is_finite() && v.abs() < 0.1)
                && input.buttons.iter().all(|b| *b == 0),
            "スティックとボタンを離してください"
        );
        Ok(())
    }

    pub(super) fn preparation_request(&mut self, req: &Request, manual: bool) -> Result<Reply> {
        // 画面の切替は操作権や緊停と独立。非表示のガイドでパッド操作を消費しない。
        if req.action == "preparation_guide" {
            anyhow::ensure!(manual, "ガイドの表示切替は画面から行ってください");
            self.guide.enabled = req.flag.context("ガイドの表示状態が必要です")?;
            self.guide.reset_input();
            self.pad.home_ready = false;
            self.pad.home_since = None;
            self.pad.ee_armed = false;
            return Ok(Reply::accepted());
        }
        anyhow::ensure!(
            manual && !self.authority.active(),
            "準備の切替は人間が画面またはコントローラから行ってください"
        );
        match req.action.as_str() {
            "preparation_restart" => {
                self.stop_for_homing_setup()?;
                self.manual_input = ControllerState::default();
                self.machine.invalidate_origins();
                self.court = None;
                self.preparation = PreparationPhase::Setting;
                self.guide.reset_input();
                self.pad.home_ready = false;
                self.pad.home_since = None;
                self.pad.ee_armed = false;
                Ok(Reply::data("停止してコート選択に戻りました".into()))
            }
            "preparation_court" => {
                anyhow::ensure!(
                    self.preparation == PreparationPhase::Setting
                        && self.homing_idle()
                        && self.sequence.is_none()
                        && !self.sts.control_busy()
                        && self.sts.teach_id.is_none(),
                    "準備画面で全操作を停止してからコートを変更してください"
                );
                let court = match req.text.as_deref() {
                    Some("red") => Court::Red,
                    Some("blue") => Court::Blue,
                    _ => bail!("赤コートか青コートを選んでください"),
                };
                self.stop_for_homing_setup()?;
                if self.court != Some(court) {
                    self.machine.invalidate_origins();
                    self.court = Some(court);
                }
                Ok(Reply::data(format!("{}を選択しました", court.label())))
            }
            "preparation_wait" => {
                anyhow::ensure!(
                    self.preparation == PreparationPhase::Setting,
                    "準備画面へ戻って再確認してください"
                );
                self.preparation_ready()?;
                self.manual_input = ControllerState::default();
                self.screen_input_times = [None; crate::input::MACHINE_INPUT_COUNT];
                self.preparation = PreparationPhase::Waiting;
                Ok(Reply::data(
                    "開始待ち。会場の合図後に「競技開始」を押してください".into(),
                ))
            }
            "preparation_start" => {
                anyhow::ensure!(
                    self.preparation == PreparationPhase::Waiting,
                    "準備完了を確認して開始待ちへ移ってください"
                );
                self.preparation_ready()?;
                self.preparation = PreparationPhase::Active;
                if let Err(error) = self.start() {
                    self.preparation = PreparationPhase::Recovery;
                    self.fault(error.to_string());
                    return Err(error);
                }
                Ok(Reply::accepted())
            }
            "preparation_return" => {
                if self
                    .machine
                    .origin_states(self.telemetry.as_ref())
                    .iter()
                    .any(|o| !o.captured)
                {
                    self.stop_for_homing_setup()?;
                } else {
                    self.stop(false)?;
                }
                self.preparation = PreparationPhase::Setting;
                Ok(Reply::data(
                    "準備画面に戻りました。必要な操作から準備を再開してください".into(),
                ))
            }
            _ => bail!("不明な準備操作です"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared() -> Runtime {
        let shared = Arc::new(Shared::new(BridgeConfig {
            serial_device: "unused".into(),
            baud_rate: 115200,
            rate_hz: 100.0,
            machine: MachineProfile::embedded().unwrap(),
            profile_path: "/dev/null".into(),
            simulate: true,
        }));
        let mut runtime = Runtime::new(shared);
        runtime.court = Some(Court::Red);
        for _ in 0..80 {
            runtime.tick().unwrap();
        }
        for index in 0..3 {
            assert!(
                runtime
                    .machine
                    .capture_origin(index, runtime.telemetry.as_ref())
            );
        }
        runtime
    }
    fn wait(runtime: &mut Runtime) {
        runtime
            .request(&Request::new("preparation_wait"), true)
            .unwrap();
    }

    #[test]
    fn waiting_sends_no_motion_and_blocks_all_other_control_paths() {
        let mut r = prepared();
        let before = r.shared.status_snapshot().tx_count;
        wait(&mut r);
        assert_eq!(r.shared.status_snapshot().tx_count, before + 1);
        assert_eq!(r.shared.status_snapshot().logs.back().unwrap(), "TX TONE 1");
        for action in [
            "run",
            "home",
            "origin",
            "input",
            "ee",
            "sts",
            "test_mode",
            "apply",
            "connection",
            "claim",
            "sequence_run",
        ] {
            assert!(r.request(&Request::new(action), true).is_err(), "{action}");
        }
        r.screen_control = false;
        let mut pad = ControllerState::default();
        pad.axes[0] = 1.0;
        pad.buttons[6] = 1;
        r.read_pad(pad, Instant::now()).unwrap();
        assert!(!r.drive.running() && r.drive.awaiting().is_none());
        assert!(r.request(&Request::new("preparation_start"), true).is_err());
        r.read_pad(ControllerState::default(), Instant::now())
            .unwrap();
        r.request(&Request::new("preparation_start"), true).unwrap();
        assert_eq!(r.preparation, PreparationPhase::Active);
        r.tick().unwrap();
        assert!(r.drive.running());
    }

    #[test]
    fn emergency_reset_and_feedback_recovery_do_not_restore_preparation() {
        let mut r = prepared();
        wait(&mut r);
        r.request(&Request::new("estop"), true).unwrap();
        r.request(&Request::new("estop_reset"), true).unwrap();
        assert_eq!(r.preparation, PreparationPhase::Recovery);
        assert!(r.request(&Request::new("run"), true).is_err());
        assert!(r.request(&Request::new("preparation_start"), true).is_err());
        r.request(&Request::new("preparation_return"), true)
            .unwrap();
        r.tick().unwrap();
        wait(&mut r);
        r.machine.invalidate_origins();
        r.tick().unwrap();
        assert_eq!(r.preparation, PreparationPhase::Recovery);
    }

    #[test]
    fn origins_and_human_authority_are_required() {
        let mut r = prepared();
        assert!(r.request(&Request::new("preparation_wait"), false).is_err());
        r.machine.invalidate_origins();
        assert!(
            r.request(
                &Request {
                    flag: Some(true),
                    ..Request::new("preparation_wait")
                },
                true
            )
            .is_err()
        );
        assert_eq!(r.preparation, PreparationPhase::Setting);
    }
    #[test]
    fn court_selection_is_explicit_and_changes_invalidate_origins() {
        let mut r = prepared();
        r.court = None;
        assert!(r.preparation_ready().is_err());
        let select = |name: &str| Request {
            text: Some(name.into()),
            ..Request::new("preparation_court")
        };
        assert!(r.request(&select("green"), true).is_err());
        assert!(r.request(&select("red"), false).is_err());
        r.request(&select("red"), true).unwrap();
        assert_eq!(r.court, Some(Court::Red));
        assert!(
            r.machine
                .origin_states(r.telemetry.as_ref())
                .iter()
                .all(|a| !a.captured)
        );
        for i in 0..3 {
            assert!(r.machine.capture_origin(i, r.telemetry.as_ref()));
        }
        r.request(&select("red"), true).unwrap();
        assert!(
            r.machine
                .origin_states(r.telemetry.as_ref())
                .iter()
                .all(|a| a.captured)
        );
        wait(&mut r);
        assert!(r.request(&select("blue"), true).is_err());
        r.request(&Request::new("preparation_return"), true)
            .unwrap();
        r.request(&select("blue"), true).unwrap();
        assert_eq!(r.court, Some(Court::Blue));
        assert!(
            r.machine
                .origin_states(r.telemetry.as_ref())
                .iter()
                .all(|a| !a.captured)
        );
    }
    #[test]
    fn restarting_preparation_releases_theta_and_keeps_only_existing_z_hold() {
        for enabled in [0, 2, 4, 6, 7] {
            let mut r = prepared();
            if enabled != 0 {
                r.send(&format!("ENABLE {enabled} 1")).unwrap();
                r.send("RUN").unwrap();
                r.tick().unwrap();
            }
            r.request(&Request::new("preparation_restart"), true)
                .unwrap();
            r.tick().unwrap();
            let t = r.telemetry.as_ref().unwrap();
            assert_ne!(t.mode, RunMode::Run);
            assert_eq!(t.held_slots.unwrap_or(0), enabled & 4);
            assert!(r.court.is_none());
            assert!(!r.drive.running());
            r.request(
                &Request {
                    text: Some("blue".into()),
                    ..Request::new("preparation_court")
                },
                true,
            )
            .unwrap();
            r.tick().unwrap();
            assert_eq!(
                r.telemetry.as_ref().unwrap().held_slots.unwrap_or(0),
                enabled & 4
            );
            assert!(r.homing_idle());
            assert!(r.homing.is_none());
        }
    }

    #[test]
    fn court_selection_releases_old_theta_hold_and_restart_keeps_estop_latched() {
        let mut r = prepared();
        r.send("ENABLE 6 1").unwrap();
        r.send("RUN").unwrap();
        r.tick().unwrap();
        r.request(
            &Request {
                text: Some("blue".into()),
                ..Request::new("preparation_court")
            },
            true,
        )
        .unwrap();
        r.tick().unwrap();
        assert_eq!(r.telemetry.as_ref().unwrap().held_slots, Some(4));
        r.engage_emergency().unwrap();
        r.tick().unwrap();
        r.request(&Request::new("preparation_restart"), true)
            .unwrap();
        r.tick().unwrap();
        assert!(r.emergency);
        assert_eq!(r.telemetry.as_ref().unwrap().held_slots, Some(0));
    }

    #[test]
    fn returning_to_lost_origins_releases_theta_without_dropping_z() {
        let mut r = prepared();
        r.send("ENABLE 6 1").unwrap();
        r.send("RUN").unwrap();
        r.tick().unwrap();
        r.machine.invalidate_origins();
        r.preparation = PreparationPhase::Recovery;
        r.request(&Request::new("preparation_return"), true)
            .unwrap();
        r.tick().unwrap();
        assert_eq!(r.telemetry.as_ref().unwrap().held_slots, Some(4));
        assert_ne!(r.telemetry.as_ref().unwrap().mode, RunMode::Run);
        assert_eq!(r.preparation_step(), PreparationStep::Home);
    }
}

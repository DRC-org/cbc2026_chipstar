use super::*;

impl Runtime {
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
        anyhow::ensure!(
            manual && !self.authority.active(),
            "準備の切替は人間がGUIから行ってください"
        );
        match req.action.as_str() {
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
                anyhow::ensure!(req.flag == Some(true), "開始姿勢と退避を確認してください");
                self.manual_input = ControllerState::default();
                self.screen_input_times = [None; 6];
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
                self.stop(false)?;
                self.preparation = PreparationPhase::Setting;
                Ok(Reply::data(
                    "準備操作に戻りました。待機中の作業は審判の許可に従ってください".into(),
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
            .request(
                &Request {
                    flag: Some(true),
                    ..Request::new("preparation_wait")
                },
                true,
            )
            .unwrap();
    }

    #[test]
    fn waiting_sends_no_motion_and_blocks_all_other_control_paths() {
        let mut r = prepared();
        let before = r.shared.status_snapshot().tx_count;
        wait(&mut r);
        assert_eq!(r.shared.status_snapshot().tx_count, before);
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
    fn confirmation_origins_and_human_authority_are_required() {
        let mut r = prepared();
        assert!(r.request(&Request::new("preparation_wait"), true).is_err());
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
}

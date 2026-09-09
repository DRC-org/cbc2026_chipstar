//! 準備ガイドの入力は通常操縦より先に処理する。決定は中立へ戻った時だけ実行する。
use super::*;

pub(super) struct Guide {
    pub enabled: bool,
    pub confirmed: bool,
    armed: bool,
    pending: Option<(usize, Instant)>,
    context: Option<(PreparationPhase, PreparationStep, bool, bool, bool)>,
}
impl Default for Guide {
    fn default() -> Self {
        Self {
            enabled: true,
            confirmed: false,
            armed: false,
            pending: None,
            context: None,
        }
    }
}
impl Guide {
    pub fn reset_input(&mut self) {
        self.armed = false;
        self.pending = None;
        self.confirmed = false;
        self.context = None;
    }
}

impl Runtime {
    pub(super) fn operation_feedback(&mut self, cue: u8) {
        if self.fresh()
            && self.device.as_ref().is_some_and(|d| d.tone)
            && let Err(error) = self.send(&format!("TONE {cue}"))
        {
            self.shared.log(format!("操作音を送信できません: {error}"));
        }
    }

    /// trueなら入力を消費済み。falseのときだけ既存の機体操縦へ渡す。
    pub(super) fn read_guide(&mut self, input: &ControllerState, now: Instant) -> Result<bool> {
        if !self.guide.enabled {
            self.guide.reset_input();
            return Ok(false);
        }
        if self.authority.active() || self.test.enabled || self.sts.control_busy() {
            self.guide.reset_input();
            return Ok(true);
        }
        let moving = self.drive.running() || self.drive.awaiting().is_some();
        let context = (
            self.preparation,
            self.preparation_step(),
            moving,
            self.homing.is_some() || self.sequence.is_some(),
            self.emergency,
        );
        if self.guide.context != Some(context) {
            self.guide.reset_input();
            self.guide.context = Some(context);
            self.pad.ee_armed = false;
        }
        let neutral_axes = input.axes.iter().all(|v| v.is_finite() && v.abs() < 0.1);
        let released = neutral_axes && input.buttons.iter().all(|b| *b == 0);
        if !self.guide.armed {
            self.guide.armed = released;
            return Ok(true);
        }
        // ○だけは操縦・ホーミング中も「停止して最初へ戻る」として受け付ける。
        if (moving || self.homing.is_some() || self.sequence.is_some())
            && input.buttons[1] == 0
            && !self.guide.pending.is_some_and(|(button, _)| button == 1)
        {
            return Ok(self.homing.is_some() || self.sequence.is_some());
        }
        if let Some((button, since)) = self.guide.pending {
            let only_button = neutral_axes
                && input
                    .buttons
                    .iter()
                    .enumerate()
                    .all(|(i, b)| *b == 0 || i == button);
            if !only_button {
                self.operation_feedback(2);
                self.guide.reset_input();
                return Ok(true);
            }
            let long = matches!(button, 0 | 1 | 2 | 4 | 6);
            if input.buttons[button] != 0 {
                let confirmed = !long || now.duration_since(since) >= Duration::from_secs(1);
                if long && confirmed && !self.guide.confirmed {
                    self.operation_feedback(3);
                }
                self.guide.confirmed = confirmed;
                return Ok(true);
            }
            let confirmed = !long || now.duration_since(since) >= Duration::from_secs(1);
            self.guide.reset_input();
            if confirmed {
                self.guide_action(button)?;
                self.error.clear();
            } else {
                self.operation_feedback(2);
            }
            return Ok(true);
        }
        if neutral_axes {
            let pressed: Vec<_> = input
                .buttons
                .iter()
                .enumerate()
                .filter(|(_, b)| **b != 0)
                .map(|(i, _)| i)
                .collect();
            if pressed.len() == 1 && matches!(pressed[0], 0 | 1 | 2 | 4 | 6 | 13 | 14) {
                self.guide.pending = Some((pressed[0], now));
            } else if !pressed.is_empty() {
                self.guide.reset_input();
            }
        } else {
            self.guide.reset_input();
        }
        Ok(true)
    }

    fn guide_action(&mut self, button: usize) -> Result<()> {
        if button == 1 {
            self.request(&Request::new("preparation_restart"), true)?;
            return Ok(());
        }
        if self.emergency {
            if button == 2 {
                self.request(&Request::new("estop_reset"), true)?;
            }
            return Ok(());
        }
        match (self.preparation, button) {
            (PreparationPhase::Waiting, 6) => {
                self.request(&Request::new("preparation_start"), true)?;
            }
            (PreparationPhase::Recovery, 6) => {
                self.request(&Request::new("preparation_return"), true)?;
            }
            (PreparationPhase::Active, 6) => {
                self.request(&Request::new("run"), true)?;
            }
            (PreparationPhase::Setting, 13 | 14) if self.court.is_none() => {
                self.request(
                    &Request {
                        text: Some(if button == 14 { "blue" } else { "red" }.into()),
                        ..Request::new("preparation_court")
                    },
                    true,
                )?;
            }
            (PreparationPhase::Setting, 4) => {
                self.request(
                    &Request {
                        flag: Some(true),
                        value: Some(180.0),
                        ..Request::new("home")
                    },
                    true,
                )?;
            }
            (PreparationPhase::Setting, 6) if self.court.is_some() => {
                self.request(&Request::new("run"), true)?;
            }
            (PreparationPhase::Setting, 0) => {
                self.request(&Request::new("preparation_wait"), true)?;
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn preparation_step(&self) -> PreparationStep {
        if self.court.is_none() {
            return PreparationStep::Court;
        }
        if self.homing.is_some() {
            return PreparationStep::Home;
        }
        if self.axes_ready(false).is_err() {
            return PreparationStep::Connection;
        }
        if self
            .machine
            .origin_states(self.telemetry.as_ref())
            .iter()
            .any(|o| !o.captured)
        {
            return PreparationStep::Home;
        }
        PreparationStep::Position
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn runtime() -> Runtime {
        let mut r = super::super::tests::screen_runtime();
        r.guide = Guide::default();
        r.screen_control = false;
        r.gamepad_name = "DualSense test".into();
        r.court = None;
        r
    }
    fn button(index: usize) -> ControllerState {
        let mut input = ControllerState::default();
        input.buttons[index] = 1;
        input
    }
    fn press(r: &mut Runtime, button_index: usize, duration: u64) -> Result<()> {
        let now = Instant::now();
        r.read_pad(ControllerState::default(), now)?;
        r.read_pad(button(button_index), now)?;
        r.read_pad(button(button_index), now + Duration::from_millis(duration))?;
        r.read_pad(
            ControllerState::default(),
            now + Duration::from_millis(duration),
        )
    }
    fn origins(r: &mut Runtime) {
        for i in 0..3 {
            assert!(r.machine.capture_origin(i, r.telemetry.as_ref()));
        }
    }
    #[test]
    fn controller_walks_from_court_to_waiting_and_explicit_start() {
        let mut r = runtime();
        press(&mut r, 14, 1).unwrap();
        assert_eq!(r.court, Some(Court::Blue));
        assert!(r.ee.targets.is_empty());
        assert_eq!(r.court, Some(Court::Blue));
        assert_eq!(r.court.unwrap().homing_theta(), 90.0);
        assert_eq!(Court::Red.homing_theta(), -90.0);
        assert_eq!(r.preparation_step(), PreparationStep::Home);
        assert!(press(&mut r, 0, 1000).is_err());
        assert_eq!(r.preparation_step(), PreparationStep::Home);
        // ホーミング完了時と同じ原点状態を与え、後続のガイド遷移を検証する。
        origins(&mut r);
        assert_eq!(r.preparation_step(), PreparationStep::Position);
        press(&mut r, 6, 999).unwrap();
        assert!(r.drive.awaiting().is_none());
        press(&mut r, 6, 1000).unwrap();
        assert!(r.drive.awaiting().is_some());
        r.tick().unwrap();
        assert!(r.drive.running());
        press(&mut r, 0, 1000).unwrap();
        assert_eq!(r.preparation_step(), PreparationStep::Position);
        press(&mut r, 5, 1).unwrap();
        r.tick().unwrap();
        press(&mut r, 0, 999).unwrap();
        assert_eq!(r.preparation, PreparationPhase::Setting);
        press(&mut r, 0, 1000).unwrap();
        assert_eq!(r.preparation, PreparationPhase::Waiting);
        press(&mut r, 6, 999).unwrap();
        assert_eq!(r.preparation, PreparationPhase::Waiting);
        press(&mut r, 6, 1000).unwrap();
        assert_eq!(r.preparation, PreparationPhase::Active);
        assert!(r.drive.awaiting().is_some());
    }
    #[test]
    fn held_buttons_changes_of_context_and_deflection_never_confirm() {
        let mut r = runtime();
        let now = Instant::now();
        r.read_pad(button(14), now).unwrap();
        r.read_pad(button(14), now + Duration::from_secs(3))
            .unwrap();
        r.read_pad(ControllerState::default(), now).unwrap();
        assert!(r.court.is_none());
        r.read_pad(button(14), now).unwrap();
        r.read_pad(ControllerState::default(), now).unwrap();
        assert_eq!(r.preparation_step(), PreparationStep::Home);
        // コート選択を離した直後から押されているCreateは実行しない。
        r.read_pad(button(4), now).unwrap();
        r.read_pad(button(4), now + Duration::from_secs(2)).unwrap();
        r.read_pad(ControllerState::default(), now).unwrap();
        assert!(r.homing.is_none());
        r.read_pad(button(4), now).unwrap();
        let mut mixed = button(4);
        mixed.axes[0] = 0.5;
        r.read_pad(mixed, now).unwrap();
        r.read_pad(ControllerState::default(), now).unwrap();
        assert!(r.homing.is_none());
        assert!(r.manual_input.axes.iter().all(|v| *v == 0.0));
    }
    #[test]
    fn homing_requires_long_release_and_ps_cancels_pending_action() {
        let mut r = runtime();
        r.court = Some(Court::Red);
        press(&mut r, 4, 999).unwrap();
        assert!(r.homing.is_none());
        let now = Instant::now();
        r.read_pad(ControllerState::default(), now).unwrap();
        r.read_pad(button(4), now).unwrap();
        r.read_pad(button(4), now + Duration::from_secs(1)).unwrap();
        assert!(r.homing.is_none());
        assert!(r.guide.confirmed);
        let mut stop = button(4);
        stop.buttons[5] = 1;
        r.read_pad(stop, now + Duration::from_secs(1)).unwrap();
        r.read_pad(ControllerState::default(), now).unwrap();
        assert!(r.homing.is_none());
        press(&mut r, 4, 1000).unwrap();
        assert!(r.homing.is_some());
        press(&mut r, 5, 1).unwrap();
        assert!(r.homing.is_none());
    }
    #[test]
    fn waiting_back_disconnect_and_emergency_require_new_confirmation() {
        let mut r = runtime();
        r.court = Some(Court::Red);
        origins(&mut r);
        r.preparation = PreparationPhase::Waiting;
        let now = Instant::now();
        r.read_pad(ControllerState::default(), now).unwrap();
        r.read_pad(button(6), now).unwrap();
        r.disconnect_pad();
        r.read_pad(button(6), now + Duration::from_secs(2)).unwrap();
        r.read_pad(ControllerState::default(), now).unwrap();
        assert_eq!(r.preparation, PreparationPhase::Waiting);
        press(&mut r, 1, 1000).unwrap();
        assert_eq!(r.preparation, PreparationPhase::Setting);
        assert_eq!(r.preparation_step(), PreparationStep::Court);
        r.engage_emergency().unwrap();
        press(&mut r, 6, 1000).unwrap();
        assert!(r.emergency);
        press(&mut r, 2, 1000).unwrap();
        assert!(!r.emergency);
        assert!(!r.drive.running() && r.drive.awaiting().is_none());
    }
    #[test]
    fn leaving_guide_preserves_debug_controls_but_not_waiting_bypass() {
        let mut r = runtime();
        r.court = Some(Court::Red);
        origins(&mut r);
        let request = |enabled| Request {
            flag: Some(enabled),
            ..Request::new("preparation_guide")
        };
        assert!(r.request(&request(false), false).is_err());
        r.authority.claim("test".into(), Instant::now());
        r.request(&request(false), true).unwrap();
        r.authority.release();
        assert!(!r.guide.enabled);
        press(&mut r, 6, 1).unwrap();
        assert!(r.drive.awaiting().is_some());
        r.stop(false).unwrap();
        r.tick().unwrap();
        r.preparation = PreparationPhase::Waiting;
        press(&mut r, 6, 1000).unwrap();
        assert_eq!(r.preparation, PreparationPhase::Waiting);
        assert!(r.drive.awaiting().is_none());
        r.request(&request(true), true).unwrap();
        // 画面へ戻った時点で押されているOptionsは決定に使わない。
        let now = Instant::now();
        r.read_pad(button(6), now).unwrap();
        r.read_pad(button(6), now + Duration::from_secs(2)).unwrap();
        r.read_pad(ControllerState::default(), now).unwrap();
        assert_eq!(r.preparation, PreparationPhase::Waiting);
    }
    #[test]
    fn mouse_and_pad_share_actions_and_restart_cancels_motion_and_homing() {
        for homing in [false, true] {
            let mut r = runtime();
            r.screen_control = true;
            // マウスでコートを選び、画面操縦の設定のままパッドでホーミングできる。
            r.request(
                &Request {
                    text: Some("red".into()),
                    ..Request::new("preparation_court")
                },
                true,
            )
            .unwrap();
            assert_eq!(r.preparation_step(), PreparationStep::Home);
            if homing {
                press(&mut r, 4, 1000).unwrap();
                assert!(r.homing.is_some());
                press(&mut r, 1, 1000).unwrap();
            } else {
                origins(&mut r);
                r.request(&Request::new("run"), true).unwrap();
                r.tick().unwrap();
                assert!(r.drive.running());
                r.request(&Request::new("preparation_restart"), true)
                    .unwrap();
            }
            assert!(r.homing.is_none());
            assert!(!r.drive.running() && r.drive.awaiting().is_none());
            assert_eq!(r.preparation, PreparationPhase::Setting);
            assert_eq!(r.preparation_step(), PreparationStep::Court);
            assert!(r.court.is_none());
            assert!(
                r.machine
                    .origin_states(r.telemetry.as_ref())
                    .iter()
                    .all(|o| !o.captured)
            );
        }
    }

    #[test]
    fn guide_does_not_clear_mouse_jog_and_waiting_can_restart_by_mouse() {
        let mut r = runtime();
        r.screen_control = true;
        r.manual_input.axes[0] = 0.5;
        r.read_pad(ControllerState::default(), Instant::now())
            .unwrap();
        assert_eq!(r.manual_input.axes[0], 0.5);
        r.manual_input = ControllerState::default();
        r.court = Some(Court::Blue);
        origins(&mut r);
        r.request(&Request::new("preparation_wait"), true).unwrap();
        assert!(r.preparation.locked());
        assert!(
            r.request(&Request::new("preparation_restart"), false)
                .is_err()
        );
        r.request(&Request::new("preparation_restart"), true)
            .unwrap();
        assert!(!r.preparation.locked());
        assert!(r.court.is_none());
    }
    #[test]
    fn operation_sound_matches_mouse_pad_rejection_and_hold_threshold_once() {
        let mut r = runtime();
        assert!(r.device.as_ref().unwrap().tone);
        let cues = |r: &Runtime| {
            r.shared
                .status_snapshot()
                .logs
                .iter()
                .filter(|line| line.starts_with("TX TONE "))
                .cloned()
                .collect::<Vec<_>>()
        };
        r.request(
            &Request {
                text: Some("blue".into()),
                ..Request::new("preparation_court")
            },
            true,
        )
        .unwrap();
        assert_eq!(cues(&r), vec!["TX TONE 1"]);
        let now = Instant::now();
        r.read_pad(ControllerState::default(), now).unwrap();
        r.read_pad(button(4), now).unwrap();
        r.read_pad(button(4), now + Duration::from_secs(1)).unwrap();
        r.read_pad(button(4), now + Duration::from_secs(2)).unwrap();
        assert_eq!(cues(&r), vec!["TX TONE 1", "TX TONE 3"]);
        r.read_pad(ControllerState::default(), now + Duration::from_secs(2))
            .unwrap();
        assert!(r.homing.is_some());
        assert_eq!(cues(&r), vec!["TX TONE 1", "TX TONE 3", "TX TONE 1"]);
        assert!(r.request(&Request::new("preparation_wait"), true).is_err());
        assert_eq!(cues(&r).last().unwrap(), "TX TONE 2");
        let count = cues(&r).len();
        r.request(
            &Request {
                flag: Some(false),
                ..Request::new("preparation_guide")
            },
            true,
        )
        .unwrap();
        r.request(&Request::new("stop"), true).unwrap();
        assert_eq!(cues(&r).len(), count);
        r.guide.enabled = true;
        r.device.as_mut().unwrap().tone = false;
        r.operation_feedback(1);
        assert_eq!(cues(&r).len(), count);
    }
}

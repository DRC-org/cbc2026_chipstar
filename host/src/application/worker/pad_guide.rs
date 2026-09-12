//! 準備ガイドの入力は通常操縦より先に処理する。決定は離した時、準備完了後のやり直しは長押しで実行する。
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
    pub fn restart_pending(&self) -> bool {
        self.pending.is_some_and(|(button, _)| button == 1)
    }

    pub fn reset_input(&mut self) {
        self.armed = false;
        self.pending = None;
        self.confirmed = false;
        self.context = None;
    }
}

impl Runtime {
    pub(super) fn guide_restart_hold_required(&self) -> bool {
        !self.emergency
            && self.homing.is_none()
            && self.sequence.is_none()
            && (self.preparation == PreparationPhase::Waiting
                || self.preparation_step() == PreparationStep::Position)
    }

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
        let automatic = self.homing.is_some() || self.sequence.is_some();
        let context = (
            self.preparation,
            self.preparation_step(),
            moving,
            automatic,
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
        // 操縦中の○はEE、R1+○はボーナスに渡す。準備へ戻る操作はPS停止後に行う。
        if moving && !automatic {
            return Ok(false);
        }
        // 自動動作中は機構操作へ渡さず、○による中断だけを受け付ける。
        if automatic && input.buttons[1] == 0 && !self.guide.restart_pending() {
            return Ok(true);
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
            if button == 1 && self.guide_restart_hold_required() {
                if input.buttons[button] == 0 {
                    // 短押しは取消。ホーミング済みの原点と保持姿勢を変えない。
                    self.guide.reset_input();
                } else if now.saturating_duration_since(since) >= Duration::from_secs(1) {
                    self.guide.reset_input();
                    self.guide_action(button)?;
                    self.error.clear();
                }
                return Ok(true);
            }
            if input.buttons[button] != 0 {
                return Ok(true);
            }
            self.guide.reset_input();
            self.guide_action(button)?;
            self.error.clear();
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
            if pressed.len() == 1 && matches!(pressed[0], 0 | 1 | 2 | 13 | 14) {
                self.guide.pending = Some((pressed[0], now));
                self.guide.confirmed = !(pressed[0] == 1 && self.guide_restart_hold_required());
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
            if button == 0 {
                self.request(&Request::new("estop_reset"), true)?;
            }
            return Ok(());
        }
        match (self.preparation, button) {
            (PreparationPhase::Waiting, 0) => {
                self.request(&Request::new("preparation_start"), true)?;
            }
            (PreparationPhase::Recovery, 0) => {
                self.request(&Request::new("preparation_return"), true)?;
            }
            (PreparationPhase::Active, 0) => {
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
            (PreparationPhase::Setting, 0) if self.preparation_step() == PreparationStep::Home => {
                self.request(
                    &Request {
                        flag: Some(true),
                        value: Some(180.0),
                        ..Request::new("home")
                    },
                    true,
                )?;
            }
            (PreparationPhase::Setting, 2) if self.preparation_step() == PreparationStep::Home => {
                self.request(&Request::new("preparation_manual_begin"), true)?;
            }
            (PreparationPhase::Setting, 0)
                if self.preparation_step() == PreparationStep::ManualHome =>
            {
                self.request(&Request::new("preparation_manual_theta"), true)?;
            }
            (PreparationPhase::Setting, 0)
                if self.preparation_step() == PreparationStep::Position =>
            {
                self.request(&Request::new("run"), true)?;
            }
            (PreparationPhase::Setting, 2)
                if self.preparation_step() == PreparationStep::Position =>
            {
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
        if self.manual_origins {
            return PreparationStep::ManualHome;
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
    fn controller_can_set_manual_origins_and_start_without_carried_button_presses() {
        let mut r = runtime();
        for axis in &mut r.cfg.machine.axes {
            if let Some(limit) = &mut axis.limit {
                limit.normally_closed = false;
            }
            if axis.name == "theta" {
                axis.origin_position = 0.0;
                axis.limit = None;
            }
        }
        r.machine.reconfigure(r.cfg.machine.clone());
        press(&mut r, 14, 1).unwrap();
        let now = Instant::now();
        r.read_pad(ControllerState::default(), now).unwrap();
        r.read_pad(button(2), now).unwrap();
        r.read_pad(button(2), now + Duration::from_secs(1)).unwrap();
        assert!(!r.manual_origins && r.homing.is_none());
        r.read_pad(ControllerState::default(), now + Duration::from_secs(1))
            .unwrap();
        r.tick().unwrap();
        assert_eq!(r.preparation_step(), PreparationStep::ManualHome);
        assert!(r.machine.origin_states(r.telemetry.as_ref())[0].captured);
        assert!(r.machine.origin_states(r.telemetry.as_ref())[2].captured);
        press(&mut r, 0, 1).unwrap();
        assert_eq!(r.preparation_step(), PreparationStep::Position);
        assert!(r.homing.is_none() && r.front_return_pending.is_none());
        // 次の画面で押しっぱなしの×を操縦開始として使わない。
        let now = Instant::now();
        r.read_pad(button(0), now).unwrap();
        r.read_pad(button(0), now + Duration::from_secs(1)).unwrap();
        r.read_pad(ControllerState::default(), now + Duration::from_secs(1))
            .unwrap();
        assert!(!r.drive.running() && r.drive.awaiting().is_none());
        press(&mut r, 2, 1).unwrap();
        assert_eq!(r.preparation, PreparationPhase::Waiting);
        press(&mut r, 0, 1).unwrap();
        assert!(r.drive.awaiting().is_some() && r.homing.is_none());
        assert!(
            r.shared
                .status_snapshot()
                .logs
                .iter()
                .any(|line| line == "TX TONE 1")
        );
    }

    #[test]
    fn controller_walks_from_court_to_waiting_and_explicit_start() {
        let mut r = runtime();
        // 実機で調整した角度に依存せず、コートごとの符号と操作フローを検証する。
        r.cfg.machine.homing_theta_deg = 90.0;
        press(&mut r, 14, 1).unwrap();
        assert_eq!(r.court, Some(Court::Blue));
        assert!(r.ee.targets.is_empty());
        assert_eq!(r.court, Some(Court::Blue));
        assert_eq!(
            r.court
                .unwrap()
                .homing_theta(r.cfg.machine.homing_theta_deg),
            90.0
        );
        assert_eq!(
            Court::Red.homing_theta(r.cfg.machine.homing_theta_deg),
            -90.0
        );
        assert_eq!(r.preparation_step(), PreparationStep::Home);
        assert!(r.request(&Request::new("preparation_wait"), true).is_err());
        assert_eq!(r.preparation_step(), PreparationStep::Home);
        // ホーミング完了時と同じ原点状態を与え、後続のガイド遷移を検証する。
        origins(&mut r);
        assert_eq!(r.preparation_step(), PreparationStep::Position);
        press(&mut r, 0, 1).unwrap();
        assert!(r.drive.awaiting().is_some());
        r.tick().unwrap();
        assert!(r.drive.running());
        press(&mut r, 0, 1).unwrap();
        assert_eq!(r.preparation_step(), PreparationStep::Position);
        press(&mut r, 5, 1).unwrap();
        r.tick().unwrap();
        press(&mut r, 2, 1).unwrap();
        assert_eq!(r.preparation, PreparationPhase::Waiting);
        press(&mut r, 0, 1).unwrap();
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
        // コート選択を離した直後から押されている×は実行しない。
        r.read_pad(button(0), now).unwrap();
        r.read_pad(button(0), now + Duration::from_secs(2)).unwrap();
        r.read_pad(ControllerState::default(), now).unwrap();
        assert!(r.homing.is_none());
        r.read_pad(button(0), now).unwrap();
        let mut mixed = button(0);
        mixed.axes[0] = 0.5;
        r.read_pad(mixed, now).unwrap();
        r.read_pad(ControllerState::default(), now).unwrap();
        assert!(r.homing.is_none());
        assert!(r.manual_input.axes.iter().all(|v| *v == 0.0));
    }
    #[test]
    fn homing_requires_release_and_ps_cancels_pending_action() {
        let mut r = runtime();
        r.machine.invalidate_origins();
        r.court = Some(Court::Red);
        let now = Instant::now();
        r.read_pad(ControllerState::default(), now).unwrap();
        r.read_pad(button(0), now).unwrap();
        r.read_pad(button(0), now + Duration::from_secs(1)).unwrap();
        assert!(r.homing.is_none());
        assert!(r.guide.confirmed);
        let mut stop = button(0);
        stop.buttons[5] = 1;
        r.read_pad(stop, now + Duration::from_secs(1)).unwrap();
        r.read_pad(ControllerState::default(), now).unwrap();
        assert!(r.homing.is_none());
        press(&mut r, 0, 1).unwrap();
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
        r.read_pad(button(0), now).unwrap();
        r.disconnect_pad();
        r.read_pad(button(0), now + Duration::from_secs(2)).unwrap();
        r.read_pad(ControllerState::default(), now).unwrap();
        assert_eq!(r.preparation, PreparationPhase::Waiting);
        press(&mut r, 1, 1000).unwrap();
        assert_eq!(r.preparation, PreparationPhase::Setting);
        assert_eq!(r.preparation_step(), PreparationStep::Court);
        r.engage_emergency().unwrap();
        press(&mut r, 6, 1).unwrap();
        assert!(r.emergency);
        press(&mut r, 0, 1).unwrap();
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
        press(&mut r, 6, 1).unwrap();
        assert_eq!(r.preparation, PreparationPhase::Waiting);
        assert!(r.drive.awaiting().is_none());
        r.request(&request(true), true).unwrap();
        // 画面へ戻った時点で押されている×は決定に使わない。
        let now = Instant::now();
        r.read_pad(button(0), now).unwrap();
        r.read_pad(button(0), now + Duration::from_secs(2)).unwrap();
        r.read_pad(ControllerState::default(), now).unwrap();
        assert_eq!(r.preparation, PreparationPhase::Waiting);
    }
    #[test]
    fn ps_then_fresh_circle_restarts_without_reusing_a_held_circle() {
        let mut r = runtime();
        r.court = Some(Court::Red);
        origins(&mut r);
        r.drive = DriveState::Running;
        let now = Instant::now();
        let mut stop = button(1);
        stop.buttons[5] = 1;
        r.read_pad(stop, now).unwrap();
        r.read_pad(button(1), now).unwrap();
        r.read_pad(button(1), now + Duration::from_secs(1)).unwrap();
        r.read_pad(ControllerState::default(), now).unwrap();
        assert!(!r.drive.running());
        assert_eq!(r.court, Some(Court::Red));
        assert!(
            r.machine
                .origin_states(r.telemetry.as_ref())
                .iter()
                .all(|o| o.captured)
        );
        press(&mut r, 1, 1000).unwrap();
        assert!(r.court.is_none());
        assert!(
            r.machine
                .origin_states(r.telemetry.as_ref())
                .iter()
                .all(|o| !o.captured)
        );
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
                press(&mut r, 0, 1).unwrap();
                assert!(r.homing.is_some());
                press(&mut r, 1, 1).unwrap();
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
    fn operation_sound_matches_mouse_and_tap_without_repeating_while_held() {
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
        r.read_pad(button(0), now).unwrap();
        r.read_pad(button(0), now + Duration::from_secs(1)).unwrap();
        r.read_pad(button(0), now + Duration::from_secs(2)).unwrap();
        assert_eq!(cues(&r), vec!["TX TONE 1"]);
        r.read_pad(ControllerState::default(), now + Duration::from_secs(2))
            .unwrap();
        assert!(r.homing.is_some());
        assert_eq!(cues(&r), vec!["TX TONE 1", "TX TONE 1"]);
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

    #[test]
    fn post_homing_circle_tap_preserves_origins_and_ee_hold() {
        for phase in [PreparationPhase::Setting, PreparationPhase::Waiting] {
            let mut r = runtime();
            r.court = Some(Court::Blue);
            origins(&mut r);
            r.preparation = phase;
            r.ee.targets.insert("ee_rotation".into(), 1500.0);
            let before = r.shared.status_snapshot().logs.clone();
            press(&mut r, 1, 999).unwrap();
            assert_eq!(r.preparation, phase);
            assert_eq!(r.court, Some(Court::Blue));
            assert!(
                r.machine
                    .origin_states(r.telemetry.as_ref())
                    .iter()
                    .all(|origin| origin.captured)
            );
            assert_eq!(r.ee.targets["ee_rotation"], 1500.0);
            assert_eq!(r.shared.status_snapshot().logs, before);
            r.publish();
            assert!(r.shared.status_snapshot().guide_restart_hold);
            assert!(!r.shared.status_snapshot().guide_restart_holding);
            assert!(!r.shared.status_snapshot().guide_release);
        }
    }

    #[test]
    fn circle_restarts_at_one_second_once_and_reports_the_hold_gesture() {
        let mut r = runtime();
        r.court = Some(Court::Red);
        origins(&mut r);
        r.preparation = PreparationPhase::Waiting;
        let now = Instant::now();
        r.read_pad(ControllerState::default(), now).unwrap();
        r.read_pad(button(1), now).unwrap();
        r.publish();
        assert!(r.shared.status_snapshot().guide_restart_holding);
        assert!(!r.shared.status_snapshot().guide_release);
        r.read_pad(button(1), now + Duration::from_millis(999))
            .unwrap();
        assert_eq!(r.court, Some(Court::Red));
        r.read_pad(button(1), now + Duration::from_millis(1000))
            .unwrap();
        assert!(r.court.is_none());
        assert_eq!(r.preparation_step(), PreparationStep::Court);
        assert!(
            r.machine
                .origin_states(r.telemetry.as_ref())
                .iter()
                .all(|origin| !origin.captured)
        );
        let before = r.shared.status_snapshot().logs.clone();
        assert_eq!(before.iter().filter(|line| *line == "TX TONE 1").count(), 1);
        r.read_pad(button(1), now + Duration::from_secs(2)).unwrap();
        r.read_pad(ControllerState::default(), now + Duration::from_secs(2))
            .unwrap();
        assert_eq!(r.shared.status_snapshot().logs, before);
    }

    #[test]
    fn circle_hold_is_cancelled_by_deflection_ps_disconnect_or_context_change() {
        for case in ["stick", "ps", "disconnect", "context"] {
            let mut r = runtime();
            r.court = Some(Court::Blue);
            origins(&mut r);
            r.preparation = PreparationPhase::Waiting;
            let now = Instant::now();
            r.read_pad(ControllerState::default(), now).unwrap();
            r.read_pad(button(1), now).unwrap();
            match case {
                "stick" => {
                    let mut input = button(1);
                    input.axes[0] = 0.5;
                    r.read_pad(input, now + Duration::from_millis(400)).unwrap();
                }
                "ps" => {
                    let mut input = button(1);
                    input.buttons[5] = 1;
                    r.read_pad(input, now + Duration::from_millis(400)).unwrap();
                }
                "disconnect" => r.disconnect_pad(),
                _ => r.preparation = PreparationPhase::Recovery,
            }
            r.read_pad(button(1), now + Duration::from_secs(2)).unwrap();
            r.read_pad(ControllerState::default(), now + Duration::from_secs(2))
                .unwrap();
            assert_eq!(r.court, Some(Court::Blue), "{case}");
            assert!(
                r.machine
                    .origin_states(r.telemetry.as_ref())
                    .iter()
                    .all(|origin| origin.captured)
            );
        }
    }
}

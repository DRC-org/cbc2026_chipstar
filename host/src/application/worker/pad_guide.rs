//! 準備ガイドの入力は通常操縦より先に処理する。決定は中立へ戻った時だけ実行する。
use super::*;

pub(super) struct Guide {
    pub enabled: bool,
    pub step: PreparationStep,
    pub blue: bool,
    pub confirmed: bool,
    armed: bool,
    pending: Option<(usize, Instant)>,
    context: Option<(PreparationPhase, PreparationStep, bool, bool, bool)>,
}
impl Default for Guide {
    fn default() -> Self {
        Self {
            enabled: true,
            step: PreparationStep::Court,
            blue: false,
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
    /// trueなら入力を消費済み。falseのときだけ既存の機体操縦へ渡す。
    pub(super) fn read_guide(&mut self, input: &ControllerState, now: Instant) -> Result<bool> {
        if !self.guide.enabled || self.screen_control {
            self.guide.reset_input();
            return Ok(false);
        }
        if self.authority.active()
            || self.test.enabled
            || self.sts.control_busy()
            || self.sequence.is_some()
        {
            self.guide.reset_input();
            return Ok(true);
        }
        let moving = self.drive.running() || self.drive.awaiting().is_some();
        let context = (
            self.preparation,
            self.guide.step,
            moving,
            self.homing.is_some(),
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
        if self.homing.is_some() {
            return Ok(true);
        }
        if moving {
            // 操縦中の×・○・Options・Createはガイド操作に使わない。PS停止後に再度中立を要求する。
            return Ok(false);
        }
        if self.preparation == PreparationPhase::Active {
            // PS停止後も明示した再開操作を使う。
            self.guide.step = PreparationStep::Position;
        }
        if let Some((button, since)) = self.guide.pending {
            let only_button = neutral_axes
                && input
                    .buttons
                    .iter()
                    .enumerate()
                    .all(|(i, b)| *b == 0 || i == button);
            if !only_button {
                self.guide.reset_input();
                return Ok(true);
            }
            let long = button == 2
                || button == 4
                || button == 6
                || (button == 0
                    && self.guide.step == PreparationStep::Finish
                    && self.preparation == PreparationPhase::Setting);
            if input.buttons[button] != 0 {
                self.guide.confirmed = !long || now.duration_since(since) >= Duration::from_secs(1);
                return Ok(true);
            }
            let confirmed = !long || now.duration_since(since) >= Duration::from_secs(1);
            self.guide.reset_input();
            if confirmed {
                self.guide_action(button)?;
                self.error.clear();
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
        use PreparationStep::*;
        if self.emergency {
            if button == 2 {
                self.request(&Request::new("estop_reset"), true)?;
            }
            return Ok(());
        }
        match self.preparation {
            PreparationPhase::Waiting => {
                if button == 6 {
                    self.preparation_request(&Request::new("preparation_start"), true)?;
                } else if button == 1 {
                    self.preparation_request(&Request::new("preparation_return"), true)?;
                    self.guide.step = Finish;
                }
                return Ok(());
            }
            PreparationPhase::Recovery => {
                if button == 1 {
                    self.preparation_request(&Request::new("preparation_return"), true)?;
                    self.guide.step = Connection;
                }
                return Ok(());
            }
            PreparationPhase::Active => {
                if button == 6 {
                    self.start()?;
                }
                if button == 1 {
                    self.preparation_request(&Request::new("preparation_return"), true)?;
                    self.guide.step = Position;
                }
                return Ok(());
            }
            PreparationPhase::Setting => {}
        }
        if button == 1 {
            self.guide.step = match self.guide.step {
                Court => Court,
                Connection => Court,
                Home => Connection,
                Position => Home,
                Finish => Position,
            };
            return Ok(());
        }
        match (self.guide.step, button) {
            (Court, 13) => self.guide.blue = false,
            (Court, 14) => self.guide.blue = true,
            (Court, 0) => {
                self.preparation_request(
                    &Request {
                        text: Some(if self.guide.blue { "blue" } else { "red" }.into()),
                        ..Request::new("preparation_court")
                    },
                    true,
                )?;
                self.guide.step = Connection;
            }
            (Connection, 0) => {
                self.axes_ready(false)?;
                anyhow::ensure!(self.settings.ready(), "機体設定の反映を待ってください");
                self.guide.step = Home;
            }
            (Home, 4) => {
                self.begin_homing(true, true, 180.0)?;
            }
            (Home, 0) => {
                self.ready()?;
                self.guide.step = Position;
            }
            (Position, 6) => {
                self.start()?;
            }
            (Position, 0) => {
                self.preparation_ready()?;
                self.guide.step = Finish;
            }
            (Finish, 0) => {
                self.preparation_request(&Request::new("preparation_wait"), true)?;
            }
            _ => {}
        }
        Ok(())
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
        assert!(r.guide.blue);
        assert!(r.ee.targets.is_empty());
        press(&mut r, 0, 1).unwrap();
        assert_eq!(r.court, Some(Court::Blue));
        assert_eq!(r.court.unwrap().homing_theta(), 90.0);
        assert_eq!(Court::Red.homing_theta(), -90.0);
        press(&mut r, 0, 1).unwrap();
        assert_eq!(r.guide.step, PreparationStep::Home);
        assert!(press(&mut r, 0, 1).is_err());
        assert_eq!(r.guide.step, PreparationStep::Home);
        // ホーミング完了時と同じ原点状態を与え、後続のガイド遷移を検証する。
        origins(&mut r);
        press(&mut r, 0, 1).unwrap();
        assert_eq!(r.guide.step, PreparationStep::Position);
        press(&mut r, 6, 999).unwrap();
        assert!(r.drive.awaiting().is_none());
        press(&mut r, 6, 1000).unwrap();
        assert!(r.drive.awaiting().is_some());
        r.tick().unwrap();
        assert!(r.drive.running());
        press(&mut r, 0, 1000).unwrap();
        assert_eq!(r.guide.step, PreparationStep::Position);
        press(&mut r, 5, 1).unwrap();
        r.tick().unwrap();
        press(&mut r, 0, 1).unwrap();
        assert_eq!(r.guide.step, PreparationStep::Finish);
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
        r.read_pad(button(0), now).unwrap();
        r.read_pad(button(0), now + Duration::from_secs(3)).unwrap();
        r.read_pad(ControllerState::default(), now).unwrap();
        assert!(r.court.is_none());
        r.read_pad(button(0), now).unwrap();
        r.read_pad(button(0), now + Duration::from_secs(3)).unwrap();
        assert!(r.court.is_none()); // 離す前は決定しない。
        r.read_pad(ControllerState::default(), now).unwrap();
        assert_eq!(r.guide.step, PreparationStep::Connection);
        r.read_pad(button(0), now).unwrap();
        r.read_pad(ControllerState::default(), now).unwrap();
        assert_eq!(r.guide.step, PreparationStep::Connection);
        r.read_pad(button(0), now).unwrap();
        let mut mixed = button(0);
        mixed.axes[0] = 0.5;
        r.read_pad(mixed, now).unwrap();
        r.read_pad(ControllerState::default(), now).unwrap();
        assert_eq!(r.guide.step, PreparationStep::Connection);
        assert!(r.manual_input.axes.iter().all(|v| *v == 0.0));
    }
    #[test]
    fn homing_requires_long_release_and_ps_cancels_pending_action() {
        let mut r = runtime();
        r.court = Some(Court::Red);
        r.guide.step = PreparationStep::Home;
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
        press(&mut r, 1, 1).unwrap();
        assert_eq!(r.preparation, PreparationPhase::Setting);
        assert_eq!(r.guide.step, PreparationStep::Finish);
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
}

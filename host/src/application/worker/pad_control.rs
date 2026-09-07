//! 操縦パッドの操作受付。確認操作と通常運転を分離する。
use super::*;

#[derive(Default)]
pub(super) struct Control {
    previous: [u8; 17],
    pub home_ready: bool,
    pub home_since: Option<Instant>,
    pub ee_armed: bool,
}

impl Runtime {
    pub(super) fn read_pad(&mut self, input: ControllerState, now: Instant) -> Result<()> {
        let previous = self.pad.previous;
        self.pad.previous = input.buttons;
        let buttons = input.buttons;
        if !self.screen_control {
            self.manual_input = input.clone();
        }
        // 停止は操作権に関係なく優先し、押下中は他の操作を受け付けない。
        if buttons[5] != 0 {
            self.pad.home_since = None;
            self.pad.home_ready = false;
            self.pad.ee_armed = false;
            if previous[5] == 0 {
                self.stop(false)?;
            }
            return Ok(());
        }
        if self.authority.active() || self.screen_control || self.emergency {
            self.pad.home_since = None;
            self.pad.home_ready = false;
            self.pad.ee_armed = false;
            return Ok(());
        }
        let stopped = !self.drive.running()
            && self.drive.awaiting().is_none()
            && !self.test.enabled
            && !self.sts.active
            && !self.sts.busy()
            && self.homing.is_none()
            && self.fresh()
            && self.settings.ready();
        // 停止状態で両ボタンを離してから確認する。接続時の押しっぱなしでは始めない。
        if stopped && buttons[4] == 0 {
            self.pad.home_ready = true;
        }
        let neutral = input.axes.iter().all(|v| v.is_finite() && v.abs() < 0.1)
            && buttons[11..15].iter().all(|b| *b == 0);
        if stopped && self.pad.home_ready && buttons[4] != 0 && neutral {
            let since = *self.pad.home_since.get_or_insert(now);
            if now.duration_since(since) >= Duration::from_secs(1) {
                self.pad.home_ready = false;
                self.pad.home_since = None;
                return self.begin_homing(true, true, 180.0).map(|_| ());
            }
        } else {
            self.pad.home_since = None;
            if !stopped {
                self.pad.home_ready = false;
            }
        }
        if buttons[6] != 0 && previous[6] == 0 {
            self.start()?;
        }
        if !self.drive.running()
            || self.test.enabled
            || self.sts.active
            || self.sts.busy()
            || self.homing.is_some()
        {
            self.pad.ee_armed = false;
            return Ok(());
        }
        let axes = crate::machine::ee::axes(&self.cfg.machine);
        if !self.pad.ee_armed {
            self.pad.ee_armed = axes.iter().all(|a| a.pad_value(&input).abs() < 0.1);
            return Ok(());
        }
        let mut targets = std::collections::BTreeMap::new();
        let grip_pressed = buttons[13] != buttons[14];
        if grip_pressed {
            // 3本一括の割当不足を、一部だけ駆動する前に検出する。
            for name in ["ee_grip_1", "ee_grip_2", "ee_grip_3"] {
                if !axes.iter().any(|a| a.name == name) {
                    self.pad.ee_armed = false;
                    bail!("把持3本のEE割当を設定してください");
                }
            }
        }
        for axis in &axes {
            if axis.pad_value(&input).abs() >= 0.1 && !self.ee.targets.contains_key(&axis.name) {
                targets.insert(axis.name.clone(), axis.initial);
            }
        }
        if !targets.is_empty() {
            #[derive(serde::Serialize)]
            struct Input {
                targets: std::collections::BTreeMap<String, f32>,
            }
            let req = Request {
                text: Some(toml::to_string(&Input { targets })?),
                ..Request::new("ee")
            };
            if let Err(error) = self.ee_request(&req) {
                self.pad.ee_armed = false;
                return Err(error);
            }
        }
        Ok(())
    }

    pub(super) fn disconnect_pad(&mut self) {
        if (!self.authority.active() && !self.screen_control && self.drive.running())
            || self.homing.is_some()
        {
            self.fault("DualSenseが切断されました".into());
        }
        self.pad = Control::default();
        self.gamepad_name.clear();
        if !self.screen_control {
            self.manual_input = ControllerState::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn runtime() -> Runtime {
        let mut r = super::super::tests::screen_runtime();
        r.screen_control = false;
        r.gamepad_name = "DualSense test".into();
        r
    }
    fn home_button() -> ControllerState {
        let mut input = ControllerState::default();
        input.buttons[4] = 1;
        input
    }
    #[test]
    fn home_requires_release_and_one_second_and_never_resumes() {
        let mut r = runtime();
        let now = Instant::now();
        r.read_pad(home_button(), now).unwrap();
        r.read_pad(home_button(), now + Duration::from_secs(3))
            .unwrap();
        assert!(r.homing.is_none());
        r.read_pad(ControllerState::default(), now).unwrap();
        r.read_pad(home_button(), now).unwrap();
        r.read_pad(home_button(), now + Duration::from_millis(999))
            .unwrap();
        assert!(r.homing.is_none());
        r.read_pad(home_button(), now + Duration::from_secs(1))
            .unwrap();
        assert!(r.homing.is_some());
        assert!(!r.drive.running() && r.drive.awaiting().is_none());
        r.stop(false).unwrap();
        r.read_pad(home_button(), now + Duration::from_secs(5))
            .unwrap();
        assert!(r.homing.is_none());
    }
    #[test]
    fn nonneutral_input_cancels_confirmation_and_disconnect_cancels_home() {
        let mut r = runtime();
        let now = Instant::now();
        r.read_pad(ControllerState::default(), now).unwrap();
        r.read_pad(home_button(), now).unwrap();
        let mut moved = home_button();
        moved.axes[0] = 0.5;
        r.read_pad(moved, now + Duration::from_secs(2)).unwrap();
        assert!(r.homing.is_none());
        assert!(r.pad.home_since.is_none());
        r.read_pad(home_button(), now).unwrap();
        r.read_pad(home_button(), now + Duration::from_secs(1))
            .unwrap();
        assert!(r.homing.is_some());
        r.disconnect_pad();
        assert!(r.homing.is_none());
        assert!(!r.drive.running());
    }
    fn add_grips(r: &mut Runtime) {
        for i in 1..=3 {
            r.cfg
                .machine
                .pwm_servos
                .push(crate::machine::PwmServoProfile {
                    name: format!("ee_grip_{i}"),
                    channel: i - 1,
                    input_axis: None,
                    input_sign: if i == 2 { -1.0 } else { 1.0 },
                    speed_us_per_second: 100.0,
                    minimum_us: 1400,
                    maximum_us: 1600,
                    initial_us: 1500,
                    enabled: true,
                });
        }
        r.test.peers.insert("pwm", Instant::now());
        r.drive = DriveState::Running;
    }
    #[test]
    fn grips_require_neutral_and_all_three_permissions_and_apply_sign_and_slow() {
        let mut r = runtime();
        add_grips(&mut r);
        let now = Instant::now();
        let mut input = ControllerState::default();
        input.buttons[14] = 1;
        r.read_pad(input.clone(), now).unwrap();
        assert!(r.ee.targets.is_empty());
        r.read_pad(ControllerState::default(), now).unwrap();
        r.cfg.machine.pwm_servos[2].enabled = false;
        assert!(r.read_pad(input.clone(), now).is_err());
        assert!(r.ee.targets.is_empty());
        r.cfg.machine.pwm_servos[2].enabled = true;
        r.read_pad(ControllerState::default(), now).unwrap();
        r.read_pad(input.clone(), now).unwrap();
        assert_eq!(r.ee.targets.len(), 3);
        r.tick_ee(now).unwrap();
        let initial = r.ee.targets["ee_grip_1"];
        let initial_reverse = r.ee.targets["ee_grip_2"];
        r.tick_ee(now + Duration::from_millis(100)).unwrap();
        assert_eq!(r.ee.targets["ee_grip_1"], initial + 10.0);
        assert_eq!(r.ee.targets["ee_grip_2"], initial_reverse - 10.0);
        input.buttons[9] = 1;
        r.read_pad(input, now).unwrap();
        r.tick_ee(now + Duration::from_millis(200)).unwrap();
        assert_eq!(r.ee.targets["ee_grip_1"], initial + 12.0);
        r.read_pad(ControllerState::default(), now).unwrap();
        r.tick_ee(now + Duration::from_millis(300)).unwrap();
        assert_eq!(r.ee.targets["ee_grip_1"], initial + 12.0);
        r.stop(false).unwrap();
        assert!(r.ee.targets.is_empty());
        assert!(!r.pad.ee_armed);
    }
    #[test]
    fn screen_and_emergency_exclude_pad_actions() {
        let mut r = runtime();
        let now = Instant::now();
        for emergency in [false, true] {
            r.screen_control = !emergency;
            r.emergency = emergency;
            r.read_pad(ControllerState::default(), now).unwrap();
            r.read_pad(home_button(), now).unwrap();
            r.read_pad(home_button(), now + Duration::from_secs(2))
                .unwrap();
            assert!(r.homing.is_none());
            assert!(r.drive.awaiting().is_none());
        }
    }
    #[test]
    fn ai_authority_excludes_confirmation_and_ps_has_priority() {
        let mut r = runtime();
        let now = Instant::now();
        r.authority.claim("test".into(), now);
        r.read_pad(ControllerState::default(), now).unwrap();
        r.read_pad(home_button(), now).unwrap();
        r.read_pad(home_button(), now + Duration::from_secs(2))
            .unwrap();
        assert!(r.homing.is_none());
        r.authority.release();
        r.read_pad(ControllerState::default(), now).unwrap();
        let mut stop = home_button();
        stop.buttons[5] = 1;
        r.read_pad(stop.clone(), now).unwrap();
        r.read_pad(stop, now + Duration::from_secs(3)).unwrap();
        assert!(r.homing.is_none());
        assert!(r.drive.awaiting().is_none());
    }
}

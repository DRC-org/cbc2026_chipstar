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
        self.gamepad_input = Some(input.clone());
        // 停止は操作権に関係なく優先し、押下中は他の操作を受け付けない。
        if buttons[5] != 0 {
            self.guide.reset_input();
            self.manual_input = ControllerState::default();
            self.pad.home_since = None;
            self.pad.home_ready = false;
            self.pad.ee_armed = false;
            if previous[5] == 0 {
                if self.guide.enabled {
                    self.request(&Request::new("stop"), true)?;
                } else {
                    self.stop(false)?;
                }
            }
            return Ok(());
        }
        if self.read_guide(&input, now)? {
            if !self.screen_control {
                self.manual_input = ControllerState::default();
            }
            self.pad.home_since = None;
            self.pad.home_ready = false;
            return Ok(());
        }
        // R1を押している間は、通常のアーム・EE入力と重ならないボーナス操作層にする。
        if !self.screen_control
            && !self.preparation.locked()
            && buttons[10] != 0
            && self
                .cfg
                .machine
                .bonus
                .as_ref()
                .is_some_and(|profile| profile.enabled)
        {
            self.manual_input = ControllerState::default();
            if buttons[6] != 0 && previous[6] == 0 {
                self.bonus_request(&Request {
                    flag: Some(!self.bonus.semi_auto),
                    ..Request::new("bonus_mode")
                })?;
            }
            if buttons[4] != 0 && previous[4] == 0 {
                self.bonus_request(&Request::new("bonus_capture"))?;
            }
            if buttons[0] != 0 && previous[0] == 0 {
                self.bonus_request(&Request {
                    value: Some(3.0),
                    ..Request::new("bonus_receive")
                })?;
            }
            if buttons[3] != 0 && previous[3] == 0 {
                self.bonus_request(&Request {
                    value: Some(self.bonus.selected_box as f32),
                    ..Request::new("bonus_shoot")
                })?;
            }
            let boxes = self
                .cfg
                .machine
                .bonus
                .as_ref()
                .map_or(0, |profile| profile.boxes.len());
            if boxes > 0 && buttons[7] != 0 && previous[7] == 0 {
                self.bonus.selected_box = (self.bonus.selected_box + boxes - 1) % boxes;
            }
            if boxes > 0 && buttons[8] != 0 && previous[8] == 0 {
                self.bonus.selected_box = (self.bonus.selected_box + 1) % boxes;
            }
            if !self.bonus.semi_auto {
                let direction = i8::from(buttons[14] != 0) - i8::from(buttons[13] != 0);
                if direction != 0 || previous[13] != 0 || previous[14] != 0 {
                    self.bonus_request(&Request {
                        value: Some(f32::from(direction)),
                        ..Request::new("bonus_jog")
                    })?;
                }
                if buttons[11] != 0 && previous[11] == 0 {
                    self.bonus_request(&Request {
                        flag: Some(true),
                        ..Request::new("bonus_align")
                    })?;
                }
                if buttons[12] != 0 && previous[12] == 0 {
                    self.bonus_request(&Request {
                        flag: Some(false),
                        ..Request::new("bonus_align")
                    })?;
                }
                if buttons[1] != 0 && previous[1] == 0 {
                    self.bonus_request(&Request {
                        flag: Some(true),
                        ..Request::new("bonus_lid")
                    })?;
                }
                if buttons[2] != 0 && previous[2] == 0 {
                    self.bonus_request(&Request {
                        flag: Some(false),
                        ..Request::new("bonus_lid")
                    })?;
                }
            }
            return Ok(());
        }
        if !self.screen_control && !self.preparation.locked() {
            self.manual_input = input.clone();
        }
        if self.preparation.locked()
            || self.authority.active()
            || self.screen_control
            || self.emergency
            || self.sequence.is_some()
        {
            self.pad.home_since = None;
            self.pad.home_ready = false;
            self.pad.ee_armed = false;
            return Ok(());
        }
        let stopped = self.homing_idle() && self.fresh() && self.settings.ready();
        // 停止状態で両ボタンを離してから確認する。接続時の押しっぱなしでは始めない。
        if !self.guide.enabled && stopped && buttons[4] == 0 {
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
        if !self.guide.enabled && buttons[6] != 0 && previous[6] == 0 {
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
        let fold_pressed = buttons[11] != buttons[12]
            && (buttons[11] != previous[11] || buttons[12] != previous[12]);
        let grip_level = if buttons[13] != 0 && buttons[14] == 0 && previous[13] == 0 {
            Some(self.shared.sequence_config().grip_open)
        } else if buttons[14] != 0 && buttons[13] == 0 && previous[14] == 0 {
            Some(self.shared.sequence_config().grip_closed)
        } else if buttons[1] != 0 && previous[1] == 0 {
            Some(self.shared.sequence_config().grip_handoff_open)
        } else {
            None
        };
        if fold_pressed {
            let axis = axes
                .iter()
                .find(|axis| axis.name == "ee_fold")
                .context("畳みのEE割当を設定してください")?;
            let direction = axis.pad_value(&input) * axis.sign;
            targets.insert(
                axis.name.clone(),
                if direction > 0.0 { axis.max } else { axis.min },
            );
        }
        if let Some(values) = grip_level {
            // 3本一括の割当不足を、一部だけ駆動する前に検出する。
            for (index, name) in ["ee_grip_1", "ee_grip_2", "ee_grip_3"]
                .into_iter()
                .enumerate()
            {
                let _axis = axes
                    .iter()
                    .find(|axis| axis.name == name)
                    .context("把持3本のEE割当を設定してください")?;
                targets.insert(name.into(), values[index]);
            }
        }
        // 先端回転は△で、正面合わせ時に保存したフィールド基準から180°反転する。
        // 2回押すと補正差を含めて元の手合わせ角へ戻る。θ補正は専用経路が続ける。
        if buttons[3] != 0 && previous[3] == 0 {
            let _ = axes
                .iter()
                .find(|axis| axis.name == "ee_rotation")
                .context("先端回転のEE割当を設定してください")?;
            let current = 180.0 - (180.0 - self.ee.rotation_field).rem_euclid(360.0);
            let next = if current > 90.0 {
                current - 180.0
            } else {
                current + 180.0
            };
            targets.insert("ee_rotation".into(), next);
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
        self.guide.reset_input();
        self.pad = Control::default();
        self.gamepad_name.clear();
        self.gamepad_input = None;
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
    fn bonus_runtime() -> Runtime {
        let mut r = runtime();
        let embedded = MachineProfile::embedded().unwrap();
        r.cfg.machine.dc_motors = embedded.dc_motors;
        r.cfg.machine.serial_svmd = embedded.serial_svmd;
        r.cfg.machine.bonus = embedded.bonus;
        r.cfg.machine.bonus.as_mut().unwrap().enabled = true;
        r.cfg.machine.bonus.as_mut().unwrap().selector_duty = 100;
        r.cfg.machine.bonus.as_mut().unwrap().selector_slow_duty = 40;
        r.cfg.machine.dc_motors[0].input_sign = 1.0;
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
        let theta = r
            .cfg
            .machine
            .axes
            .iter()
            .position(|axis| axis.name == "theta")
            .unwrap();
        let theta_slot = usize::from(r.cfg.machine.axes[theta].slot);
        r.machine.invalidate_origin(theta);
        r.telemetry.as_mut().unwrap().slots[theta_slot].measured = 132.0;
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
        let theta_origin = r
            .machine
            .origin_states(r.telemetry.as_ref())
            .into_iter()
            .find(|axis| axis.name == "theta")
            .unwrap();
        assert!(theta_origin.captured);
        assert_eq!(theta_origin.position, 0.0);
        assert!(!r.drive.running() && r.drive.awaiting().is_none());
        r.stop(false).unwrap();
        r.read_pad(home_button(), now + Duration::from_secs(5))
            .unwrap();
        assert!(r.homing.is_none());
    }
    #[test]
    fn r1_bonus_layer_drives_only_while_direction_is_held() {
        let mut r = bonus_runtime();
        let now = Instant::now();
        let mut right = ControllerState::default();
        right.buttons[10] = 1;
        right.buttons[14] = 1;
        r.read_pad(right, now).unwrap();
        assert!(
            r.shared
                .status_snapshot()
                .logs
                .iter()
                .any(|line| line == "TX CAN 2 784 0104000000640000")
        );

        let mut released = ControllerState::default();
        released.buttons[10] = 1;
        r.read_pad(released, now + Duration::from_millis(50))
            .unwrap();
        assert!(
            r.shared
                .status_snapshot()
                .logs
                .iter()
                .any(|line| line == "TX CAN 2 784 0104000000000000")
        );
        assert!(r.manual_input.axes.iter().all(|value| *value == 0.0));
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
    #[test]
    fn options_restarts_operation_after_origins_are_ready() {
        let mut r = runtime();
        let now = Instant::now();
        r.read_pad(ControllerState::default(), now).unwrap();
        let mut options = ControllerState::default();
        options.buttons[6] = 1;
        r.read_pad(options, now).unwrap();
        assert!(r.drive.awaiting().is_some());
        r.tick().unwrap();
        assert!(r.drive.running());
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
                    acceleration_us_per_second2: 2500.0,
                    minimum_us: 500,
                    maximum_us: 1600,
                    initial_us: 1500,
                    enabled: true,
                });
        }
        r.test.peers.insert("pwm", Instant::now());
        r.drive = DriveState::Running;
    }
    #[test]
    fn circle_during_guided_driving_opens_grips_without_restarting_preparation() {
        for phase in [PreparationPhase::Setting, PreparationPhase::Active] {
            for deflected in [false, true] {
                let mut r = runtime();
                add_grips(&mut r);
                r.guide.enabled = true;
                r.preparation = phase;
                let now = Instant::now();
                for _ in 0..2 {
                    r.read_pad(ControllerState::default(), now).unwrap();
                }
                let mut circle = ControllerState::default();
                circle.buttons[1] = 1;
                if deflected {
                    circle.axes[0] = 0.5;
                }
                r.read_pad(circle, now).unwrap();
                r.read_pad(ControllerState::default(), now).unwrap();
                assert!(r.drive.running());
                assert_eq!(r.preparation, phase);
                assert_eq!(r.court, Some(Court::Red));
                assert!(
                    r.machine
                        .origin_states(r.telemetry.as_ref())
                        .iter()
                        .all(|o| o.captured)
                );
                for (index, value) in r
                    .shared
                    .sequence_config()
                    .grip_handoff_open
                    .iter()
                    .enumerate()
                {
                    assert_eq!(
                        r.ee.goals.get(&format!("ee_grip_{}", index + 1)),
                        Some(value)
                    );
                }
            }
        }
    }
    #[test]
    fn guided_r1_circle_only_opens_bonus_lid_even_when_r1_is_released_first() {
        let mut r = bonus_runtime();
        add_grips(&mut r);
        r.guide.enabled = true;
        r.cfg.machine.bonus.as_mut().unwrap().lid_open_position = 2600;
        let servo = r
            .cfg
            .machine
            .serial_svmd
            .as_mut()
            .unwrap()
            .servos
            .iter_mut()
            .find(|servo| servo.name == "bonus_lid")
            .unwrap();
        servo.enabled = true;
        let expected = format!(
            "TX {}",
            crate::protocol::serial_svmd::Command::Target {
                id: servo.id,
                position: 2600,
                speed: servo.speed_position_per_second.round() as u16,
                acceleration: servo.acceleration,
            }
            .to_cctl_line()
        );
        let now = Instant::now();
        for _ in 0..2 {
            r.read_pad(ControllerState::default(), now).unwrap();
        }
        let mut input = ControllerState::default();
        input.buttons[10] = 1;
        r.read_pad(input.clone(), now).unwrap();
        input.buttons[1] = 1;
        r.read_pad(input.clone(), now).unwrap();
        input.buttons[10] = 0;
        r.read_pad(input, now).unwrap();
        r.read_pad(ControllerState::default(), now).unwrap();
        assert!(r.shared.status_snapshot().logs.contains(&expected));
        assert!(r.ee.goals.is_empty());
        assert!(r.drive.running());
        assert_eq!(r.court, Some(Court::Red));
        assert!(
            r.machine
                .origin_states(r.telemetry.as_ref())
                .iter()
                .all(|o| o.captured)
        );
    }
    #[test]
    fn triangle_reverses_tip_rotation_and_returns_to_captured_field() {
        let mut r = runtime();
        r.cfg.machine.serial_svmd = MachineProfile::embedded().unwrap().serial_svmd;
        r.drive = DriveState::Running;
        r.test.peers.insert("sts", Instant::now());
        let now = Instant::now();
        r.ee.rotation_field = -45.0;
        r.read_pad(ControllerState::default(), now).unwrap();
        let mut triangle = ControllerState::default();
        triangle.buttons[3] = 1;
        r.read_pad(triangle.clone(), now).unwrap();
        assert_eq!(r.ee.rotation_field, 135.0);
        assert!(r.ee.targets.contains_key("ee_rotation"));
        r.read_pad(ControllerState::default(), now).unwrap();
        r.read_pad(triangle, now).unwrap();
        assert_eq!(r.ee.rotation_field, -45.0);

        r.read_pad(ControllerState::default(), now).unwrap();
        r.ee.rotation_field = 0.25;
        let mut triangle = ControllerState::default();
        triangle.buttons[3] = 1;
        r.read_pad(triangle.clone(), now).unwrap();
        assert_eq!(r.ee.rotation_field, 180.25);
        r.read_pad(ControllerState::default(), now).unwrap();
        r.read_pad(triangle, now).unwrap();
        assert_eq!(r.ee.rotation_field, 0.25);
    }
    #[test]
    fn grips_require_neutral_then_select_pick_close_and_handoff_levels() {
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
        assert_eq!(r.ee.goals.len(), 3);
        assert!(r.ee.goals.values().all(|value| *value == 700.0));

        r.read_pad(ControllerState::default(), now).unwrap();
        let mut pickup_open = ControllerState::default();
        pickup_open.buttons[13] = 1;
        r.read_pad(pickup_open, now).unwrap();
        assert!(r.ee.goals.values().all(|value| *value == 1500.0));

        r.read_pad(ControllerState::default(), now).unwrap();
        let mut handoff_open = ControllerState::default();
        handoff_open.buttons[1] = 1;
        r.read_pad(handoff_open, now).unwrap();
        assert!(r.ee.goals.values().all(|value| *value == 500.0));
        r.stop(false).unwrap();
        assert!(r.ee.targets.is_empty());
        assert!(!r.pad.ee_armed);
    }
    #[test]
    fn fold_button_selects_one_endpoint_per_press() {
        let mut r = runtime();
        r.cfg
            .machine
            .pwm_servos
            .push(crate::machine::PwmServoProfile {
                name: "ee_fold".into(),
                channel: 0,
                input_axis: None,
                input_sign: 1.0,
                speed_us_per_second: 100.0,
                acceleration_us_per_second2: 2500.0,
                minimum_us: 1400,
                maximum_us: 1600,
                initial_us: 1500,
                enabled: true,
            });
        r.test.peers.insert("pwm", Instant::now());
        r.drive = DriveState::Running;
        let now = Instant::now();
        r.read_pad(ControllerState::default(), now).unwrap();
        let mut up = ControllerState::default();
        up.buttons[11] = 1;
        r.read_pad(up.clone(), now).unwrap();
        assert_eq!(r.ee.goals["ee_fold"], 1600.0);
        r.read_pad(up, now).unwrap();
        assert_eq!(r.ee.goals["ee_fold"], 1600.0);
        r.read_pad(ControllerState::default(), now).unwrap();
        let mut down = ControllerState::default();
        down.buttons[12] = 1;
        r.read_pad(down, now).unwrap();
        assert_eq!(r.ee.goals["ee_fold"], 1400.0);
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

use super::*;

fn running() -> (Runtime, Instant) {
    let mut r = super::tests::rotation_runtime(1500);
    r.screen_control = false;
    r.guide.enabled = false;
    r.drive = DriveState::Running;
    let now = Instant::now();
    r.tick_ee(now).unwrap();
    (r, now)
}

fn input(x: f32) -> ControllerState {
    let mut input = ControllerState::default();
    input.axes[2] = x;
    input
}

#[test]
fn trim_requires_neutral_then_holds_the_new_field_while_theta_moves() {
    let (mut r, now) = running();
    let initial = r.ee.rotation_field;
    r.read_pad(input(1.0), now).unwrap();
    r.tick_ee(now + Duration::from_millis(50)).unwrap();
    assert_eq!(r.ee.rotation_field, initial);
    r.read_pad(input(0.0), now).unwrap();
    r.read_pad(input(1.0), now).unwrap();
    r.tick_ee(now + Duration::from_millis(100)).unwrap();
    let trimmed = initial + 0.75;
    assert!((r.ee.rotation_field - trimmed).abs() < 0.001);
    r.read_pad(input(0.0), now).unwrap();
    let theta = r
        .cfg
        .machine
        .axes
        .iter()
        .find(|a| a.name == "theta")
        .unwrap()
        .clone();
    r.telemetry.as_mut().unwrap().slots[theta.slot as usize].measured +=
        10.0 * theta.native_per_unit;
    r.tick_ee(now + Duration::from_millis(150)).unwrap();
    assert_eq!(r.ee.rotation_field, trimmed);
    let axis = ee::axes(&r.cfg.machine)
        .into_iter()
        .find(|a| a.name == "ee_rotation")
        .unwrap();
    assert_eq!(
        r.ee.targets["ee_rotation"],
        f32::from(axis.rotation_count(trimmed, 10.0).unwrap())
    );
    let target = axis
        .rotation_command(axis.rotation_count(trimmed, 10.0).unwrap(), false)
        .remove(0);
    assert!(
        r.shared
            .status_snapshot()
            .logs
            .contains(&format!("TX {target}"))
    );
    let mut triangle = input(0.0);
    triangle.buttons[3] = 1;
    r.read_pad(triangle.clone(), now).unwrap();
    r.read_pad(input(0.0), now).unwrap();
    r.read_pad(triangle, now).unwrap();
    assert!((r.ee.rotation_field - trimmed).abs() < 0.001);
    r.publish();
    assert_eq!(
        r.shared.status_snapshot().ee_rotation_field,
        Some(r.ee.rotation_field)
    );
    r.stop(false).unwrap();
    r.publish();
    assert_eq!(r.shared.status_snapshot().ee_rotation_field, None);
}

#[test]
fn trim_uses_proportional_input_l1_and_sign_without_drifting_in_deadzone() {
    for (stick, sign, slow, expected) in [
        (1.0, 1.0, false, 0.75),
        (-1.0, 1.0, false, -0.75),
        (1.0, -1.0, false, -0.75),
        (0.55, 1.0, false, 0.375),
        (1.0, 1.0, true, 0.15),
        (0.09, 1.0, false, 0.0),
        (f32::NAN, 1.0, false, 0.0),
    ] {
        let (mut r, now) = running();
        r.cfg.machine.slow_speed_percent = 20.0;
        r.cfg
            .machine
            .serial_svmd
            .as_mut()
            .unwrap()
            .servos
            .iter_mut()
            .find(|s| s.name == "ee_rotation")
            .unwrap()
            .input_sign = sign;
        let initial = r.ee.rotation_field;
        r.read_pad(input(0.0), now).unwrap();
        let mut pad = input(stick);
        pad.buttons[9] = u8::from(slow);
        r.read_pad(pad, now).unwrap();
        r.tick_ee(now + Duration::from_millis(50)).unwrap();
        assert!((r.ee.rotation_field - initial - expected).abs() < 0.001);
    }
}

#[test]
fn stops_and_other_input_modes_do_not_accept_or_replay_trim() {
    for mode in [
        "stop",
        "screen",
        "test",
        "sts",
        "waiting",
        "disconnect",
        "ai",
        "home",
    ] {
        let (mut r, now) = running();
        r.read_pad(input(0.0), now).unwrap();
        r.read_pad(input(1.0), now).unwrap();
        let initial = r.ee.rotation_field;
        match mode {
            "stop" => {
                r.stop(false).unwrap();
            }
            "screen" => r.screen_control = true,
            "test" => r.test.enabled = true,
            "sts" => r.sts.active = true,
            "waiting" => r.preparation = PreparationPhase::Waiting,
            "disconnect" => r.disconnect_pad(),
            "ai" => r.authority.claim("test".into(), now),
            "home" => {
                r.front_return_pending = Some(180.0);
                r.begin_front_return(180.0);
            }
            _ => unreachable!(),
        }
        let was_cleared = r.ee.targets.is_empty();
        let before = r.shared.status_snapshot().logs.clone();
        r.tick_ee(now + Duration::from_millis(50)).unwrap();
        if was_cleared {
            assert_eq!(r.shared.status_snapshot().logs, before);
        } else {
            assert_eq!(r.ee.rotation_field, initial, "{mode}");
        }
    }
}

#[test]
fn relative_mouse_commands_accumulate_and_validate_before_output() {
    let (mut r, now) = running();
    let initial = r.ee.rotation_field;
    for delta in [1.0, 1.0, -1.0] {
        r.request(
            &Request {
                value: Some(delta),
                ..Request::new("ee_trim")
            },
            true,
        )
        .unwrap();
    }
    assert!((r.ee.rotation_field - initial - 1.0).abs() < 0.001);
    for delta in [f32::NAN, f32::INFINITY, 100000.0] {
        let field = r.ee.rotation_field;
        let before = r.shared.status_snapshot().logs.clone();
        assert!(
            r.request(
                &Request {
                    value: Some(delta),
                    ..Request::new("ee_trim")
                },
                true
            )
            .is_err()
        );
        assert_eq!(r.ee.rotation_field, field);
        assert_eq!(r.shared.status_snapshot().logs, before);
    }
    r.stop(false).unwrap();
    assert!(
        r.request(
            &Request {
                value: Some(1.0),
                ..Request::new("ee_trim")
            },
            true
        )
        .is_err()
    );
    r.tick_ee(now + Duration::from_millis(50)).unwrap();
    assert!(r.ee.targets.is_empty());
}

#[test]
fn trim_respects_configured_servo_speed_and_zero_means_finite_manual_speed() {
    for (speed, expected) in [(0.0, 0.75), (1000.0, 0.75), (10.0, 0.05)] {
        let (mut r, now) = running();
        let servo = r
            .cfg
            .machine
            .serial_svmd
            .as_mut()
            .unwrap()
            .servos
            .iter_mut()
            .find(|s| s.name == "ee_rotation")
            .unwrap();
        servo.counts_per_deg = 10.0;
        servo.speed_position_per_second = speed;
        let initial = r.ee.rotation_field;
        r.read_pad(input(0.0), now).unwrap();
        r.read_pad(input(1.0), now).unwrap();
        r.tick_ee(now + Duration::from_millis(50)).unwrap();
        assert!((r.ee.rotation_field - initial - expected).abs() < 0.001);
    }
}

#[test]
fn bonus_layer_and_stop_require_releasing_the_stick_before_trimming_again() {
    for stop in [false, true] {
        let (mut r, now) = running();
        r.cfg.machine.bonus = crate::machine::test_support::with_bonus().bonus;
        r.cfg.machine.bonus.as_mut().unwrap().enabled = true;
        r.read_pad(input(0.0), now).unwrap();
        if stop {
            let mut pad = input(1.0);
            pad.buttons[5] = 1;
            r.read_pad(pad, now).unwrap();
            r.drive = DriveState::Running;
            r.tick_ee(now).unwrap();
        } else {
            let mut pad = input(1.0);
            pad.buttons[10] = 1;
            r.read_pad(pad, now).unwrap();
        }
        let initial = r.ee.rotation_field;
        r.read_pad(input(1.0), now).unwrap();
        r.tick_ee(now + Duration::from_millis(50)).unwrap();
        assert_eq!(r.ee.rotation_field, initial);
        r.read_pad(input(0.0), now).unwrap();
        r.read_pad(input(1.0), now).unwrap();
        r.tick_ee(now + Duration::from_millis(100)).unwrap();
        assert!(r.ee.rotation_field > initial);
    }
}

#[test]
fn triangle_keeps_unwrapped_angles_and_small_corrections_across_half_turns() {
    for initial in [-500.0, -100.0, -90.5, 90.5, 270.5, 400.0] {
        let (mut r, now) = running();
        r.rotation_request(initial).unwrap();
        r.read_pad(input(0.0), now).unwrap();
        let mut triangle = input(0.0);
        triangle.buttons[3] = 1;
        r.read_pad(triangle.clone(), now).unwrap();
        assert_eq!((r.ee.rotation_field - initial).abs(), 180.0);
        r.request(
            &Request {
                value: Some(1.0),
                ..Request::new("ee_trim")
            },
            true,
        )
        .unwrap();
        r.read_pad(input(0.0), now).unwrap();
        r.read_pad(triangle, now).unwrap();
        assert_eq!(r.ee.rotation_field, initial + 1.0);
    }
}

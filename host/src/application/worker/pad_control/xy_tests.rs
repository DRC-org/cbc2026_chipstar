use super::*;
use crate::machine::xy::PlanarMode;

fn mode_request(mode: PlanarMode) -> Request {
    Request {
        text: Some(mode.key().into()),
        ..Request::new("planar_mode")
    }
}
fn l3() -> ControllerState {
    let mut input = ControllerState::default();
    input.buttons[7] = 1;
    input
}

#[test]
fn xy_mode_requires_fresh_l3_press_and_neutral_axes_for_pad_and_mouse() {
    let mut r = tests::runtime();
    let now = Instant::now();
    r.read_pad(l3(), now).unwrap(); // 接続したときから押していた入力は採用しない。
    assert_eq!(r.planar_mode, PlanarMode::Rtheta);
    r.read_pad(ControllerState::default(), now).unwrap();
    r.read_pad(l3(), now).unwrap();
    assert_eq!(r.planar_mode, PlanarMode::Xy);
    r.read_pad(l3(), now).unwrap();
    assert_eq!(r.planar_mode, PlanarMode::Xy);
    assert!(!r.drive.running());
    r.read_pad(ControllerState::default(), now).unwrap();
    let mut held = l3();
    held.axes[5] = 0.5;
    assert!(r.read_pad(held, now).is_err());
    assert!(r.request(&mode_request(PlanarMode::Rtheta), true).is_err());
    assert_eq!(r.planar_mode, PlanarMode::Xy);
    r.read_pad(ControllerState::default(), now).unwrap();
    r.request(&mode_request(PlanarMode::Rtheta), true).unwrap();
    r.publish();
    assert_eq!(r.shared.status_snapshot().planar_mode, PlanarMode::Rtheta);
    assert!(!r.drive.running());
    r.disconnect_pad();
    r.read_pad(l3(), now).unwrap();
    assert_eq!(r.planar_mode, PlanarMode::Rtheta);
}

#[test]
fn xy_mode_keeps_bonus_l3_and_ps_priority_and_preparation_feedback() {
    let mut r = tests::bonus_runtime();
    let now = Instant::now();
    r.read_pad(ControllerState::default(), now).unwrap();
    let mut bonus = l3();
    bonus.buttons[10] = 1;
    r.read_pad(bonus, now).unwrap();
    assert_eq!(r.planar_mode, PlanarMode::Rtheta);
    assert_eq!(
        r.bonus.selected_box,
        r.cfg.machine.bonus.as_ref().unwrap().boxes.len() - 1
    );
    r.read_pad(ControllerState::default(), now).unwrap();
    let mut stop = l3();
    stop.buttons[5] = 1;
    r.read_pad(stop, now).unwrap();
    assert_eq!(r.planar_mode, PlanarMode::Rtheta);
    r.guide.enabled = true;
    r.read_pad(ControllerState::default(), now).unwrap();
    r.read_pad(l3(), now).unwrap();
    assert_eq!(r.planar_mode, PlanarMode::Xy);
    assert!(
        r.shared
            .status_snapshot()
            .logs
            .iter()
            .any(|line| line == "TX TONE 1")
    );
}

#[test]
fn xy_mode_cannot_change_during_waiting_tests_or_ai_control() {
    let mut r = tests::runtime();
    r.preparation = PreparationPhase::Waiting;
    assert!(r.request(&mode_request(PlanarMode::Xy), true).is_err());
    r.preparation = PreparationPhase::Setting;
    r.test.enabled = true;
    assert!(r.request(&mode_request(PlanarMode::Xy), true).is_err());
    r.test.enabled = false;
    r.authority.claim("xy-test".into(), Instant::now());
    assert!(r.request(&mode_request(PlanarMode::Xy), true).is_err());
    assert_eq!(r.planar_mode, PlanarMode::Rtheta);
}

#[test]
fn xy_mode_is_used_by_the_running_worker_and_screen_jog_keeps_axis_meaning() {
    let mut r = tests::runtime();
    r.cfg.machine.xy.radius_offset_mm = 100.0;
    r.cfg.machine.xy.speed_mm_per_second = 100.0;
    for axis in &mut r.cfg.machine.axes {
        axis.jog_ramp_seconds = 0.0;
    }
    r.machine.reconfigure(r.cfg.machine.clone());
    for (i, position) in [300.0, 45.0, 100.0].into_iter().enumerate() {
        assert!(
            r.machine
                .capture_coordinate(i, r.telemetry.as_ref(), position)
        );
    }
    r.request(&mode_request(PlanarMode::Xy), true).unwrap();
    r.start_with_front_return(false).unwrap();
    r.tick().unwrap();
    assert!(r.drive.running());
    let mut input = ControllerState::default();
    input.axes[0] = 0.5;
    input.axes[5] = 0.4;
    r.read_pad(input, Instant::now()).unwrap();
    r.tick().unwrap();
    assert!(r.drive.running(), "{}", r.error);
    let logs = r.shared.status_snapshot().logs;
    let mut speeds = [0.0; 3];
    for axis in &r.cfg.machine.axes {
        let prefix = format!("TX JOG {} ", axis.slot);
        let value = logs
            .iter()
            .rev()
            .find_map(|l| l.strip_prefix(&prefix))
            .unwrap();
        speeds[axis.slot as usize] = value.parse::<f32>().unwrap() / axis.native_per_unit;
    }
    let angle = r
        .machine
        .axis_position("theta", r.telemetry.as_ref())
        .unwrap()
        .to_radians();
    let radius = r.machine.axis_position("r", r.telemetry.as_ref()).unwrap() + 100.0;
    let vx = -speeds[0] * angle.sin() - radius * angle.cos() * speeds[1].to_radians();
    let vy = speeds[0] * angle.cos() - radius * angle.sin() * speeds[1].to_radians();
    assert!((vx - 50.0).abs() < 0.01, "vx={vx}");
    assert!(vy.abs() < 0.01, "vy={vy}");
    assert!((speeds[2] - 80.0).abs() < 0.01);
    assert!(r.request(&mode_request(PlanarMode::Rtheta), true).is_err());
    r.read_pad(ControllerState::default(), Instant::now())
        .unwrap();
    r.tick().unwrap();
    r.request(&mode_request(PlanarMode::Rtheta), true).unwrap();
    assert!(r.drive.running());

    r.request(&mode_request(PlanarMode::Xy), true).unwrap();
    r.stop(false).unwrap();
    r.screen_control = true;
    r.start_with_front_return(false).unwrap();
    r.tick().unwrap();
    r.request(
        &Request {
            axis: Some("r".into()),
            value: Some(-0.2),
            ..Request::new("input")
        },
        true,
    )
    .unwrap();
    r.tick().unwrap();
    let logs = r.shared.status_snapshot().logs;
    let last_theta = logs
        .iter()
        .rev()
        .find(|l| l.starts_with("TX TARGET 1 ") || l.starts_with("TX JOG 1 "))
        .unwrap();
    assert!(last_theta.starts_with("TX TARGET 1 "), "{last_theta}");
    assert_eq!(r.planar_mode, PlanarMode::Xy);
}

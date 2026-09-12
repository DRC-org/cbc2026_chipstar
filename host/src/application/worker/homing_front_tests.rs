use super::*;
use crate::application::sequence::{Action, Side, Stage, Start, Step};

fn fixture(degrees: f32) -> Runtime {
    let mut runtime = crate::application::worker::tests::screen_runtime();
    let theta = runtime
        .cfg
        .machine
        .axes
        .iter_mut()
        .find(|axis| axis.name == "theta")
        .unwrap();
    theta.speed_per_second = 40.0;
    theta.jog_ramp_seconds = 0.5;
    runtime.machine.reconfigure(runtime.cfg.machine.clone());
    set_theta(&mut runtime, degrees);
    runtime.telemetry.as_mut().unwrap().mode = RunMode::Run;
    runtime.telemetry.as_mut().unwrap().enabled_slots = 6;
    runtime.front_return_pending = Some(180.0);
    runtime
}

fn set_theta(runtime: &mut Runtime, degrees: f32) {
    let theta = runtime
        .cfg
        .machine
        .axes
        .iter()
        .find(|axis| axis.name == "theta")
        .unwrap();
    let slot = theta.slot;
    let native = runtime.machine.native_position(slot, degrees).unwrap();
    let telemetry = runtime.telemetry.as_mut().unwrap();
    telemetry.slots[slot as usize].measured = native;
    telemetry.slots[slot as usize].target = native;
    // 同じ位置を模擬基板にも与え、次の受信で実測が巻き戻らないようにする。
    runtime.send(&format!("TARGET {slot} {native:.5}")).unwrap();
}

fn target(runtime: &Runtime, name: &str) -> f32 {
    runtime
        .machine
        .origin_states(runtime.telemetry.as_ref())
        .iter()
        .find(|axis| axis.name == name)
        .unwrap()
        .target
}

fn start_return(runtime: &mut Runtime) -> Instant {
    runtime.start().unwrap();
    assert!(runtime.front_return_pending.is_none());
    assert!(matches!(
        runtime.homing.as_ref().unwrap().phase,
        Phase::AwaitFrontRun
    ));
    let now = runtime.homing.as_ref().unwrap().since + Duration::from_millis(1);
    runtime.tick_at(now).unwrap();
    assert!(runtime.drive.running());
    assert!(matches!(
        runtime.homing.as_ref().unwrap().phase,
        Phase::ReturnFront
    ));
    now
}

#[test]
fn front_return_waits_for_run_acknowledgement_before_moving() {
    let mut runtime = fixture(90.0);
    runtime.start().unwrap();
    let now = runtime.homing.as_ref().unwrap().since;
    let count = runtime.shared.status_snapshot().tx_count;
    runtime
        .tick_homing(now + Duration::from_millis(100))
        .unwrap();
    runtime
        .tick_homing(now + Duration::from_millis(400))
        .unwrap();
    assert_eq!(runtime.shared.status_snapshot().tx_count, count);
    assert!((target(&runtime, "theta") - 90.0).abs() < 0.001);
    assert!(runtime.homing.as_ref().unwrap().motion.is_none());

    runtime.tick_at(now + Duration::from_millis(450)).unwrap();
    assert!(matches!(
        runtime.homing.as_ref().unwrap().phase,
        Phase::ReturnFront
    ));
    assert!((target(&runtime, "theta") - 90.0).abs() < 0.001);
}

#[test]
fn first_start_returns_from_either_side_with_acceleration_then_later_starts_hold() {
    for degrees in [-90.0_f32, 90.0] {
        let mut runtime = fixture(degrees);
        let now = start_return(&mut runtime);
        let radial = target(&runtime, "r");
        let height = target(&runtime, "z");
        assert!((runtime.homing.as_ref().unwrap().motion.unwrap().duration() - 2.75).abs() < 0.001);
        let mut previous_target = degrees;
        let mut previous_velocity = 0.0;
        let mut peak_speed = 0.0_f32;
        for step in 1..=60 {
            runtime
                .tick_at(now + Duration::from_millis(step * 50))
                .unwrap();
            let current = target(&runtime, "theta");
            let velocity = (current - previous_target) / 0.05;
            assert!(current.abs() <= previous_target.abs() + 0.001);
            assert!(current * degrees.signum() >= -0.001);
            assert!(velocity.abs() <= 40.01);
            assert!((velocity - previous_velocity).abs() / 0.05 <= 80.1);
            assert!((target(&runtime, "r") - radial).abs() < 0.001);
            assert!((target(&runtime, "z") - height).abs() < 0.001);
            peak_speed = peak_speed.max(velocity.abs());
            previous_target = current;
            previous_velocity = velocity;
        }
        assert!(peak_speed > 39.9);
        assert!(previous_target.abs() < 0.001);
        assert!(runtime.homing.is_none());
        assert!(runtime.front_return_pending.is_none());
        assert!(runtime.drive.running());

        set_theta(&mut runtime, degrees * 0.5);
        runtime.stop(false).unwrap();
        runtime.start().unwrap();
        assert!(runtime.homing.is_none());
        assert!((target(&runtime, "theta") - degrees * 0.5).abs() < 0.001);
        runtime.tick_at(now + Duration::from_secs(4)).unwrap();
        assert!((target(&runtime, "theta") - degrees * 0.5).abs() < 0.001);
    }
}

#[test]
fn front_return_requires_both_finished_trajectory_and_measured_arrival() {
    let mut runtime = fixture(90.0);
    let now = start_return(&mut runtime);
    set_theta(&mut runtime, 0.0);
    runtime
        .tick_homing(now + Duration::from_millis(100))
        .unwrap();
    assert!(runtime.homing.is_some());

    set_theta(&mut runtime, 10.0);
    runtime.tick_homing(now + Duration::from_secs(3)).unwrap();
    assert!(runtime.homing.is_some());
    assert!(target(&runtime, "theta").abs() < 0.001);

    set_theta(&mut runtime, 0.5);
    runtime
        .tick_homing(now + Duration::from_millis(3050))
        .unwrap();
    assert!(runtime.homing.is_none());
    assert!(runtime.drive.running());
}

#[test]
fn ps_interrupts_front_return_with_position_hold_and_resume_does_not_retry() {
    let mut runtime = fixture(-90.0);
    let now = start_return(&mut runtime);
    runtime.tick_at(now + Duration::from_millis(250)).unwrap();
    let before = runtime.shared.status_snapshot().logs.len();
    let measured = runtime
        .machine
        .axis_position("theta", runtime.telemetry.as_ref())
        .unwrap();
    let mut ps = ControllerState::default();
    ps.buttons[5] = 1;
    runtime
        .read_pad(ps, now + Duration::from_millis(300))
        .unwrap();
    assert!(runtime.homing.is_none());
    assert!(runtime.front_return_pending.is_none());
    assert!(!runtime.drive.running());
    assert!((target(&runtime, "theta") - measured).abs() < 0.001);
    let logs = runtime.shared.status_snapshot().logs;
    assert!(
        logs.iter()
            .skip(before)
            .any(|line| line.starts_with("TX TARGET 2 "))
    );
    assert!(
        !logs
            .iter()
            .skip(before)
            .any(|line| line == "TX STOP" || line == "TX SAFE")
    );
    runtime
        .read_pad(ControllerState::default(), now + Duration::from_millis(350))
        .unwrap();
    runtime.start().unwrap();
    assert!(runtime.homing.is_none());
    assert!((target(&runtime, "theta") - measured).abs() < 0.001);
}

#[test]
fn ordinary_stop_preserves_an_unstarted_return_but_cut_and_z_hold_discard_it() {
    let mut runtime = fixture(90.0);
    runtime.stop(false).unwrap();
    assert_eq!(runtime.front_return_pending, Some(180.0));
    runtime.start().unwrap();
    assert!(matches!(
        runtime.homing.as_ref().unwrap().phase,
        Phase::AwaitFrontRun
    ));

    for z_hold in [false, true] {
        let mut runtime = fixture(90.0);
        if z_hold {
            runtime.stop_with_z_hold().unwrap();
        } else {
            runtime.stop(true).unwrap();
        }
        assert!(runtime.front_return_pending.is_none());
        assert!(runtime.homing.is_none());
    }
}

fn sequence_start_request() -> Request {
    Request {
        text: Some(
            toml::to_string(&Start {
                group: 0,
                side: Side::Left,
                stage: Stage::Prepare,
            })
            .unwrap(),
        ),
        ..Request::new("sequence_start")
    }
}

#[test]
fn explicit_sequence_start_uses_its_own_motion_and_consumes_pending_return() {
    let mut runtime = fixture(90.0);
    let mut config = runtime.shared.sequence_config();
    config.prepare = vec![Step::new(Action::WorkPosition)];
    config.groups[0].theta = 25.0;
    runtime.shared.set_sequence_config(config);
    runtime.request(&sequence_start_request(), true).unwrap();
    assert!(runtime.sequence.is_some());
    assert!(runtime.homing.is_none());
    assert!(runtime.front_return_pending.is_none());
    runtime.stop(false).unwrap();
    runtime.start().unwrap();
    assert!(runtime.homing.is_none());
}

#[test]
fn front_return_excludes_manual_jog_and_competing_sequence_requests() {
    let mut runtime = fixture(90.0);
    let now = start_return(&mut runtime);
    runtime.manual_input.axes[0] = 1.0;
    runtime.manual_input.axes[1] = 1.0;
    let before = runtime.shared.status_snapshot().logs.len();
    runtime.tick_at(now + Duration::from_millis(50)).unwrap();
    assert!(
        !runtime
            .shared
            .status_snapshot()
            .logs
            .iter()
            .skip(before)
            .any(|line| line.starts_with("TX JOG "))
    );
    assert!(runtime.request(&sequence_start_request(), true).is_err());
    assert!(runtime.sequence.is_none());
    assert!(runtime.homing.is_some());
}

#[test]
fn already_front_start_finishes_without_a_position_jump() {
    let mut runtime = fixture(0.0);
    let now = start_return(&mut runtime);
    assert_eq!(
        runtime.homing.as_ref().unwrap().motion.unwrap().duration(),
        0.0
    );
    runtime.tick_at(now + Duration::from_millis(50)).unwrap();
    assert!(runtime.homing.is_none());
    assert!(runtime.drive.running());
    assert!(target(&runtime, "theta").abs() < 0.001);
}

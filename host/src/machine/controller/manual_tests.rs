use super::*;
use crate::protocol::telemetry::{RunMode, SlotState};
fn telemetry(native: f32) -> Telemetry {
    Telemetry {
        uptime_ms: 0,
        slots: [SlotState {
            measured: native,
            target: native,
        }; 3],
        enabled_slots: 7,
        mode: RunMode::Run,
        error_bits: [0; 3],
        contacts: Some(0),
        stale_slots: 0,
        buses: 3,
    }
}
#[test]
fn frozen_feedback_does_not_accumulate_a_manual_position_target() {
    let profile = MachineProfile::embedded().unwrap();
    let axis = profile.axes[0].clone();
    let slow_percent = profile.slow_speed_percent;
    let mut machine = MachineController::new(profile);
    let t = telemetry(5.0);
    machine.observe(&t);
    assert!(machine.capture_origin(0, Some(&t)));
    let mut input = ControllerState::default();
    let input_axis = axis.input_axis.unwrap();
    // r原点は可動域上端なので、そこから内側へ進む入力を使う。
    let stick = -0.5 * axis.input_sign;
    input.axes[input_axis] = stick;
    for _ in 0..1000 {
        machine.observe(&t);
        let line = &machine.jog_lines(&input, &t, false)[0];
        let velocity: f32 = line.split_whitespace().last().unwrap().parse().unwrap();
        let expected = stick * axis.input_sign * axis.speed_per_second * axis.native_per_unit;
        assert!((velocity - expected).abs() < 0.001);
    }
    assert!((machine.origin_states(Some(&t))[0].position - axis.origin_position).abs() < 0.001);
    let line = &machine.jog_lines(&input, &t, true)[0];
    let velocity: f32 = line.split_whitespace().last().unwrap().parse().unwrap();
    let expected = stick
        * axis.input_sign
        * axis.speed_per_second
        * slow_percent
        * 0.01
        * axis.native_per_unit;
    assert!((velocity - expected).abs() < 0.001);
    input.axes[input_axis] = 0.0;
    let lines = machine.jog_lines(&input, &t, false);
    let velocity: f32 = lines[0].split_whitespace().last().unwrap().parse().unwrap();
    assert_eq!(velocity, 0.0);
}
#[test]
fn displayed_position_uses_the_captured_offset() {
    let profile = MachineProfile::embedded().unwrap();
    let native_per_unit = profile.axes[0].native_per_unit;
    let origin_position = profile.axes[0].origin_position;
    let mut machine = MachineController::new(profile);
    let start = telemetry(5.0);
    machine.observe(&start);
    machine.capture_origin(0, Some(&start));
    let moved = telemetry(5.0 + native_per_unit);
    machine.observe(&moved);
    assert!(
        (machine.origin_states(Some(&moved))[0].position - (origin_position + 1.0)).abs() < 0.001
    );
    let mut input = ControllerState::default();
    input.axes[1] = 1.0;
    let restarted = telemetry(0.0);
    machine.observe(&restarted);
    assert!(machine.origin_states(Some(&restarted))[0].lost);
    let lines = machine.jog_lines(&input, &restarted, false);
    let velocity: f32 = lines[0].split_whitespace().last().unwrap().parse().unwrap();
    assert_eq!(velocity, 0.0);
}
#[test]
fn holding_outside_a_soft_limit_does_not_command_a_return() {
    let mut machine = MachineController::new(MachineProfile::embedded().unwrap());
    let start = telemetry(0.0);
    machine.capture_origin(0, Some(&start));
    let moved = telemetry(8.0);
    machine.hold_at_measured(Some(&moved));
    assert_eq!(
        machine.targets[0],
        machine.origin_states(Some(&moved))[0].position
    );
}

#[test]
fn z_ten_mm_feedback_and_five_mm_per_second_command_use_rotor_degrees() {
    let profile = MachineProfile::embedded().unwrap();
    let axis = profile.axes[2].clone();
    let mut machine = MachineController::new(profile);
    let start = telemetry(1234.0);
    assert!(machine.capture_origin(2, Some(&start)));
    let moved = telemetry(1234.0 + axis.native_per_unit * 10.0);
    assert!((machine.origin_states(Some(&moved))[2].position - 10.0).abs() < 0.001);
    let mut input = ControllerState::default();
    input.axes[3] = 1.0;
    let line = &machine.jog_lines(&input, &start, false)[2];
    let velocity: f32 = line.split_whitespace().last().unwrap().parse().unwrap();
    let expected = axis.input_sign * axis.speed_per_second * axis.native_per_unit;
    assert!((velocity - expected).abs() < 0.001);
}

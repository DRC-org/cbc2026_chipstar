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
        contacts: Some(7),
        stale_slots: 0,
        buses: 3,
    }
}
#[test]
fn frozen_feedback_does_not_accumulate_a_manual_position_target() {
    let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
    let t = telemetry(5.0);
    machine.observe(&t);
    assert!(machine.capture_origin(0, Some(&t)));
    let mut input = ControllerState::default();
    input.axes[1] = 0.5;
    for _ in 0..1000 {
        machine.observe(&t);
        assert_eq!(machine.jog_lines(&input, &t, false)[0], "JOG 0 0.20000");
    }
    assert_eq!(machine.origin_states(Some(&t))[0].position, 0.0);
    assert_eq!(machine.jog_lines(&input, &t, true)[0], "JOG 0 0.04000");
    input.axes[1] = 0.0;
    assert_eq!(machine.jog_lines(&input, &t, false)[0], "JOG 0 0.00000");
}
#[test]
fn displayed_position_uses_the_captured_offset() {
    let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
    let start = telemetry(5.0);
    machine.observe(&start);
    machine.capture_origin(0, Some(&start));
    let moved = telemetry(5.04);
    machine.observe(&moved);
    assert!((machine.origin_states(Some(&moved))[0].position - 1.0).abs() < 0.001);
    let mut input = ControllerState::default();
    input.axes[1] = 1.0;
    let restarted = telemetry(0.0);
    machine.observe(&restarted);
    assert!(machine.origin_states(Some(&restarted))[0].lost);
    assert_eq!(
        machine.jog_lines(&input, &restarted, false)[0],
        "JOG 0 0.00000"
    );
}
#[test]
fn holding_outside_a_soft_limit_does_not_command_a_return() {
    let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
    let start = telemetry(0.0);
    machine.capture_origin(0, Some(&start));
    machine.hold_at_measured(Some(&telemetry(8.0)));
    assert_eq!(machine.targets[0], 200.0);
}

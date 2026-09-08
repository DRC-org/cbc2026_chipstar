use super::*;
use crate::protocol::telemetry::{RunMode, SlotState};
use std::time::Duration;

fn fixture() -> (MachineController, ControllerState, Telemetry) {
    let mut profile = MachineProfile::embedded().unwrap();
    profile.axes.truncate(1);
    let a = &mut profile.axes[0];
    a.input_axis = Some(0);
    a.input_sign = 1.0;
    a.native_per_unit = -2.0;
    a.speed_per_second = 20.0;
    a.jog_ramp_seconds = 0.5;
    a.minimum = 0.0;
    a.maximum = 120.0;
    a.origin_position = 100.0;
    a.limit = Some(AxisLimit {
        input: 0,
        direction: 1.0,
        normally_closed: false,
    });
    let mut machine = MachineController::new(profile);
    let mut t = Telemetry {
        uptime_ms: 0,
        mode: RunMode::Run,
        enabled_slots: 1,
        error_bits: [0; 3],
        contacts: Some(0),
        stale_slots: 0,
        buses: 3,
        slots: [SlotState {
            target: -200.0,
            measured: -200.0,
        }; 3],
    };
    machine.capture_origin(0, Some(&t));
    t.slots[0].measured = -100.0;
    let mut input = ControllerState::default();
    input.axes[0] = 1.0;
    (machine, input, t)
}
fn step(m: &mut MachineController, input: &ControllerState, t: &Telemetry, now: Instant) -> f32 {
    let lines = m.ramped_jog_lines(input, t, false, now);
    if lines[0].starts_with("TARGET") {
        0.0
    } else {
        lines[0]
            .split_whitespace()
            .last()
            .unwrap()
            .parse::<f32>()
            .unwrap()
            / -2.0
    }
}
#[test]
fn reversal_and_neutral_respect_acceleration_and_finish_at_rest() {
    let (mut m, mut input, t) = fixture();
    let start = Instant::now();
    let mut previous: f32 = 0.0;
    for i in 0..130 {
        if i == 30 {
            input.axes[0] = -1.0;
        }
        if i == 80 {
            input.axes[0] = 0.0;
        }
        let velocity = step(&mut m, &input, &t, start + Duration::from_millis(i * 20));
        assert!((velocity - previous).abs() <= 0.801);
        if i == 29 {
            assert!((velocity - 20.0).abs() < 0.001);
        }
        if i == 30 {
            assert!(velocity > 0.0);
        }
        previous = velocity;
    }
    assert_eq!(previous, 0.0);
}
#[test]
fn brakes_before_known_switch_even_when_configured_maximum_is_beyond_it() {
    let (mut m, input, mut t) = fixture();
    let start = Instant::now();
    let mut position: f32 = 50.0;
    let mut previous: f32 = 0.0;
    let mut decelerated = false;
    for i in 0..700 {
        t.slots[0].measured = -2.0 * position;
        let velocity = step(&mut m, &input, &t, start + Duration::from_millis(i * 20));
        if velocity < previous - 0.001 {
            decelerated = true;
        }
        assert!((velocity - previous).abs() <= 0.801);
        position += velocity * 0.02;
        assert!(position <= 100.0001);
        previous = velocity;
    }
    assert!(decelerated);
    assert!(position > 99.99);
}
#[test]
fn switch_and_stop_reset_override_a_pending_ramp() {
    let (mut m, input, mut t) = fixture();
    let start = Instant::now();
    for i in 0..30 {
        step(&mut m, &input, &t, start + Duration::from_millis(i * 20));
    }
    t.contacts = Some(1);
    assert_eq!(
        step(&mut m, &input, &t, start + Duration::from_millis(600)),
        0.0
    );
    t.contacts = Some(0);
    m.reset_jog();
    assert_eq!(
        step(&mut m, &input, &t, start + Duration::from_secs(1)),
        0.0
    );
    let mut retreat = input.clone();
    retreat.axes[0] = -1.0;
    t.contacts = Some(1);
    assert!(step(&mut m, &retreat, &t, start + Duration::from_millis(1020)) < 0.0);
}

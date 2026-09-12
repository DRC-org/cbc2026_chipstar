use super::*;
use crate::protocol::telemetry::{RunMode, SlotState};
use std::time::Duration;

fn fixture(r: f32, theta: f32, offset: f32) -> (MachineController, Telemetry) {
    let mut profile = MachineProfile::embedded().unwrap();
    profile.xy.speed_mm_per_second = 100.0;
    profile.xy.radius_offset_mm = offset;
    profile.xy.y_min_mm = None;
    profile.xy.y_max_mm = None;
    profile.slow_speed_percent = 20.0;
    for axis in &mut profile.axes {
        axis.input_sign = 1.0;
        axis.input_axis = Some(match axis.name.as_str() {
            "r" => 1,
            "theta" => 0,
            _ => 6,
        });
        axis.jog_ramp_seconds = 0.0;
        axis.limit = None;
        axis.native_per_unit = if axis.name == "theta" { -2.0 } else { 3.0 };
        axis.speed_per_second = if axis.name == "theta" { 90.0 } else { 200.0 };
        axis.minimum = if axis.name == "theta" { -180.0 } else { 0.0 };
        axis.maximum = if axis.name == "theta" { 180.0 } else { 1000.0 };
    }
    let mut machine = MachineController::new(profile);
    let t = Telemetry {
        held_slots: Some(0),
        uptime_ms: 0,
        mode: RunMode::Run,
        enabled_slots: 7,
        error_bits: [0; 3],
        contacts: Some(0),
        stale_slots: 0,
        buses: 3,
        slots: [SlotState {
            measured: 123.0,
            target: 123.0,
        }; 3],
    };
    for (i, coordinate) in [r, theta, 100.0].into_iter().enumerate() {
        assert!(machine.capture_coordinate(i, Some(&t), coordinate));
    }
    (machine, t)
}
fn input(x: f32, y: f32) -> ControllerState {
    let mut input = ControllerState::default();
    input.axes[0] = x;
    input.axes[1] = y;
    input
}
fn velocities(
    m: &mut MachineController,
    t: &Telemetry,
    input: &ControllerState,
    slow: bool,
    now: Instant,
) -> [f32; 3] {
    let lines = m.ramped_xy_jog_lines(input, t, slow, now);
    let mut values = [0.0; 3];
    for (axis, line) in m.profile.axes.iter().zip(lines) {
        if line.starts_with("JOG ") {
            values[axis.slot as usize] = line
                .split_whitespace()
                .last()
                .unwrap()
                .parse::<f32>()
                .unwrap()
                / axis.native_per_unit;
        }
    }
    values
}
fn world([r, theta, _]: [f32; 3], radius: f32, angle: f32) -> [f32; 2] {
    let (sin, cos) = angle.to_radians().sin_cos();
    [
        -r * sin - radius * cos * theta.to_radians(),
        r * cos - radius * sin * theta.to_radians(),
    ]
}
fn close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.002, "{actual} != {expected}");
}

#[test]
fn xy_deadzone_matches_the_neutral_check_used_to_start_and_switch_modes() {
    for x in [-0.099, 0.0, 0.099] {
        for y in [-0.099, 0.0, 0.099] {
            assert_eq!(xy::stick(&input(x, y)), [0.0; 2]);
        }
    }
    assert_eq!(xy::stick(&input(0.01, 0.5)), [0.01, 0.5]);
}

#[test]
fn fixed_xy_directions_follow_measured_theta_and_keep_small_joint_components() {
    for theta in [-135.0, -90.0, -3.0, 0.0, 45.0, 90.0, 135.0] {
        for [x, y] in [[1.0, 0.0], [0.0, 1.0], [-0.6, 0.8], [0.01, 1.0]] {
            let (mut m, t) = fixture(300.0, theta, 100.0);
            let i = input(x, y);
            let got = world(
                velocities(&mut m, &t, &i, false, Instant::now()),
                400.0,
                theta,
            );
            let expected = xy::stick(&i).map(|v| v * 100.0);
            close(got[0], expected[0]);
            close(got[1], expected[1]);
        }
    }
}

#[test]
fn radius_offset_is_used_even_at_r_zero_and_triggers_still_drive_z() {
    let (mut m, t) = fixture(0.0, 0.0, 200.0);
    let mut i = input(1.0, 0.0);
    i.axes[5] = 0.4;
    let v = velocities(&mut m, &t, &i, false, Instant::now());
    close(v[0], 0.0);
    close(v[1], (-100.0_f32 / 200.0).to_degrees());
    close(v[2], 80.0);
}

#[test]
fn diagonal_and_low_speed_are_limited_as_one_xy_vector() {
    for slow in [false, true] {
        let (mut m, t) = fixture(300.0, 0.0, 100.0);
        let v = world(
            velocities(&mut m, &t, &input(1.0, 1.0), slow, Instant::now()),
            400.0,
            0.0,
        );
        close(v[0], v[1]);
        close(v[0].hypot(v[1]), if slow { 20.0 } else { 100.0 });
    }
}

#[test]
fn joint_speed_and_limit_switch_scale_both_axes_without_bending_direction() {
    let (mut m, mut t) = fixture(300.0, 30.0, 100.0);
    m.profile.axes[1].speed_per_second = 2.0;
    let v = velocities(&mut m, &t, &input(1.0, 1.0), false, Instant::now());
    assert!(v[1].abs() <= 2.00001);
    let v = world(v, 400.0, 30.0);
    close(v[0], v[1]);
    assert!(v[0] > 0.0);
    m.profile.axes[0].limit = Some(AxisLimit {
        input: 0,
        direction: 1.0,
        normally_closed: false,
    });
    t.contacts = Some(1);
    let v = velocities(&mut m, &t, &input(1.0, 1.0), false, Instant::now());
    assert_eq!([v[0], v[1]], [0.0; 2]);
    let v = velocities(&mut m, &t, &input(-1.0, -1.0), false, Instant::now());
    assert!(v[0] < 0.0);
}

#[test]
fn invalid_pose_holds_both_planar_axes_instead_of_falling_back_to_rtheta() {
    for case in 0..5 {
        let (mut m, mut t) = fixture(300.0, 0.0, 100.0);
        match case {
            0 => m.invalidate_origin(0),
            1 => t.stale_slots = 2,
            2 => t.slots[1].measured = f32::NAN,
            3 => {
                m.capture_coordinate(0, Some(&t), 0.0);
                m.profile.xy.radius_offset_mm = 0.0;
            }
            _ => m.set_soft_limits(false),
        }
        let mut i = input(1.0, 1.0);
        i.axes[5] = 0.4;
        let v = velocities(&mut m, &t, &i, false, Instant::now());
        assert_eq!([v[0], v[1]], [0.0; 2]);
        assert!(v[2] > 0.0);
    }
}

#[test]
fn xy_acceleration_preserves_direction_and_neutral_finishes_at_measured_position() {
    let (mut m, t) = fixture(300.0, 45.0, 100.0);
    m.profile.axes[0].jog_ramp_seconds = 1.0;
    m.profile.axes[1].jog_ramp_seconds = 1.0;
    let start = Instant::now();
    let mut previous = [0.0; 2];
    for n in 0..100 {
        let i = if n < 40 {
            input(1.0, 0.0)
        } else {
            input(0.0, 0.0)
        };
        let v = world(
            velocities(&mut m, &t, &i, false, start + Duration::from_millis(n * 20)),
            400.0,
            45.0,
        );
        close(v[1], 0.0);
        assert!((v[0] - previous[0]).abs() <= 4.002);
        previous = v;
    }
    assert_eq!(previous, [0.0; 2]);
    assert!(m.jog_at_rest());
    let lines = m.ramped_xy_jog_lines(&input(0.0, 0.0), &t, false, start + Duration::from_secs(2));
    assert_eq!(lines[0], "TARGET 0 123.00000");
    assert_eq!(lines[1], "TARGET 1 123.00000");
}

#[test]
fn xy_settings_round_trip_and_old_profiles_keep_defaults() {
    let mut p = MachineProfile::embedded().unwrap();
    p.xy = Default::default();
    let text = toml::to_string_pretty(&p).unwrap();
    assert!(!text.contains("[xy]"));
    assert_eq!(MachineProfile::parse(&text).unwrap().xy, p.xy);
    p.xy.radius_offset_mm = 123.4;
    p.xy.speed_mm_per_second = 80.0;
    p.xy.y_min_mm = Some(-250.0);
    p.xy.y_max_mm = Some(850.0);
    assert_eq!(
        MachineProfile::parse(&toml::to_string_pretty(&p).unwrap())
            .unwrap()
            .xy,
        p.xy
    );
    p.xy.speed_mm_per_second = f32::NAN;
    assert!(p.validate().is_err());
}

#[test]
fn xy_lost_measurement_never_sends_nan_and_holds_the_restored_position() {
    let (mut m, mut t) = fixture(300.0, 45.0, 100.0);
    let now = Instant::now();
    m.ramped_xy_jog_lines(&input(1.0, 0.0), &t, false, now);
    t.slots[1].measured = f32::NAN;
    let lines = m.ramped_xy_jog_lines(&input(1.0, 0.0), &t, false, now);
    assert!(lines.iter().all(|line| !line.contains("NaN")));
    assert_eq!(lines[1], "JOG 1 0.00000");
    t.slots[1].measured = 127.0;
    let lines = m.ramped_xy_jog_lines(&input(0.0, 0.0), &t, false, now);
    assert_eq!(lines[1], "TARGET 1 127.00000");
}

#[test]
fn xy_low_speed_press_ramps_down_instead_of_clipping_the_current_velocity() {
    let (mut m, t) = fixture(300.0, 0.0, 100.0);
    m.profile.xy.speed_mm_per_second = 500.0;
    m.profile.axes[1].speed_per_second = 30.0;
    m.profile.axes[0].jog_ramp_seconds = 1.0;
    m.profile.axes[1].jog_ramp_seconds = 1.0;
    let start = Instant::now();
    let mut previous = 0.0;
    for n in 0..200 {
        let v = world(
            velocities(
                &mut m,
                &t,
                &input(1.0, 0.0),
                n >= 100,
                start + Duration::from_millis(n * 20),
            ),
            400.0,
            0.0,
        );
        assert!(
            (v[0] - previous).abs() <= 4.002,
            "step {n}: {previous} -> {}",
            v[0]
        );
        if n == 100 {
            assert!(v[0] > 200.0);
        }
        previous = v[0];
    }
    close(previous, 400.0 * 6.0_f32.to_radians());
}

#[test]
fn y_limit_profiles_validate_both_sides_and_optional_bounds() {
    for (min, max, valid) in [
        (None, None, true),
        (Some(-100.0), None, true),
        (None, Some(800.0), true),
        (Some(-300.0), Some(800.0), true),
        (Some(300.0), Some(300.0), false),
        (Some(301.0), Some(300.0), false),
        (Some(f32::NAN), None, false),
        (None, Some(f32::INFINITY), false),
        (None, Some(10001.0), false),
    ] {
        let profile = xy::XyProfile {
            y_min_mm: min,
            y_max_mm: max,
            ..Default::default()
        };
        assert_eq!(profile.validate().is_ok(), valid, "{profile:?}");
        if valid {
            let restored: xy::XyProfile =
                toml::from_str(&toml::to_string(&profile).unwrap()).unwrap();
            assert_eq!(restored, profile);
        }
    }
}

#[test]
fn xy_position_uses_radius_offset_and_live_measurements_without_fabricated_zeros() {
    for theta in [0.0, 60.0, -60.0, 90.0, 180.0] {
        let (mut m, mut t) = fixture(100.0, theta, 300.0);
        let position = m.xy_position(Some(&t)).unwrap();
        close(position[0], -400.0 * theta.to_radians().sin());
        close(position[1], 400.0 * theta.to_radians().cos());
        t.slots[0].measured += 60.0 * m.profile.axes[0].native_per_unit;
        close(
            m.xy_position(Some(&t)).unwrap()[1],
            460.0 * theta.to_radians().cos(),
        );
        t.stale_slots = 2;
        assert!(m.xy_position(Some(&t)).is_none());
        t.stale_slots = 0;
        m.invalidate_origin(0);
        assert!(m.xy_position(Some(&t)).is_none());
        assert!(m.xy_position(None).is_none());
    }
}

#[test]
fn y_boundaries_stop_outward_motion_but_allow_return_sideways_and_z() {
    for theta in [0.0, 60.0, -60.0, 135.0, -135.0, 180.0] {
        for direction in [-1.0, 1.0] {
            for outside in [0.0, 5.0] {
                let (mut m, t) = fixture(300.0, theta, 100.0);
                // Y境界を単独で検証する。θの可動域による制限は別のテストで確認する。
                m.profile.axes[1].minimum = -360.0;
                m.profile.axes[1].maximum = 360.0;
                let y = m.xy_position(Some(&t)).unwrap()[1];
                if direction > 0.0 {
                    m.profile.xy.y_max_mm = Some(y - outside);
                } else {
                    m.profile.xy.y_min_mm = Some(y + outside);
                }
                let mut outward = input(1.0, direction);
                outward.axes[5] = 0.4;
                let v = velocities(&mut m, &t, &outward, false, Instant::now());
                close(v[0], 0.0);
                close(v[1], 0.0);
                close(v[2], 80.0);
                let back = world(
                    velocities(&mut m, &t, &input(0.0, -direction), false, Instant::now()),
                    400.0,
                    theta,
                );
                assert!(
                    back[1] * direction < 0.0,
                    "theta={theta}, direction={direction}, outside={outside}, back={back:?}"
                );
                let sideways = world(
                    velocities(&mut m, &t, &input(1.0, 0.0), false, Instant::now()),
                    400.0,
                    theta,
                );
                assert!(sideways[0] > 0.0);
                close(sideways[1], 0.0);
            }
        }
    }
}

#[test]
fn y_boundary_scales_diagonal_direction_and_cancels_residual_outward_velocity() {
    let (mut m, t) = fixture(300.0, 45.0, 100.0);
    let y = m.xy_position(Some(&t)).unwrap()[1];
    m.profile.xy.y_max_mm = Some(y + 1.0);
    let v = world(
        velocities(&mut m, &t, &input(1.0, 1.0), false, Instant::now()),
        400.0,
        45.0,
    );
    close(v[0], v[1]);
    assert!(v[1] > 0.0 && v[1] <= 5.002);
    for axis in &mut m.profile.axes {
        axis.jog_ramp_seconds = 1.0;
    }
    m.xy_velocity = [50.0, 50.0];
    m.profile.xy.y_max_mm = Some(y);
    let v = velocities(&mut m, &t, &input(0.0, 0.0), false, Instant::now());
    close(v[0], 0.0);
    close(v[1], 0.0);
    assert_eq!(m.xy_velocity, [0.0, 0.0]);
}

#[test]
fn y_motion_brakes_to_each_boundary_without_crossing_and_can_back_away() {
    for (direction, boundary) in [(1.0, 300.0), (-1.0, 150.0)] {
        let (mut m, mut t) = fixture(100.0, 0.0, 100.0);
        for axis in &mut m.profile.axes {
            axis.jog_ramp_seconds = 0.5;
        }
        if direction > 0.0 {
            m.profile.xy.y_max_mm = Some(boundary);
        } else {
            m.profile.xy.y_min_mm = Some(boundary);
        }
        let start = Instant::now();
        let mut peak: f32 = 0.0;
        let mut last: f32 = 0.0;
        for n in 0..1000 {
            let v = velocities(
                &mut m,
                &t,
                &input(0.0, direction),
                false,
                start + Duration::from_millis(n * 20),
            );
            peak = peak.max(v[0].abs());
            last = v[0].abs();
            t.slots[0].measured += v[0] * 0.02 * m.profile.axes[0].native_per_unit;
            let y = m.xy_position(Some(&t)).unwrap()[1];
            assert!(
                (y - boundary) * direction <= 0.001,
                "y={y}, boundary={boundary}"
            );
        }
        assert!(peak > 20.0 && last < 0.1, "peak={peak}, last={last}");
        close(m.xy_position(Some(&t)).unwrap()[1], boundary);
        let v = velocities(
            &mut m,
            &t,
            &input(0.0, -direction),
            false,
            start + Duration::from_secs(20),
        );
        assert!(v[0] * direction < 0.0);
    }
}

#[test]
fn y_limits_do_not_replace_the_joint_limits_or_axis_based_jog() {
    let (mut m, t) = fixture(300.0, 0.0, 100.0);
    m.profile.xy.y_max_mm = Some(400.0);
    let lines = m.ramped_jog_lines(&input(0.0, 1.0), &t, false, Instant::now());
    assert!(lines[0].starts_with("JOG 0 "));
    assert!(!lines[0].ends_with(" 0.00000"));
    m.reset_jog();
    m.profile.xy.y_max_mm = Some(900.0);
    m.profile.axes[0].maximum = 300.0;
    let v = velocities(&mut m, &t, &input(0.0, 1.0), false, Instant::now());
    close(v[0], 0.0);
    close(v[1], 0.0);
}

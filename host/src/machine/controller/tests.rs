use super::*;
use crate::machine::profile::EMBEDDED_PROFILE;

fn neutral_input() -> ControllerState {
    ControllerState {
        axes: [0.0; 6],
        buttons: [0; 17],
    }
}

#[test]
fn embedded_profile_is_valid() {
    let profile = MachineProfile::embedded().unwrap();
    assert_eq!(profile.axes.len(), 3);
    assert_eq!(profile.axes[0].name, "r");
    for name in ["r", "z"] {
        let limit = profile
            .axes
            .iter()
            .find(|axis| axis.name == name)
            .and_then(|axis| axis.limit)
            .unwrap();
        assert!(!limit.normally_closed, "{name}のリミットはNO接点");
        assert!(!limit.reached(0), "{name}の未押下を到達扱いしている");
        assert!(
            limit.reached(1 << limit.input),
            "{name}の押下を検出できない"
        );
    }
}

#[test]
fn serial_servo_bringup_profile_is_valid_and_initially_disabled() {
    let profile = MachineProfile::parse(include_str!("../../../config/serial_svmd_bringup.toml"));
    assert!(profile.is_ok(), "{profile:?}");
    let profile = profile.unwrap();
    assert!(profile.axes.is_empty());
    assert!(profile.requires_serial_svmd());
    assert!(profile.requires_can_bus_2());
    assert!(
        profile
            .serial_svmd
            .as_ref()
            .unwrap()
            .servos
            .iter()
            .all(|servo| !servo.enabled)
    );
}

#[test]
fn every_axis_can_be_jogged_for_manual_checks() {
    // 配線後の手動確認とホーミングに必要。動かせない軸があると原点が採れない。
    let profile = MachineProfile::embedded().unwrap();
    for axis in &profile.axes {
        assert!(
            axis.input_axis.is_some() && axis.speed_per_second > 0.0,
            "{} を手動で動かせない",
            axis.name
        );
    }
}

/// `[parameters]` の検証用に、最小構成のプロファイルを組み立てる。
fn profile_with_parameters(body: &str) -> String {
    format!(
        r#"
protocol_version = 1

[parameters]
{body}

[[axes]]
name = "r"
unit = "mm"
slot = 0
input_axis = 1
speed_per_second = 10.0
native_per_unit = 1.0
minimum = -10.0
maximum = 10.0
initial = 0.0
"#
    )
}

#[test]
fn rejects_unknown_parameter_names() {
    let source = profile_with_parameters("no_such_gain = 1.0");
    assert!(MachineProfile::parse(&source).is_err());
}

#[test]
fn rejects_duplicate_slots() {
    let source = EMBEDDED_PROFILE.replace("slot = 1", "slot = 0");
    assert!(MachineProfile::parse(&source).is_err());
}

#[test]
fn converts_manual_velocity_to_native_units() {
    let mut machine = MachineController::new(MachineProfile::embedded().unwrap());
    machine.set_soft_limits(false);
    let mut input = neutral_input();
    input.axes[1] = 0.5;
    input.axes[0] = 0.5;
    let lines = machine.jog_lines(&input, &telemetry_with(0, [0.0; 3]), false);
    assert_eq!(lines[0], "JOG 0 -0.20000");
    let theta_velocity: f32 = lines[1].split_whitespace().last().unwrap().parse().unwrap();
    assert!((theta_velocity + 660.1103).abs() < 0.001);
    assert_eq!(lines[2], "JOG 2 0.00000");
}

#[test]
fn deadzone_nonfinite_input_and_low_speed_are_bounded() {
    let mut machine = MachineController::new(MachineProfile::embedded().unwrap());
    machine.set_soft_limits(false);
    let t = telemetry_with(0, [0.0; 3]);
    let mut input = neutral_input();
    for value in [0.05, f32::NAN, f32::INFINITY] {
        input.axes[1] = value;
        let lines = machine.jog_lines(&input, &t, false);
        let velocity: f32 = lines[0].split_whitespace().last().unwrap().parse().unwrap();
        assert_eq!(velocity, 0.0);
    }
    input.axes[1] = 2.0;
    assert_eq!(machine.jog_lines(&input, &t, true)[0], "JOG 0 -0.08000");
}

/// SW1が閉じた状態（B接点の平常時）のテレメトリ。
fn telemetry_with(contacts: u8, measured: [f32; 3]) -> Telemetry {
    use crate::protocol::telemetry::{RunMode, SlotState};
    Telemetry {
        uptime_ms: 0,
        slots: [
            SlotState {
                target: 0.0,
                measured: measured[0],
            },
            SlotState {
                target: 0.0,
                measured: measured[1],
            },
            SlotState {
                target: 0.0,
                measured: measured[2],
            },
        ],
        enabled_slots: 7,
        mode: RunMode::Run,
        error_bits: [0; 3],
        contacts: Some(contacts),
        stale_slots: 0,
        buses: 3,
    }
}

/// リミットスイッチを持つ機体のプロファイル。
///
/// 実機はまだスイッチ未取付で `config/rtheta.toml` の `[axes.limit]` を
/// 無効にしてあるため、リミットの検証はここで組み立てた構成で行う。
fn profile_with_limits() -> MachineProfile {
    MachineProfile::parse(
        r#"
protocol_version = 1

[[axes]]
name = "r"
unit = "mm"
slot = 0
input_axis = 1
input_sign = 1.0
speed_per_second = 100.0
native_per_unit = 0.10005072
minimum = 0.0
maximum = 120.0
initial = 0.0
origin_position = 120.0

[axes.limit]
input = 0
direction = 1.0

[[axes]]
name = "theta"
unit = "deg"
slot = 1
input_axis = 0
input_sign = 1.0
speed_per_second = 90.0
native_per_unit = 140.82353
minimum = -180.0
maximum = 180.0
initial = 0.0

[[axes]]
name = "z"
unit = "mm"
slot = 2
input_axis = 3
input_sign = 1.0
speed_per_second = 30.0
native_per_unit = 0.15707964
minimum = 0.0
maximum = 75.0
initial = 0.0

[axes.limit]
input = 1
direction = -1.0
"#,
    )
    .unwrap()
}

#[test]
fn power_cycling_a_motor_invalidates_the_origin() {
    let mut machine = MachineController::new(MachineProfile::embedded().unwrap());
    let steady = telemetry_with(0, [5.0, 0.0, 0.0]);
    machine.observe(&steady);
    assert!(machine.capture_origin(0, Some(&steady)));
    assert!(machine.origin_states(None)[0].captured);

    // モータが入り直して実測が0へ飛ぶ。1周期では起こりえない移動量。
    let restarted = telemetry_with(0, [0.0, 0.0, 0.0]);
    machine.observe(&restarted);
    let origins = machine.origin_states(None);
    assert!(!origins[0].captured, "原点を捨てていない");
    assert!(origins[0].lost, "失ったことを伝えていない");
}

#[test]
fn feedback_loss_reported_by_the_board_invalidates_the_origin() {
    let mut machine = MachineController::new(MachineProfile::embedded().unwrap());
    let steady = telemetry_with(0, [5.0, 0.0, 0.0]);
    machine.observe(&steady);
    assert!(machine.capture_origin(0, Some(&steady)));

    let lost = Telemetry {
        stale_slots: 0b001,
        ..telemetry_with(0, [5.0, 0.0, 0.0])
    };
    machine.observe(&lost);
    assert!(!machine.origin_states(None)[0].captured);
    assert!(machine.origin_states(None)[0].lost);
}

#[test]
fn normal_jogging_does_not_invalidate_the_origin() {
    let mut machine = MachineController::new(MachineProfile::embedded().unwrap());
    let start = telemetry_with(0, [0.0, 0.0, 0.0]);
    machine.observe(&start);
    assert!(machine.capture_origin(0, Some(&start)));

    // ジョグ相当の連続した移動では原点を捨てない。
    for step in 1..20 {
        let moving = telemetry_with(0, [step as f32 * 0.02, 0.0, 0.0]);
        machine.observe(&moving);
        assert!(
            machine.origin_states(None)[0].captured,
            "step {step} で捨てた"
        );
    }
}

#[test]
fn neutral_command_holds_after_manual_repositioning() {
    let mut machine = MachineController::new(MachineProfile::embedded().unwrap());
    machine.observe(&telemetry_with(0, [0.2, 0.0, 0.0]));
    let moved = telemetry_with(0, [0.4, 0.0, 0.0]);
    machine.observe(&moved);
    assert!((machine.target("r").unwrap() - 10.0).abs() < 1e-3);
    assert_eq!(
        machine.jog_lines(&neutral_input(), &moved, false)[0],
        "JOG 0 0.00000"
    );
}

#[test]
fn holding_needs_feedback() {
    let mut machine = MachineController::new(MachineProfile::embedded().unwrap());
    machine.observe(&telemetry_with(0, [0.4, 0.0, 0.0]));
    let before = machine.target("r").unwrap();
    machine.hold_at_measured(None);
    assert_eq!(machine.target("r"), Some(before));
}

#[test]
fn captures_origin_on_the_limit_edge_without_moving_the_axis() {
    let mut machine = MachineController::new(profile_with_limits());
    machine.observe(&telemetry_with(0b011, [0.0; 3]));
    assert!(!machine.origin_states(None)[0].captured);
    let reached = telemetry_with(0b010, [8.0, 0.0, 0.0]);
    machine.observe(&reached);
    let origins = machine.origin_states(Some(&reached));
    assert!(origins[0].captured);
    assert!((origins[0].position - 120.0).abs() < 1e-3);
    assert!((origins[0].target - origins[0].position).abs() < 1e-3);
    assert_eq!(
        machine.jog_lines(&neutral_input(), &reached, false)[0],
        "JOG 0 0.00000"
    );
}

#[test]
fn limit_blocks_only_the_direction_that_reaches_it() {
    let mut machine = MachineController::new(profile_with_limits());
    machine.observe(&telemetry_with(0b011, [8.0, 0.0, 0.0]));
    let reached = telemetry_with(0b010, [8.0, 0.0, 0.0]);
    machine.observe(&reached);
    let mut input = neutral_input();
    input.axes[1] = 1.0;
    assert_eq!(
        machine.jog_lines(&input, &reached, false)[0],
        "JOG 0 0.00000"
    );
    input.axes[1] = -1.0;
    assert!(machine.jog_lines(&input, &reached, false)[0].starts_with("JOG 0 -10."));
}

#[test]
fn adjustment_allows_origin_search_and_normal_mode_enforces_bounds() {
    let mut machine = MachineController::new(profile_with_limits());
    let t = telemetry_with(0b011, [0.0; 3]);
    let mut input = neutral_input();
    input.axes[1] = 1.0;
    assert_eq!(machine.jog_lines(&input, &t, false)[0], "JOG 0 0.00000");
    machine.set_soft_limits(false);
    assert!(machine.jog_lines(&input, &t, false)[0].starts_with("JOG 0 10."));
    assert!(machine.capture_origin(0, Some(&t)));
    machine.set_soft_limits(true);
    assert_eq!(machine.jog_lines(&input, &t, false)[0], "JOG 0 0.00000");
}

#[test]
fn switchless_axis_needs_an_origin_before_normal_motion() {
    let machine = MachineController::new(MachineProfile::embedded().unwrap());
    let mut input = neutral_input();
    input.axes[0] = 1.0;
    assert_eq!(
        machine.jog_lines(&input, &telemetry_with(0, [0.0; 3]), false)[1],
        "JOG 1 0.00000"
    );
}

#[test]
fn unknown_contacts_block_only_axes_with_a_configured_switch() {
    let t = Telemetry {
        contacts: None,
        ..telemetry_with(0, [0.0; 3])
    };
    let mut input = neutral_input();
    input.axes[1] = 1.0;
    let mut without_limits = MachineProfile::embedded().unwrap();
    for axis in &mut without_limits.axes {
        axis.limit = None;
    }
    for (profile, blocked) in [(profile_with_limits(), true), (without_limits, false)] {
        let mut machine = MachineController::new(profile);
        machine.set_soft_limits(false);
        machine.observe(&t);
        assert_eq!(
            machine.jog_lines(&input, &t, false)[0] == "JOG 0 0.00000",
            blocked
        );
        assert!(!machine.origin_states(Some(&t))[0].captured);
        assert_eq!(machine.origin_states(Some(&t))[0].at_limit, None);
    }
}

#[test]
fn axis_without_a_switch_is_captured_only_by_hand() {
    let mut machine = MachineController::new(MachineProfile::embedded().unwrap());
    let telemetry = telemetry_with(0b011, [0.0, 3000.0, 0.0]);
    machine.observe(&telemetry);
    let theta = 1;
    assert_eq!(
        machine.origin_states(Some(&telemetry))[theta].at_limit,
        None
    );
    assert!(!machine.origin_states(Some(&telemetry))[theta].captured);

    assert!(machine.capture_origin(theta, Some(&telemetry)));
    let states = machine.origin_states(Some(&telemetry));
    assert!(states[theta].captured);
    assert_eq!(states[theta].position, 0.0);
    // 採用直後は指令も実測に一致し、軸は動かない。
    let lines = machine.jog_lines(&neutral_input(), &telemetry, false);
    let velocity: f32 = lines[1].split_whitespace().last().unwrap().parse().unwrap();
    assert_eq!(velocity, 0.0);
}

#[test]
fn rejects_a_limit_on_a_contact_the_board_does_not_have() {
    // 接点は0..2しかない。3を指定した構成は受け付けない。
    let source = format!("{EMBEDDED_PROFILE}\n[axes.limit]\ninput = 3\ndirection = 1.0\n");
    assert!(MachineProfile::parse(&source).is_err());
}

#[test]
fn accepts_but_does_not_drive_pwm_servo_from_host_profile() {
    let mut profile = MachineProfile::embedded().unwrap();
    profile.pwm_servos = vec![PwmServoProfile {
        name: "gripper".into(),
        channel: 1,
        input_axis: Some(3),
        input_sign: -1.0,
        speed_us_per_second: 1000.0,
        minimum_us: 900,
        maximum_us: 2100,
        initial_us: 1500,
        enabled: true,
    }];
    profile.validate().unwrap();
    assert!(profile.requires_can_bus_2());
    let mut machine = MachineController::new(profile);
    let t = telemetry_with(0, [0.0; 3]);
    machine.observe(&t);
    let lines = machine.jog_lines(&neutral_input(), &t, false);
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|line| line.starts_with("JOG ")));
}

#[test]
fn accepts_but_does_not_drive_serial_servo_from_host_profile() {
    let source = format!(
        "{EMBEDDED_PROFILE}\n[serial_svmd]\n\n[[serial_svmd.servos]]\nname = \"arm\"\nid = 12\ninput_axis = 4\ninput_sign = 1.0\nspeed_position_per_second = 500.0\nminimum_position = 1000\nmaximum_position = 3000\ninitial_position = 2000\nmove_speed = 400\nacceleration = 30\nenabled = true\n"
    );
    let profile = MachineProfile::parse(&source).unwrap();
    assert!(profile.requires_serial_svmd());
    assert!(profile.requires_can_bus_2());
    let mut machine = MachineController::new(profile);
    let t = telemetry_with(0, [0.0; 3]);
    machine.observe(&t);
    let lines = machine.jog_lines(&neutral_input(), &t, false);
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|line| line.starts_with("JOG ")));
}

#[test]
fn m3508_z_profile_uses_rotor_degrees_and_rejects_legacy_dm() {
    let mut profile = MachineProfile::embedded().unwrap();
    let z = profile.axes.iter().find(|a| a.slot == 2).unwrap();
    assert!((z.native_per_unit * 72.0 - 360.0 * (3591.0 / 187.0)).abs() < 0.001);
    assert_eq!(profile.parameters["c620_slot2_esc_id"], 2.0);
    profile.parameters.insert("c620_slot2_esc_id".into(), 1.0);
    assert!(profile.validate().is_err());
    profile.parameters.insert("c620_slot2_esc_id".into(), 2.0);
    profile.parameters.insert("dm_p_max".into(), 2048.0);
    assert!(profile.validate().is_err());
}

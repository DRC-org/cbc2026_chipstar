
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
    let profile = MachineProfile::load(None).unwrap();
    assert_eq!(profile.axes.len(), 3);
    assert_eq!(profile.axes[0].name, "r");
}

#[test]
fn every_axis_can_be_jogged_for_manual_checks() {
    // 配線後の手動確認とホーミングに必要。動かせない軸があると原点が採れない。
    let profile = MachineProfile::load(None).unwrap();
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
fn sends_named_parameters_as_numeric_ids() {
    let source = profile_with_parameters("m3508_vel_kp = 0.9\nwatchdog_ms = 300.0");
    let profile = MachineProfile::parse(&source).unwrap();
    let lines = profile.parameter_lines();
    assert!(lines.contains(&"PARAM 4 0.90000".to_owned()));
    assert!(lines.contains(&"PARAM 30 300.00000".to_owned()));
}

#[test]
fn sends_can_board_parameters_through_the_gateway() {
    let source = format!(
        "{EMBEDDED_PROFILE}\n[dcmd_parameters]\nmax_duty = 1000.0\n\n[serial_svmd_parameters]\nservo_baud = 1000000.0\n"
    );
    let profile = MachineProfile::parse(&source).unwrap();
    let lines = profile.parameter_lines();
    // DCMD id 0 (max_duty) = 1000.0f = 0x447A0000
    assert!(lines.contains(&"CAN 2 784 01070000447A0000".to_owned()));
    // serial_svmd id 0 (servo_baud) = 1000000.0f = 0x49742400
    assert!(lines.contains(&"CAN 2 800 0109000049742400".to_owned()));
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
fn integrates_input_and_converts_to_native_units() {
    let profile = MachineProfile::load(None).unwrap();
    let mut machine = MachineController::new(profile);
    let mut input = neutral_input();
    input.axes[1] = 0.5;

    let lines = machine.update(&input, 0.1, None);

    assert!((machine.target("r").unwrap() - 0.5).abs() < 1e-5);
    assert_eq!(lines[0], "TARGET 0 0.02000");
    assert_eq!(lines[2], "TARGET 2 0.00000");
}

#[test]
fn applies_deadzone_and_interval_limit() {
    let profile = MachineProfile::load(None).unwrap();
    let mut machine = MachineController::new(profile);
    let mut input = neutral_input();
    input.axes[0] = 0.05;
    machine.update(&input, 1.0, None);
    assert_eq!(machine.target("theta"), Some(0.0));

    input.axes[0] = 1.0;
    machine.update(&input, 1.0, None);
    assert_eq!(machine.target("theta"), Some(1.0));
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
    let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
    let steady = telemetry_with(0, [5.0, 0.0, 0.0]);
    machine.update(&neutral_input(), 0.1, Some(&steady));
    assert!(machine.capture_origin(0, Some(&steady)));
    assert!(machine.origin_states(None)[0].captured);

    // モータが入り直して実測が0へ飛ぶ。1周期では起こりえない移動量。
    let restarted = telemetry_with(0, [0.0, 0.0, 0.0]);
    machine.update(&neutral_input(), 0.1, Some(&restarted));
    let origins = machine.origin_states(None);
    assert!(!origins[0].captured, "原点を捨てていない");
    assert!(origins[0].lost, "失ったことを伝えていない");
}

#[test]
fn feedback_loss_reported_by_the_board_invalidates_the_origin() {
    let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
    let steady = telemetry_with(0, [5.0, 0.0, 0.0]);
    machine.update(&neutral_input(), 0.1, Some(&steady));
    assert!(machine.capture_origin(0, Some(&steady)));

    let lost = Telemetry {
        stale_slots: 0b001,
        ..telemetry_with(0, [5.0, 0.0, 0.0])
    };
    machine.update(&neutral_input(), 0.1, Some(&lost));
    assert!(!machine.origin_states(None)[0].captured);
    assert!(machine.origin_states(None)[0].lost);
}

#[test]
fn normal_jogging_does_not_invalidate_the_origin() {
    let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
    let start = telemetry_with(0, [0.0, 0.0, 0.0]);
    machine.update(&neutral_input(), 0.1, Some(&start));
    assert!(machine.capture_origin(0, Some(&start)));

    // ジョグ相当の連続した移動では原点を捨てない。
    for step in 1..20 {
        let moving = telemetry_with(0, [step as f32 * 0.02, 0.0, 0.0]);
        machine.update(&neutral_input(), 0.05, Some(&moving));
        assert!(
            machine.origin_states(None)[0].captured,
            "step {step} で捨てた"
        );
    }
}

#[test]
fn run_holds_the_current_position_instead_of_returning() {
    let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
    let mut input = neutral_input();

    // ジョグでrを進めてから、停止中に手で戻された状況をつくる。
    input.axes[1] = 1.0;
    for _ in 0..5 {
        machine.update(&input, 0.1, None);
    }
    assert!(machine.target("r").unwrap() > 4.0);

    // 実測は手で戻された位置。RUN前に取り込めば、その場を保持する。
    let moved = telemetry_with(0, [0.4, 0.0, 0.0]);
    machine.hold_at_measured(Some(&moved));
    assert!((machine.target("r").unwrap() - 10.0).abs() < 1e-3);

    // 直後の指令は実測と一致し、機体は動かない。
    let lines = machine.update(&neutral_input(), 0.1, Some(&moved));
    assert_eq!(lines[0], "TARGET 0 0.40000");
}

#[test]
fn holding_needs_feedback() {
    let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
    let mut input = neutral_input();
    input.axes[1] = 1.0;
    machine.update(&input, 0.1, None);
    let before = machine.target("r").unwrap();
    // テレメトリが無ければ何もしない。勝手に0へ飛ばさない。
    machine.hold_at_measured(None);
    assert_eq!(machine.target("r"), Some(before));
}

#[test]
fn captures_origin_on_the_limit_edge_without_moving_the_axis() {
    let mut machine = MachineController::new(profile_with_limits());
    let mut input = neutral_input();
    input.axes[1] = 1.0;

    // 平常時はSW1もSW2も閉じている（B接点）。
    machine.update(&input, 0.1, Some(&telemetry_with(0b011, [0.0; 3])));
    assert!(!machine.origin_states(None)[0].captured);

    // rが前進しきってSW1が開く。その瞬間の実測値に最大位置を割り当てる。
    let reached = telemetry_with(0b010, [8.0, 0.0, 0.0]);
    let lines = machine.update(&input, 0.1, Some(&reached));
    let origins = machine.origin_states(Some(&reached));
    assert!(origins[0].captured);
    assert!((origins[0].position - 120.0).abs() < 1e-3);
    // 採用の前後で軸を動かさない。
    assert_eq!(lines[0], "TARGET 0 8.00000");
}

#[test]
fn limit_blocks_only_the_direction_that_reaches_it() {
    let mut machine = MachineController::new(profile_with_limits());
    let reached = telemetry_with(0b010, [8.0, 0.0, 0.0]);
    machine.update(
        &neutral_input(),
        0.1,
        Some(&telemetry_with(0b011, [8.0, 0.0, 0.0])),
    );
    machine.update(&neutral_input(), 0.1, Some(&reached));

    // 前進側は捨てる。
    let mut forward = neutral_input();
    forward.axes[1] = 1.0;
    machine.update(&forward, 0.1, Some(&reached));
    assert!((machine.target("r").unwrap() - 120.0).abs() < 1e-3);

    // 後退側は通し、スイッチから抜けられる。
    let mut back = neutral_input();
    back.axes[1] = -1.0;
    machine.update(&back, 0.1, Some(&reached));
    assert!(machine.target("r").unwrap() < 120.0);
}

#[test]
fn clamps_travel_only_after_the_origin_is_known() {
    let mut machine = MachineController::new(profile_with_limits());
    let mut input = neutral_input();
    input.axes[1] = -1.0;

    // 未採用の間は暫定原点基準のクランプを効かせない。
    for _ in 0..5 {
        machine.update(&input, 0.1, None);
    }
    assert!(machine.target("r").unwrap() < 0.0);

    // 手動採用の後は可動域が意味を持ち、最大側で頭打ちになる。
    let telemetry = telemetry_with(0b011, [0.0, 0.0, 0.0]);
    assert!(machine.capture_origin(0, Some(&telemetry)));
    assert!((machine.target("r").unwrap() - 120.0).abs() < 1e-3);
    input.axes[1] = 1.0;
    machine.update(&input, 0.1, Some(&telemetry));
    assert!((machine.target("r").unwrap() - 120.0).abs() < 1e-3);
}

#[test]
fn clamps_switchless_axes_from_the_start() {
    let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
    let mut input = neutral_input();
    input.axes[0] = 1.0; // theta（スイッチなし）
    for _ in 0..200 {
        machine.update(&input, 0.1, None);
    }
    // 原点未採用でも可動域で頭打ちになる。ケーブルを巻き込ませない。
    assert!((machine.target("theta").unwrap() - 180.0).abs() < 1e-3);
}

#[test]
fn treats_unknown_contacts_as_no_limit_information() {
    let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
    let mut input = neutral_input();
    input.axes[1] = 1.0;
    // sw= を持たないFWでは、接点を「全て到達」と誤解して止めてはいけない。
    let unknown = Telemetry {
        contacts: None,
        ..telemetry_with(0, [0.0; 3])
    };
    machine.update(&input, 0.1, Some(&unknown));
    assert!(machine.target("r").unwrap() > 0.0);
    assert!(!machine.origin_states(Some(&unknown))[0].captured);
    assert_eq!(machine.origin_states(Some(&unknown))[0].at_limit, None);
}

#[test]
fn axis_without_a_switch_is_captured_only_by_hand() {
    let mut machine = MachineController::new(MachineProfile::load(None).unwrap());
    let telemetry = telemetry_with(0b011, [0.0, 3000.0, 0.0]);
    machine.update(&neutral_input(), 0.1, Some(&telemetry));
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
    let lines = machine.update(&neutral_input(), 0.1, Some(&telemetry));
    assert_eq!(lines[1], "TARGET 1 3000.00000");
}

#[test]
fn rejects_a_limit_on_a_contact_the_board_does_not_have() {
    // 接点は0..2しかない。3を指定した構成は受け付けない。
    let source = format!("{EMBEDDED_PROFILE}\n[axes.limit]\ninput = 3\ndirection = 1.0\n");
    assert!(MachineProfile::parse(&source).is_err());
}

#[test]
fn validates_and_drives_pwm_servo_from_host_profile() {
    let source = format!(
        "{EMBEDDED_PROFILE}\n[[pwm_servos]]\nname = \"gripper\"\nchannel = 1\ninput_axis = 3\ninput_sign = -1.0\nspeed_us_per_second = 1000.0\nminimum_us = 900\nmaximum_us = 2100\ninitial_us = 1500\nenabled = true\n"
    );
    let profile = MachineProfile::parse(&source).unwrap();
    assert!(profile.requires_can_bus_2());
    let mut machine = MachineController::new(profile);
    let mut input = neutral_input();
    input.axes[3] = 1.0;

    let lines = machine.update(&input, 0.1, None);

    assert_eq!(lines[3], "CAN 2 768 0101010005780000");
    assert_eq!(lines.len(), 4);
}

#[test]
fn validates_and_drives_serial_servo_from_host_profile() {
    let source = format!(
        "{EMBEDDED_PROFILE}\n[serial_svmd]\n\n[[serial_svmd.servos]]\nname = \"arm\"\nid = 12\ninput_axis = 4\ninput_sign = 1.0\nspeed_position_per_second = 500.0\nminimum_position = 1000\nmaximum_position = 3000\ninitial_position = 2000\nmove_speed = 400\nacceleration = 30\nenabled = true\n"
    );
    let profile = MachineProfile::parse(&source).unwrap();
    assert!(profile.requires_serial_svmd());
    assert!(profile.requires_can_bus_2());
    let mut machine = MachineController::new(profile);
    let mut input = neutral_input();
    input.axes[4] = 1.0;

    // 目標と有効化はcctlのFDCAN2経由で送る。
    let lines = machine.update(&input, 0.1, None);
    let servo: Vec<_> = lines
        .iter()
        .filter(|line| line.starts_with("CAN 2 800 "))
        .collect();
    // 目標・有効化に続けて、実測位置の読み出しを1IDぶん巡回する。
    assert_eq!(
        servo,
        [
            "CAN 2 800 01040C1E08020190",
            "CAN 2 800 01060C0100000000",
            "CAN 2 800 01070C0000000000"
        ]
    );
}

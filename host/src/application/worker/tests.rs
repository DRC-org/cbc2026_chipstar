use super::*;
#[test]
fn restarting_a_subset_does_not_enable_axes_from_an_older_profile() {
    let mut profile = MachineProfile::embedded().unwrap();
    profile.axes.truncate(1);
    let shared = Arc::new(Shared::new(BridgeConfig {
        serial_device: "unused".into(),
        baud_rate: 115200,
        rate_hz: 20.0,
        machine: profile,
        profile_path: "/dev/null".into(),
        simulate: true,
    }));
    let mut runtime = Runtime::new(shared);
    for _ in 0..40 {
        runtime.tick().unwrap();
    }
    runtime
        .machine
        .capture_origin(0, runtime.telemetry.as_ref());
    runtime.send("ENABLE 7 1").unwrap();
    runtime.start().unwrap();
    runtime.tick().unwrap();
    assert_eq!(runtime.telemetry.as_ref().unwrap().enabled_slots, 1);
}
#[test]
fn ai_control_excludes_manual_changes_and_expires_without_resuming() {
    let shared = Arc::new(Shared::new(BridgeConfig {
        serial_device: "unused".into(),
        baud_rate: 115200,
        rate_hz: 20.0,
        machine: MachineProfile::embedded().unwrap(),
        profile_path: "/dev/null".into(),
        simulate: true,
    }));
    let mut runtime = Runtime::new(shared);
    let token = runtime
        .request(&Request::new("claim"), false)
        .unwrap()
        .token
        .unwrap();
    assert!(
        runtime
            .request(
                &Request {
                    flag: Some(true),
                    ..Request::new("adjustment")
                },
                true
            )
            .is_err()
    );
    assert!(runtime.request(&Request::new("stop"), true).is_ok());
    runtime
        .request(
            &Request {
                token: Some(token),
                axis: Some("r".into()),
                value: Some(1.0),
                ..Request::new("input")
            },
            false,
        )
        .unwrap();
    runtime
        .authority
        .set_input(1, 0.5, Instant::now() - Duration::from_secs(1));
    runtime.authority.set_input(0, 0.2, Instant::now());
    runtime.tick().unwrap();
    assert_eq!(runtime.authority.input().unwrap().axes[1], 0.0);
    assert_eq!(runtime.authority.input().unwrap().axes[0], 0.2);
    assert!(runtime.authority.active());
    runtime
        .tick_at(Instant::now() + Duration::from_secs(31))
        .unwrap();
    assert!(!runtime.authority.active());
    assert!(!runtime.drive.running());
    assert!(runtime.drive.awaiting().is_none());
}

#[test]
fn status_keeps_dualsense_input_separate_from_the_active_ai_input() {
    let mut runtime = screen_runtime();
    runtime.screen_control = false;
    runtime.gamepad_name = "DualSense test".into();
    let now = Instant::now();
    let mut pad = ControllerState::default();
    pad.axes[0] = 0.4;
    pad.buttons[0] = 1;
    runtime.read_pad(pad, now).unwrap();
    runtime.authority.claim("test".into(), now);
    runtime.authority.set_input(0, -0.7, now);

    runtime.publish();
    let status = runtime.shared.status_snapshot();
    assert_eq!(status.axes[0], -0.7);
    let gamepad = status.gamepad_input.unwrap();
    assert_eq!(gamepad.axes[0], 0.4);
    assert_eq!(gamepad.buttons[0], 1);

    runtime.disconnect_pad();
    runtime.publish();
    assert!(runtime.shared.status_snapshot().gamepad_input.is_none());
}

#[test]
fn can_status_separates_bus_off_from_c620_id_mismatch() {
    let mut runtime = screen_runtime();
    runtime.can_diagnostics.insert(
        1,
        (
            crate::protocol::can::Diagnostics {
                bus: 1,
                started: true,
                bus_off: false,
                lec: 0,
                tec: 0,
                rec: 0,
                cel: 42,
                tx_failed: 9,
            },
            Instant::now(),
        ),
    );
    runtime.c620_scan = Some((0b0000_0001, Instant::now()));

    let devices = runtime.can_device_statuses();
    let theta = devices
        .iter()
        .find(|device| device.name.starts_with("theta軸"))
        .unwrap();
    let z = devices
        .iter()
        .find(|device| device.name.starts_with("z軸"))
        .unwrap();
    assert_eq!(theta.health, CommunicationHealth::Healthy);
    assert_eq!(z.health, CommunicationHealth::Fault);
    assert!(z.detail.contains("設定ID"));

    runtime.can_diagnostics.get_mut(&1).unwrap().0.bus_off = true;
    runtime.can_diagnostics.get_mut(&1).unwrap().0.tec = 255;
    let buses = runtime.can_bus_statuses();
    assert_eq!(buses[0].health, CommunicationHealth::Fault);
    assert!(buses[0].detail.contains("bus-off"));
    assert_eq!(buses[0].tec, Some(255));
}

#[test]
fn can_status_distinguishes_serial_svmd_board_from_servo_feedback() {
    let mut runtime = screen_runtime();
    runtime.cfg.machine.serial_svmd = MachineProfile::embedded().unwrap().serial_svmd;
    runtime.test.peers.insert("sts", Instant::now());

    let waiting = runtime.can_device_statuses();
    let board = waiting
        .iter()
        .find(|device| device.name == "STS3215基板")
        .unwrap();
    let servo = waiting
        .iter()
        .find(|device| device.name.contains("STS3215") && device.name != "STS3215基板")
        .unwrap();
    assert_eq!(board.health, CommunicationHealth::Healthy);
    assert_eq!(servo.health, CommunicationHealth::Fault);
    assert!(servo.detail.contains("サーボ個体"));

    runtime.servo_feedback.insert(
        1,
        ServoFeedback {
            seen: Instant::now(),
            position: 2048,
            error: 0,
            detail: "位置=2048、出力=解除、エラー=0x00".into(),
        },
    );
    let responding = runtime.can_device_statuses();
    let servo = responding
        .iter()
        .find(|device| device.name.contains("STS3215") && device.name != "STS3215基板")
        .unwrap();
    assert_eq!(servo.health, CommunicationHealth::Healthy);
}

#[test]
fn can_status_lists_a_detected_board_without_machine_configuration() {
    let mut runtime = screen_runtime();
    assert!(
        runtime
            .can_device_statuses()
            .iter()
            .all(|device| device.name != "DCモータ基板")
    );

    runtime.test.peers.insert("dc", Instant::now());
    let devices = runtime.can_device_statuses();
    let dcmd = devices
        .iter()
        .find(|device| device.name == "DCモータ基板")
        .unwrap();
    assert_eq!(dcmd.health, CommunicationHealth::Warning);
    assert_eq!(dcmd.detail, "状態応答あり・機体設定なし");
    assert!(dcmd.age_ms.is_some_and(|age| age < 2000));
}

pub(super) fn screen_runtime() -> Runtime {
    let mut profile = MachineProfile::embedded().unwrap();
    profile.pwm_servos.clear();
    profile.serial_svmd = None;
    profile.dc_motors.clear();
    profile.svmd_parameters.clear();
    profile.serial_svmd_parameters.clear();
    profile.dcmd_parameters.clear();
    let shared = Arc::new(Shared::new(BridgeConfig {
        serial_device: "unused".into(),
        baud_rate: 115200,
        rate_hz: 20.0,
        machine: profile,
        profile_path: "/dev/null".into(),
        simulate: true,
    }));
    let mut runtime = Runtime::new(shared);
    runtime.court = Some(Court::Red);
    runtime.guide.enabled = false;
    for _ in 0..40 {
        runtime.tick().unwrap();
    }
    for index in 0..runtime.cfg.machine.axes.len() {
        assert!(
            runtime
                .machine
                .capture_origin(index, runtime.telemetry.as_ref())
        );
    }
    runtime
}

#[test]
fn screen_jog_requires_running_and_expires_without_latching_on_restart() {
    let mut runtime = screen_runtime();
    // Exercise the real-mode input gate while retaining the simulated transport.
    runtime.cfg.simulate = false;
    let input = Request {
        axis: Some("r".into()),
        value: Some(1.0),
        ..Request::new("input")
    };
    assert!(runtime.request(&input, true).is_err());
    runtime.start().unwrap();
    runtime.tick().unwrap();
    assert!(runtime.drive.running());
    runtime.request(&input, true).unwrap();
    let index = runtime.cfg.machine.axes[0].input_axis.unwrap();
    assert_eq!(runtime.manual_input.axes[index], 1.0);
    runtime
        .tick_at(Instant::now() + Duration::from_millis(151))
        .unwrap();
    assert_eq!(runtime.manual_input.axes[index], 0.0);
    runtime.request(&input, true).unwrap();
    runtime.stop(false).unwrap();
    assert_eq!(runtime.manual_input.axes[index], 0.0);
    runtime.start().unwrap();
    runtime.tick().unwrap();
    assert_eq!(runtime.manual_input.axes[index], 0.0);
    runtime.stop(false).unwrap();
    runtime
        .request(
            &Request {
                flag: Some(false),
                ..Request::new("manual_control")
            },
            true,
        )
        .unwrap();
    assert!(runtime.request(&input, true).is_err());
    assert!(runtime.start().is_err());
}

#[test]
fn stopping_ee_preserves_arm_hold_but_explicit_cut_still_disables_it() {
    for extended_sts in [false, true] {
        let mut runtime = screen_runtime();
        runtime.start().unwrap();
        runtime.tick().unwrap();
        let before = runtime.telemetry.as_ref().unwrap().clone();
        runtime.test.peers.insert("sts", Instant::now());
        if extended_sts {
            runtime.sts.active = true;
        } else {
            runtime.ee.targets.insert("ee_rotation".into(), 2050.0);
        }
        runtime.request(&Request::new("stop"), true).unwrap();
        runtime.tick().unwrap();
        let held = runtime.telemetry.as_ref().unwrap();
        assert_eq!(held.mode, RunMode::Run);
        assert_eq!(held.enabled_slots, before.enabled_slots);
        assert!(!runtime.drive.running());
        assert!(runtime.ee.targets.is_empty());
        assert!(!runtime.sts.active);
        assert!(runtime.shared.status_snapshot().logs.iter().any(|l|
            l == "TX CAN 2 800 0103000000000000"));
        runtime.request(&Request::new("safe"), true).unwrap();
        runtime.tick().unwrap();
        assert_eq!(runtime.telemetry.as_ref().unwrap().mode, RunMode::Safe);
        assert_eq!(runtime.telemetry.as_ref().unwrap().held_slots, Some(0));
    }
}

#[test]
fn gui_takeover_stops_and_revokes_the_previous_token() {
    let mut runtime = screen_runtime();
    let token = runtime
        .request(&Request::new("claim"), false)
        .unwrap()
        .token
        .unwrap();
    let run = Request {
        token: Some(token.clone()),
        ..Request::new("run")
    };
    runtime.request(&run, false).unwrap();
    runtime.tick().unwrap();
    assert!(runtime.drive.running());
    assert!(
        runtime
            .request(
                &Request {
                    token: Some(token),
                    ..Request::new("takeover")
                },
                false
            )
            .is_err()
    );
    assert!(runtime.drive.running());
    runtime.request(&Request::new("takeover"), true).unwrap();
    assert!(!runtime.drive.running());
    assert!(runtime.drive.awaiting().is_none());
    assert!(!runtime.authority.active());
    assert!(runtime.request(&run, false).is_err());
}

#[test]
fn connection_mode_changes_clear_origins_and_preserve_legacy_requests() {
    let mut runtime = screen_runtime();
    let connection = |simulate: &str| Request {
        text: Some(format!(
            "serial_device = '/nonexistent/catchrobo-test'\nbaud_rate = 115200\n{simulate}"
        )),
        ..Request::new("connection")
    };
    runtime.start().unwrap();
    assert!(
        runtime
            .request(&connection("simulate = false"), true)
            .is_err()
    );
    runtime.stop(false).unwrap();
    runtime
        .request(&connection("simulate = false"), true)
        .unwrap();
    runtime.publish();
    assert!(!runtime.shared.status_snapshot().simulated);
    assert!(!runtime.screen_control);
    // No tick on the real transport: this test never opens a serial device.
    runtime.request(&connection(""), true).unwrap();
    assert!(!runtime.cfg.simulate);
    runtime
        .request(&connection("simulate = true"), true)
        .unwrap();
    for _ in 0..40 {
        runtime.tick().unwrap();
    }
    runtime.publish();
    assert!(runtime.shared.status_snapshot().simulated);
    assert!(runtime.screen_control);
    assert!(runtime.start().is_err());
}

#[test]
fn save_as_changes_the_active_path_only_after_success() {
    let mut runtime = screen_runtime();
    let directory = std::env::temp_dir().join(format!(
        "catchrobo-save-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("machine.toml");
    runtime
        .request(
            &Request {
                text: Some(path.display().to_string()),
                ..Request::new("save")
            },
            true,
        )
        .unwrap();
    assert_eq!(runtime.shared.config().profile_path, path);
    assert!(crate::transport::profile_store::load(&path).is_ok());
    assert!(
        runtime
            .request(
                &Request {
                    text: Some(directory.join("missing/machine.toml").display().to_string()),
                    ..Request::new("save")
                },
                true
            )
            .is_err()
    );
    assert_eq!(runtime.shared.config().profile_path, path);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn emergency_revokes_ai_and_requires_human_reset_without_resuming() {
    let mut runtime = screen_runtime();
    for board in ["pwm", "dc", "sts"] {
        runtime.test.peers.insert(board, Instant::now());
    }
    let token = runtime
        .request(&Request::new("claim"), false)
        .unwrap()
        .token
        .unwrap();
    runtime
        .request(
            &Request {
                token: Some(token.clone()),
                ..Request::new("run")
            },
            false,
        )
        .unwrap();
    runtime.tick().unwrap();
    runtime.request(&Request::new("estop"), false).unwrap();
    runtime.tick().unwrap();
    assert!(runtime.emergency);
    assert!(!runtime.authority.active());
    assert!(!runtime.drive.running());
    assert!(runtime.request(&Request::new("claim"), false).is_err());
    assert!(runtime.start().is_err());
    assert!(
        runtime
            .request(&Request::new("estop_reset"), false)
            .is_err()
    );
    assert!(
        runtime
            .machine
            .origin_states(runtime.telemetry.as_ref())
            .iter()
            .all(|o| o.captured)
    );
    let logs = runtime.shared.status_snapshot().logs;
    for line in [
        "TX STOP",
        "TX CAN 2 768 0100000000000000",
        "TX CAN 2 784 0103000000000000",
        "TX CAN 2 800 0103000000000000",
    ] {
        assert!(logs.iter().any(|logged| logged == line), "missing {line}");
    }
    runtime.request(&Request::new("estop_reset"), true).unwrap();
    runtime.tick().unwrap();
    assert!(!runtime.emergency);
    assert!(!runtime.drive.running());
    assert_eq!(runtime.telemetry.as_ref().unwrap().mode, RunMode::Stop);
    assert!(
        runtime
            .request(
                &Request {
                    token: Some(token),
                    ..Request::new("run")
                },
                false
            )
            .is_err()
    );
    runtime.start().unwrap();
}

#[test]
fn output_cut_and_generic_fault_preserve_origins() {
    let mut runtime = screen_runtime();
    runtime.request(&Request::new("cut"), true).unwrap();
    assert!(
        runtime
            .machine
            .origin_states(runtime.telemetry.as_ref())
            .iter()
            .all(|o| o.captured)
    );
    runtime.fault("test disconnect".into());
    assert!(
        runtime
            .machine
            .origin_states(runtime.telemetry.as_ref())
            .iter()
            .all(|o| o.captured)
    );
}

fn select_test(runtime: &mut Runtime, target: &str, kind: &str) {
    runtime
        .request(
            &Request {
                flag: Some(true),
                ..Request::new("test_mode")
            },
            true,
        )
        .unwrap();
    runtime
        .request(
            &Request {
                axis: Some(target.into()),
                text: Some(kind.into()),
                ..Request::new("test_select")
            },
            true,
        )
        .unwrap();
    runtime.tick().unwrap();
    runtime.tick().unwrap();
}
#[test]
fn individual_test_excludes_driving_ai_and_apply_until_output_is_stopped() {
    let mut runtime = screen_runtime();
    runtime.start().unwrap();
    runtime.tick().unwrap();
    select_test(&mut runtime, "cctl:0", "velocity");
    assert!(!runtime.drive.running());
    assert!(runtime.start().is_err());
    assert!(runtime.request(&Request::new("claim"), false).is_err());
    runtime
        .request(
            &Request {
                value: Some(0.04),
                ..Request::new("test_output")
            },
            true,
        )
        .unwrap();
    runtime.tick().unwrap();
    assert!(runtime.test.active);
    assert_eq!(runtime.telemetry.as_ref().unwrap().enabled_slots, 1);
    let profile = toml::to_string(&runtime.cfg.machine).unwrap();
    assert!(
        runtime
            .request(
                &Request {
                    text: Some(profile),
                    ..Request::new("apply")
                },
                true
            )
            .is_err()
    );
    assert!(runtime.test.active);
    runtime
        .tick_at(Instant::now() + Duration::from_millis(151))
        .unwrap();
    assert!(!runtime.test.active);
    runtime
        .request(
            &Request {
                flag: Some(false),
                ..Request::new("test_mode")
            },
            true,
        )
        .unwrap();
    assert!(!runtime.drive.running());
    assert!(!runtime.test.enabled);
}
#[test]
fn position_test_holds_without_gui_refresh_and_target_change_cuts_output() {
    let mut runtime = screen_runtime();
    select_test(&mut runtime, "sts:1", "position");
    runtime
        .request(
            &Request {
                value: Some(2048.0),
                ..Request::new("test_output")
            },
            true,
        )
        .unwrap();
    runtime
        .tick_at(Instant::now() + Duration::from_millis(500))
        .unwrap();
    assert!(runtime.test.active);
    runtime.publish();
    assert!(runtime.shared.status_snapshot().outputs_active);
    runtime
        .request(
            &Request {
                axis: Some("pwm:0".into()),
                text: Some("position".into()),
                ..Request::new("test_select")
            },
            true,
        )
        .unwrap();
    assert!(!runtime.test.active);
    runtime.tick().unwrap();
    runtime.tick().unwrap();
    runtime
        .request(
            &Request {
                value: Some(1500.0),
                ..Request::new("test_output")
            },
            true,
        )
        .unwrap();
    runtime.tick().unwrap();
    assert!(runtime.test.active);
    runtime.request(&Request::new("estop"), true).unwrap();
    assert!(!runtime.test.active);
    assert!(runtime.test.enabled);
    runtime.request(&Request::new("estop_reset"), true).unwrap();
    assert!(!runtime.test.active);
}
#[test]
fn cctl_position_test_and_sensor_observation_do_not_reenable_other_axes() {
    let mut runtime = screen_runtime();
    assert!(runtime.test.peers.is_empty()); // 未設定・未選択の基板へは送信しない。
    assert_eq!(runtime.telemetry.as_ref().unwrap().mode, RunMode::Safe);
    select_test(&mut runtime, "cctl:0", "position");
    runtime
        .request(
            &Request {
                value: Some(469.9),
                ..Request::new("test_output")
            },
            true,
        )
        .unwrap();
    runtime.tick().unwrap();
    assert!(runtime.test.active);
    assert_eq!(runtime.telemetry.as_ref().unwrap().enabled_slots, 1);
    assert!((runtime.telemetry.as_ref().unwrap().slots[0].measured - 0.004).abs() < 0.001);
    runtime.request(&Request::new("stop"), true).unwrap();
    runtime.tick().unwrap();
    assert!(!runtime.test.active);
    assert_eq!(runtime.telemetry.as_ref().unwrap().mode, RunMode::Stop);
}
#[test]
fn dc_output_is_bounded_and_communication_loss_does_not_resume_tests() {
    let mut runtime = screen_runtime();
    select_test(&mut runtime, "dc:0", "duty");
    assert!(
        runtime
            .request(
                &Request {
                    value: Some(101.0),
                    ..Request::new("test_output")
                },
                true
            )
            .is_err()
    );
    runtime
        .request(
            &Request {
                value: Some(-100.0),
                ..Request::new("test_output")
            },
            true,
        )
        .unwrap();
    runtime.tick().unwrap();
    assert!(runtime.test.active);
    runtime.fault("disconnect".into());
    assert!(!runtime.test.active);
    runtime.tick().unwrap();
    assert!(!runtime.test.active);
}

#[test]
fn malformed_and_negative_peripheral_feedback_cannot_leave_a_test_active() {
    let mut runtime = screen_runtime();
    select_test(&mut runtime, "sts:1", "position");
    runtime
        .request(
            &Request {
                value: Some(2048.0),
                ..Request::new("test_output")
            },
            true,
        )
        .unwrap();
    runtime.observe_test_reply("CAN_RX bus=2 id=801 data=01あ00000000000");
    assert!(runtime.test.active);
    runtime.observe_test_reply("CAN_RX bus=2 id=802 data=0101080001010000");
    assert!(!runtime.test.active);
    select_test(&mut runtime, "pwm:0", "position");
    runtime
        .request(
            &Request {
                value: Some(1500.0),
                ..Request::new("test_output")
            },
            true,
        )
        .unwrap();
    runtime.test.started = Some(Instant::now() - Duration::from_secs(1));
    runtime.observe_test_reply("CAN_RX bus=2 id=769 data=0100000000000000");
    assert!(!runtime.test.active);
}

#[test]
fn drive_rejection_stops_without_invalidating_confirmed_settings() {
    let shared = Arc::new(Shared::new(BridgeConfig {
        serial_device: "unused".into(),
        baud_rate: 115200,
        rate_hz: 20.0,
        machine: MachineProfile::embedded().unwrap(),
        profile_path: "/dev/null".into(),
        simulate: true,
    }));
    let mut runtime = Runtime::new(shared);
    for _ in 0..40 {
        runtime.tick().unwrap();
    }
    assert!(runtime.settings.ready());
    runtime.link.fault("reject").unwrap();
    runtime.send("JOG 1 1").unwrap();
    runtime.tick().unwrap();
    assert!(!runtime.setup_error);
    assert!(runtime.settings.ready());
    assert!(!runtime.error.is_empty());
    runtime.tick().unwrap();
    assert_eq!(runtime.telemetry.as_ref().unwrap().mode, RunMode::Stop);
}

#[test]
fn communication_recovery_clears_only_transport_error_and_keeps_outputs_stopped() {
    let mut runtime = screen_runtime();
    runtime.start().unwrap();
    runtime.tick().unwrap();
    runtime.error = "機体側の異常".into();
    runtime.link.fault("disconnect").unwrap();
    let error = runtime.tick().unwrap_err();
    runtime.communication_failed(format!("{error:#}"));
    runtime.publish();
    assert!(runtime.communication_error.is_some());
    assert!(!runtime.shared.status_snapshot().connected);
    assert!(!runtime.drive.running());
    assert!(
        runtime
            .machine
            .origin_states(None)
            .iter()
            .all(|o| !o.captured)
    );
    runtime.link.fault("reconnect").unwrap();
    runtime.last_hello = Instant::now() - Duration::from_secs(2);
    runtime.tick().unwrap();
    assert!(runtime.communication_error.is_some());
    for _ in 0..45 {
        runtime.tick().unwrap();
    }
    runtime.publish();
    let status = runtime.shared.status_snapshot();
    assert!(status.connected && status.configured);
    assert!(runtime.communication_error.is_none());
    assert_eq!(status.error, "機体側の異常");
    assert!(!status.outputs_active && !status.running);
    assert!(status.origins.iter().all(|o| !o.captured));
    assert!(status.logs.iter().any(|l| l.starts_with("通信復旧:")));
}

#[test]
fn repeated_transport_failure_logs_once_and_retains_cause() {
    let mut runtime = screen_runtime();
    let error = anyhow::anyhow!("Permission denied").context("failed to open /dev/test");
    for _ in 0..3 {
        runtime.communication_failed(format!("{error:#}"));
    }
    runtime.publish();
    let status = runtime.shared.status_snapshot();
    assert_eq!(status.error, "failed to open /dev/test: Permission denied");
    assert_eq!(
        status
            .logs
            .iter()
            .filter(|l| l.starts_with("通信切断:"))
            .count(),
        1
    );
}

#[test]
fn recovery_during_emergency_does_not_release_the_latch() {
    let mut runtime = screen_runtime();
    runtime.emergency = true;
    runtime.communication_failed("USB disconnected".into());
    runtime.last_hello = Instant::now() - Duration::from_secs(2);
    for _ in 0..4 {
        runtime.tick().unwrap();
    }
    runtime.publish();
    let status = runtime.shared.status_snapshot();
    assert!(status.connected);
    assert!(runtime.communication_error.is_none());
    assert!(status.emergency);
    assert!(!status.outputs_active && !status.running);
    assert!(status.error.is_empty());
}

#[test]
fn individual_fault_blocks_held_requests_until_explicit_release() {
    let mut runtime = screen_runtime();
    select_test(&mut runtime, "cctl:0", "velocity");
    let output = Request {
        value: Some(0.01),
        ..Request::new("test_output")
    };
    runtime.request(&output, true).unwrap();
    runtime.tick().unwrap();
    assert!(runtime.test.active);
    runtime.fault("対象モータの応答切れ".into());
    for _ in 0..4 {
        assert!(runtime.request(&output, true).is_err());
        runtime.tick().unwrap();
        assert!(!runtime.test.active);
        assert_ne!(runtime.telemetry.as_ref().unwrap().mode, RunMode::Run);
    }
    runtime.request(&Request::new("test_off"), true).unwrap();
    runtime.request(&output, true).unwrap();
    runtime.tick().unwrap();
    assert!(runtime.test.active);
}

#[test]
fn explicit_position_update_preserves_output_and_origin_after_a_fault() {
    let mut runtime = screen_runtime();
    select_test(&mut runtime, "cctl:0", "position");
    let output = Request {
        value: Some(469.99),
        flag: Some(true),
        ..Request::new("test_output")
    };
    runtime.request(&output, true).unwrap();
    runtime.tick().unwrap();
    let started = runtime.test.started;
    runtime.request(&output, true).unwrap();
    assert_eq!(runtime.test.started, started);
    runtime.fault("test fault".into());
    assert!(runtime.test.restart_blocked);
    assert!(!runtime.test.active);
    assert!(
        runtime
            .machine
            .origin_states(runtime.telemetry.as_ref())
            .iter()
            .all(|origin| origin.captured)
    );
    runtime.request(&output, true).unwrap();
    assert!(runtime.test.active);
    assert!(!runtime.test.restart_blocked);
}

#[test]
fn leaving_test_stops_velocity_and_position_and_keeps_emergency_latched() {
    for kind in ["velocity", "position"] {
        let mut runtime = screen_runtime();
        select_test(&mut runtime, "cctl:0", kind);
        runtime
            .request(
                &Request {
                    value: Some(if kind == "position" { 469.99 } else { 0.01 }),
                    ..Request::new("test_output")
                },
                true,
            )
            .unwrap();
        runtime.tick().unwrap();
        assert!(runtime.test.active);
        runtime.emergency = true;
        runtime
            .request(
                &Request {
                    flag: Some(false),
                    ..Request::new("test_mode")
                },
                true,
            )
            .unwrap();
        runtime.tick().unwrap();
        assert!(!runtime.test.enabled && !runtime.test.active);
        assert!(runtime.test.selected.is_none());
        assert!(runtime.emergency);
        assert!(!runtime.drive.running());
        assert_ne!(runtime.telemetry.as_ref().unwrap().mode, RunMode::Run);
    }
}

#[test]
fn leaving_test_while_disconnected_still_ends_local_test_mode() {
    let mut runtime = screen_runtime();
    select_test(&mut runtime, "cctl:0", "position");
    runtime
        .request(
            &Request {
                value: Some(469.99),
                ..Request::new("test_output")
            },
            true,
        )
        .unwrap();
    runtime.link.fault("disconnect").unwrap();
    assert!(
        runtime
            .request(
                &Request {
                    flag: Some(false),
                    ..Request::new("test_mode")
                },
                true
            )
            .is_err()
    );
    assert!(!runtime.test.enabled && !runtime.test.active);
    assert!(runtime.test.selected.is_none());
}

#[test]
fn recovery_retries_failed_settings_without_restarting_output() {
    let mut runtime = screen_runtime();
    select_test(&mut runtime, "cctl:1", "velocity");
    runtime.settings = Settings::new(&runtime.cfg.machine);
    runtime.send("JOG 1 1").unwrap();
    runtime.receive().unwrap();
    assert!(runtime.setup_error);
    runtime.publish();
    assert!(!runtime.shared.status_snapshot().test_ready);
    runtime.request(&Request::new("recover"), true).unwrap();
    assert!(!runtime.settings.ready());
    for _ in 0..40 {
        runtime.tick().unwrap();
    }
    runtime.publish();
    let status = runtime.shared.status_snapshot();
    assert!(status.configured && status.test_ready);
    assert!(status.error.is_empty());
    assert!(!status.outputs_active && !status.running && !status.test_active);
    assert_ne!(runtime.telemetry.as_ref().unwrap().mode, RunMode::Run);
}

#[test]
fn recovery_preserves_motor_faults_and_emergency_latch() {
    let mut runtime = screen_runtime();
    runtime.telemetry.as_mut().unwrap().stale_slots = 1;
    runtime.telemetry.as_mut().unwrap().error_bits[0] = 0x80;
    runtime.request(&Request::new("recover"), true).unwrap();
    assert_eq!(runtime.telemetry.as_ref().unwrap().stale_slots, 1);
    assert_eq!(runtime.telemetry.as_ref().unwrap().error_bits[0], 0x80);
    assert!(runtime.ready().is_err());
    runtime.engage_emergency().unwrap();
    assert!(runtime.request(&Request::new("recover"), true).is_err());
    assert!(runtime.emergency);
}

#[test]
fn recovery_does_not_unlock_a_faulted_held_test_request() {
    let mut runtime = screen_runtime();
    select_test(&mut runtime, "cctl:1", "velocity");
    let output = Request {
        value: Some(1.0),
        ..Request::new("test_output")
    };
    runtime.request(&output, true).unwrap();
    runtime.tick().unwrap();
    assert!(runtime.request(&Request::new("recover"), true).is_err());
    runtime.fault("ERR code=CAN_TX".into());
    runtime.request(&Request::new("recover"), true).unwrap();
    for _ in 0..40 {
        runtime.tick().unwrap();
    }
    assert!(runtime.request(&output, true).is_err());
    runtime.request(&Request::new("test_off"), true).unwrap();
    runtime.request(&output, true).unwrap();
    assert!(runtime.error.is_empty());
}

#[test]
fn old_motor_layout_cannot_start_even_with_confirmed_settings() {
    let shared = Arc::new(Shared::new(BridgeConfig {
        serial_device: "unused".into(),
        baud_rate: 115200,
        rate_hz: 20.0,
        machine: MachineProfile::embedded().unwrap(),
        profile_path: "/dev/null".into(),
        simulate: true,
    }));
    let mut runtime = Runtime::new(shared);
    for _ in 0..40 {
        runtime.tick().unwrap();
    }
    assert!(runtime.settings.ready());
    runtime.device.as_mut().unwrap().motor_layout.clear();
    assert!(runtime.start().is_err());
    assert!(!runtime.drive.running());
}

#[test]
fn gain_apply_after_hold_preserves_coordinates_and_allows_run() {
    let mut runtime = screen_runtime();
    runtime.start().unwrap();
    runtime.tick().unwrap();
    runtime.stop(false).unwrap();
    let before = runtime.machine.origin_states(runtime.telemetry.as_ref());
    let mut profile = runtime.cfg.machine.clone();
    profile.parameters.insert("m3508_slot2_vel_kp".into(), 9.0);
    runtime.request(&Request {
        text: Some(toml::to_string(&profile).unwrap()),
        ..Request::new("apply")
    }, true).unwrap();
    for _ in 0..45 {
        runtime.tick().unwrap();
    }
    assert!(runtime.settings.ready());
    assert!(!runtime.drive.running());
    let after = runtime.machine.origin_states(runtime.telemetry.as_ref());
    for (old, new) in before.iter().zip(&after) {
        assert!(new.captured && !new.lost);
        assert!((old.position - new.position).abs() < 0.001);
    }
    runtime.start().unwrap();
    runtime.tick().unwrap();
    assert!(runtime.drive.running());
    assert!(runtime.machine.origin_states(runtime.telemetry.as_ref())
        .iter().all(|o| o.captured && !o.lost));
}

#[test]
fn output_stop_and_parameter_apply_keep_z_held_until_explicit_release() {
    let mut r = screen_runtime();
    r.start().unwrap();r.tick().unwrap();
    r.request(&Request::new("cut"), true).unwrap();r.tick().unwrap();
    assert_eq!(r.telemetry.as_ref().unwrap().held_slots, Some(4));
    let before = r.shared.status_snapshot().logs.len();
    let mut profile = r.cfg.machine.clone();
    profile.parameters.insert("m3508_slot2_vel_kp".into(), 9.0);
    r.request(&Request {text:Some(toml::to_string(&profile).unwrap()), ..Request::new("apply")}, true).unwrap();
    for _ in 0..45 {
        r.tick().unwrap();
        assert_eq!(r.telemetry.as_ref().unwrap().held_slots, Some(4));
    }
    assert!(r.settings.ready());
    assert!(!r.shared.status_snapshot().logs.iter().skip(before).any(|l| l=="TX STOP" || l=="TX SAFE"));
    r.configuration_failed("readback mismatch".into());r.tick().unwrap();
    assert_eq!(r.telemetry.as_ref().unwrap().held_slots, Some(4));
    r.request(&Request::new("recover"), true).unwrap();
    for _ in 0..45 {r.tick().unwrap();assert_eq!(r.telemetry.as_ref().unwrap().held_slots, Some(4));}
    r.engage_emergency().unwrap();r.tick().unwrap();
    assert_eq!(r.telemetry.as_ref().unwrap().held_slots, Some(0));
    r.request(&Request::new("safe"), true).unwrap();r.tick().unwrap();
    assert_eq!(r.telemetry.as_ref().unwrap().held_slots, Some(0));
}

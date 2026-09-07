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

fn screen_runtime() -> Runtime {
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
fn output_cut_preserves_origins_but_disconnect_invalidates_them() {
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
            .all(|o| !o.captured)
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
                value: Some(0.4),
                ..Request::new("test_output")
            },
            true,
        )
        .unwrap();
    runtime.tick().unwrap();
    assert!(runtime.test.active);
    assert_eq!(runtime.telemetry.as_ref().unwrap().enabled_slots, 1);
    assert!((runtime.telemetry.as_ref().unwrap().slots[0].measured - 0.4).abs() < 0.01);
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
fn explicit_position_update_preserves_output_and_can_restart_after_a_fault() {
    let mut runtime = screen_runtime();
    select_test(&mut runtime, "cctl:0", "position");
    let output = Request {
        value: Some(0.01),
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
                    value: Some(0.01),
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
                value: Some(0.01),
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

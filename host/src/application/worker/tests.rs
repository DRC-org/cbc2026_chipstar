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

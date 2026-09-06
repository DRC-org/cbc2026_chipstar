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

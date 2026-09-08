//! host実行ファイルと模擬機体を起動し、実際のUnixソケット越しに状態遷移を検証する。
use api::{Reply, Request};
use host::interface::control_api as api;
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
struct Host {
    child: Child,
    dir: PathBuf,
    socket: PathBuf,
}
impl Host {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "catchrobo-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&dir).unwrap();
        fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/config/rtheta.toml"),
            dir.join("machine.toml"),
        )
        .unwrap();
        let socket = dir.join("host.sock");
        let child = Command::new(env!("CARGO_BIN_EXE_host"))
            .args(["--headless", "--simulate", "--socket"])
            .arg(&socket)
            .arg("--machine-profile")
            .arg(dir.join("machine.toml"))
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let host = Self { child, dir, socket };
        let start = Instant::now();
        while !host.socket.exists() {
            assert!(start.elapsed() < Duration::from_secs(5));
            thread::sleep(Duration::from_millis(20));
        }
        host.wait(|s| s["configured"].as_bool() == Some(true));
        host
    }
    fn call(&self, request: Request) -> Reply {
        api::call(&self.socket, &request).unwrap()
    }
    fn status(&self) -> toml::Value {
        toml::from_str(&self.call(Request::new("status")).data).unwrap()
    }
    fn wait(&self, predicate: impl Fn(&toml::Value) -> bool) -> toml::Value {
        let start = Instant::now();
        loop {
            let s = self.status();
            if predicate(&s) {
                return s;
            }
            assert!(start.elapsed() < Duration::from_secs(6), "state: {s}");
            thread::sleep(Duration::from_millis(30));
        }
    }
    fn request(&self, action: &str, token: &str) -> Reply {
        self.call(Request {
            token: Some(token.into()),
            ..Request::new(action)
        })
    }
    fn claim(&self) -> String {
        self.call(Request::new("claim")).token.unwrap()
    }
    fn run(&self, token: &str) {
        assert!(self.request("run", token).ok);
        self.wait(|s| s["running"].as_bool() == Some(true));
    }
    fn origins(&self, token: &str) {
        for axis in ["r", "theta", "z"] {
            assert!(
                self.call(Request {
                    token: Some(token.into()),
                    axis: Some(axis.into()),
                    ..Request::new("origin")
                })
                .ok
            );
        }
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.dir);
    }
}
fn position(state: &toml::Value, axis: &str) -> f64 {
    state["origins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|origin| origin["name"].as_str() == Some(axis))
        .unwrap()["position"]
        .as_float()
        .unwrap()
}
fn all_origins(state: &toml::Value, captured: bool) -> bool {
    state["origins"]
        .as_array()
        .unwrap()
        .iter()
        .all(|origin| origin["captured"].as_bool() == Some(captured))
}
fn axis_speed(profile: &str, name: &str) -> f64 {
    let profile: toml::Value = toml::from_str(profile).unwrap();
    profile["axes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|axis| axis["name"].as_str() == Some(name))
        .unwrap()["speed_per_second"]
        .as_float()
        .unwrap()
}

#[test]
fn authority_and_expiring_input_never_latch_motion() {
    let host = Host::new();
    assert!(!host.call(Request::new("run")).ok);
    assert!(!host.status()["ai_active"].as_bool().unwrap());
    let token = host.claim();
    assert!(!host.call(Request::new("claim")).ok);
    assert!(!host.request("run", "wrong-token").ok);
    assert!(!host.request("run", &token).ok); // 原点未採用
    host.origins(&token);
    host.run(&token);
    assert!(
        host.call(Request {
            token: Some(token.clone()),
            axis: Some("r".into()),
            value: Some(-0.5),
            ..Request::new("input")
        })
        .ok
    );
    thread::sleep(Duration::from_millis(400));
    let stopped = position(&host.status(), "r");
    assert!(stopped > 0.0);
    thread::sleep(Duration::from_millis(200));
    assert!(
        (position(&host.status(), "r") - stopped).abs() < 0.01,
        "期限切れ後も進んでいる"
    );
    assert!(host.call(Request::new("stop")).ok); // tokenなしでも停止可能
    let stopped = host.wait(|s| s["running"].as_bool() == Some(false));
    assert_eq!(stopped["ai_active"].as_bool(), Some(true));
    assert!(host.request("release", &token).ok);
    let released = host.wait(|s| s["ai_active"].as_bool() == Some(false));
    assert_eq!(released["running"].as_bool(), Some(false));
}

#[test]
fn disconnect_reconnect_invalidates_origins_and_never_resumes() {
    let host = Host::new();
    let token = host.claim();
    host.origins(&token);
    host.run(&token);
    assert!(host.call(Request::new("stop")).ok);
    host.wait(|s| s["running"].as_bool() == Some(false));
    assert!(
        host.call(Request {
            token: Some(token.clone()),
            text: Some("disconnect".into()),
            ..Request::new("fault")
        })
        .ok
    );
    let disconnected = host.wait(|s| s["connected"].as_bool() == Some(false));
    assert_eq!(disconnected["running"].as_bool(), Some(false));
    assert_eq!(disconnected["outputs_active"].as_bool(), Some(false));
    assert!(all_origins(&disconnected, false));
    assert!(
        host.call(Request {
            token: Some(token.clone()),
            text: Some("reconnect".into()),
            ..Request::new("fault")
        })
        .ok
    );
    let state = host.wait(|s| s["configured"].as_bool() == Some(true));
    assert_eq!(state["running"].as_bool(), Some(false));
    assert_eq!(state["outputs_active"].as_bool(), Some(false));
    assert!(all_origins(&state, false));
    assert!(!host.request("run", &token).ok);
    host.origins(&token);
    host.run(&token);
}

#[test]
fn silent_link_loss_uses_board_watchdog_and_recovery_stays_stopped() {
    let host = Host::new();
    let token = host.claim();
    host.origins(&token);
    host.run(&token);
    assert!(
        host.call(Request {
            token: Some(token.clone()),
            text: Some("silence_rx".into()),
            ..Request::new("fault")
        })
        .ok
    );
    let disconnected = host.wait(|state| state["connected"].as_bool() == Some(false));
    assert_eq!(disconnected["running"].as_bool(), Some(false));
    assert!(all_origins(&disconnected, false));
    thread::sleep(Duration::from_millis(350));

    assert!(
        host.call(Request {
            token: Some(token.clone()),
            text: Some("resume_rx".into()),
            ..Request::new("fault")
        })
        .ok
    );
    let recovered = host.wait(|state| {
        state["configured"].as_bool() == Some(true)
            && state["connected"].as_bool() == Some(true)
            && state["outputs_active"].as_bool() == Some(false)
    });
    assert_eq!(recovered["running"].as_bool(), Some(false));
    assert!(all_origins(&recovered, false));
    assert!(!host.request("run", &token).ok);
}

#[test]
fn settings_are_temporary_until_explicitly_saved() {
    let host = Host::new();
    let token = host.claim();
    let before = fs::read_to_string(host.dir.join("machine.toml")).unwrap();
    let config = host.call(Request::new("config")).data;
    let mut edited: toml::Value = toml::from_str(&config).unwrap();
    let r = edited["axes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|axis| axis["name"].as_str() == Some("r"))
        .unwrap();
    r["speed_per_second"] = toml::Value::Float(8.0);
    assert!(
        host.call(Request {
            token: Some(token.clone()),
            text: Some(toml::to_string_pretty(&edited).unwrap()),
            ..Request::new("apply")
        })
        .ok
    );
    host.wait(|s| s["configured"].as_bool() == Some(true));
    assert_eq!(
        fs::read_to_string(host.dir.join("machine.toml")).unwrap(),
        before
    );
    assert!(host.request("save", &token).ok);
    let after = fs::read_to_string(host.dir.join("machine.toml")).unwrap();
    assert_eq!(axis_speed(&after, "r"), 8.0);
}

#[test]
fn rejected_drive_recovery_stays_stopped_and_requires_origin_again() {
    let host = Host::new();
    let token = host.claim();
    assert!(
        host.call(Request {
            token: Some(token.clone()),
            text: Some("reject".into()),
            ..Request::new("fault")
        })
        .ok
    );
    host.origins(&token);
    assert!(host.request("run", &token).ok); // 受付と基板の反映は別
    let rejected = host.wait(|s| {
        s["error"]
            .as_str()
            .is_some_and(|error| error.contains("SIM_REJECTED"))
    });
    assert_eq!(rejected["configured"].as_bool(), Some(true));
    assert_eq!(rejected["running"].as_bool(), Some(false));
    assert_eq!(rejected["outputs_active"].as_bool(), Some(false));
    assert!(all_origins(&rejected, false));
    assert!(!host.request("run", &token).ok);
    assert!(host.request("recover", &token).ok);
    let recovered = host.wait(|s| {
        s["configured"].as_bool() == Some(true) && s["error"].as_str().is_some_and(str::is_empty)
    });
    assert_eq!(recovered["running"].as_bool(), Some(false));
    assert_eq!(recovered["outputs_active"].as_bool(), Some(false));
    assert!(all_origins(&recovered, false));
    assert!(!host.request("run", &token).ok);
    host.origins(&token);
    host.run(&token);
}

#[test]
fn stop_hold_and_output_cut_have_distinct_safe_states() {
    let host = Host::new();
    let token = host.claim();
    host.origins(&token);
    host.run(&token);

    assert!(
        host.call(Request {
            token: Some(token.clone()),
            axis: Some("r".into()),
            value: Some(-0.7),
            ..Request::new("input")
        })
        .ok
    );
    host.wait(|state| {
        state["axes"][3]
            .as_float()
            .is_some_and(|value| (value + 0.7).abs() < 0.001)
    });
    assert!(host.call(Request::new("stop")).ok);
    let held = host.wait(|state| {
        state["running"].as_bool() == Some(false)
            && state["operating_state"].as_str() == Some("停止・保持")
    });
    assert_eq!(held["outputs_active"].as_bool(), Some(true));
    assert_eq!(held["ai_active"].as_bool(), Some(true));
    assert!(all_origins(&held, true));
    assert!(
        held["axes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|value| value.as_float() == Some(0.0))
    );
    let held_position = position(&held, "r");
    thread::sleep(Duration::from_millis(200));
    assert_eq!(host.status()["running"].as_bool(), Some(false));

    host.run(&token);
    thread::sleep(Duration::from_millis(200));
    assert!((position(&host.status(), "r") - held_position).abs() < 0.01);
    assert!(host.request("cut", &token).ok);
    let cut = host.wait(|state| {
        state["running"].as_bool() == Some(false)
            && state["outputs_active"].as_bool() == Some(false)
    });
    assert!(all_origins(&cut, true));
    thread::sleep(Duration::from_millis(200));
    assert_eq!(host.status()["running"].as_bool(), Some(false));
    host.run(&token);
}

#[test]
fn stop_during_run_ack_wait_cannot_leave_the_board_running() {
    let host = Host::new();
    let token = host.claim();
    host.origins(&token);

    assert!(
        host.call(Request {
            token: Some(token.clone()),
            text: Some("delay_run".into()),
            ..Request::new("fault")
        })
        .ok
    );
    assert!(host.request("run", &token).ok);
    assert!(host.call(Request::new("stop")).ok);
    let stopped = host.wait(|state| state["running"].as_bool() == Some(false));
    assert_eq!(stopped["outputs_active"].as_bool(), Some(false));
    thread::sleep(Duration::from_millis(200));
    let stable = host.status();
    assert_eq!(stable["running"].as_bool(), Some(false));
    assert_eq!(stable["outputs_active"].as_bool(), Some(false));
    assert_ne!(stable["board_mode"].as_str(), Some("RUN"));
}

#[test]
fn running_rejects_configuration_and_mode_switches_without_side_effects() {
    let host = Host::new();
    let token = host.claim();
    host.origins(&token);
    host.run(&token);
    let profile_before = host.call(Request::new("config")).data;
    let file_before = fs::read_to_string(host.dir.join("machine.toml")).unwrap();
    let requests = [
        Request {
            token: Some(token.clone()),
            text: Some(profile_before.clone()),
            ..Request::new("apply")
        },
        Request {
            token: Some(token.clone()),
            ..Request::new("save")
        },
        Request {
            token: Some(token.clone()),
            axis: Some("r".into()),
            ..Request::new("origin")
        },
        Request {
            token: Some(token.clone()),
            flag: Some(true),
            ..Request::new("adjustment")
        },
        Request {
            token: Some(token.clone()),
            text: Some("serial_device = \"unused\"\nbaud_rate = 115200\nsimulate = true\n".into()),
            ..Request::new("connection")
        },
        Request {
            token: Some(token.clone()),
            axis: Some("r".into()),
            ..Request::new("reinit")
        },
        Request {
            token: Some(token.clone()),
            flag: Some(false),
            ..Request::new("manual_control")
        },
    ];
    for request in requests {
        let action = request.action.clone();
        let reply = host.call(request);
        assert!(!reply.ok, "{action}が運転中に受理されました");
        let state = host.status();
        assert_eq!(state["running"].as_bool(), Some(true), "action={action}");
        assert_eq!(state["configured"].as_bool(), Some(true), "action={action}");
        assert!(all_origins(&state, true), "action={action}");
    }
    assert_eq!(host.call(Request::new("config")).data, profile_before);
    assert_eq!(
        fs::read_to_string(host.dir.join("machine.toml")).unwrap(),
        file_before
    );
}

#[test]
fn telemetry_faults_stop_outputs_invalidate_origins_and_block_restart() {
    for (fault, expected_error) in [
        ("mode_stop", "運転状態または原点"),
        ("disable_slot0", "運転状態または原点"),
        ("stale_slot0", "運転状態または原点"),
        ("error_slot0", "モータ応答・異常状態"),
        ("restart", "基板の再起動"),
        ("jump_slot0", "運転状態または原点"),
    ] {
        let host = Host::new();
        let token = host.claim();
        host.origins(&token);
        host.run(&token);
        assert!(
            host.call(Request {
                token: Some(token.clone()),
                text: Some(fault.into()),
                ..Request::new("fault")
            })
            .ok,
            "fault={fault}"
        );
        let stopped = host.wait(|state| {
            !state["error"].as_str().unwrap_or_default().is_empty()
                && state["running"].as_bool() == Some(false)
                && state["outputs_active"].as_bool() == Some(false)
        });
        assert!(
            stopped["error"].as_str().unwrap().contains(expected_error),
            "fault={fault}, state={stopped}"
        );
        assert_eq!(
            stopped["outputs_active"].as_bool(),
            Some(false),
            "fault={fault}"
        );
        assert!(all_origins(&stopped, false), "fault={fault}");
        assert!(!host.request("run", &token).ok, "fault={fault}");
    }
}

#[test]
fn api_emergency_revokes_the_token_and_cannot_be_reset_remotely() {
    let host = Host::new();
    let token = host.claim();
    host.origins(&token);
    host.run(&token);
    assert!(host.call(Request::new("estop")).ok);
    let stopped = host.wait(|s| {
        s["emergency"].as_bool() == Some(true) && s["outputs_active"].as_bool() == Some(false)
    });
    assert_eq!(stopped["ai_active"].as_bool(), Some(false));
    assert!(all_origins(&stopped, true));
    assert!(!host.call(Request::new("claim")).ok);
    assert!(!host.request("run", &token).ok);
    assert!(!host.request("estop_reset", &token).ok);
    assert!(host.call(Request::new("stop")).ok);
    assert_eq!(host.status()["emergency"].as_bool(), Some(true));
}

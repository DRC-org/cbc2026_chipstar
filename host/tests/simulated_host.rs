//! 実行ファイルを起動し、実際のソケット経由で停止復帰と設定保存を検証する。
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
fn position(s: &toml::Value) -> f64 {
    s["origins"][0]["position"].as_float().unwrap()
}

#[test]
fn operator_ownership_expiring_input_and_disconnect_do_not_resume() {
    let host = Host::new();
    assert!(!host.call(Request::new("run")).ok);
    assert!(!host.status()["ai_active"].as_bool().unwrap());
    let token = host.call(Request::new("claim")).token.unwrap();
    assert!(!host.call(Request::new("claim")).ok);
    assert!(!host.request("run", "wrong-token").ok);
    assert!(!host.request("run", &token).ok); // 原点未採用
    host.origins(&token);
    assert!(host.request("run", &token).ok);
    host.wait(|s| s["running"].as_bool() == Some(true));
    assert!(
        host.call(Request {
            token: Some(token.clone()),
            axis: Some("r".into()),
            value: Some(0.5),
            ..Request::new("input")
        })
        .ok
    );
    thread::sleep(Duration::from_millis(400));
    let stopped = position(&host.status());
    assert!(stopped > 0.0);
    thread::sleep(Duration::from_millis(200));
    assert!(
        (position(&host.status()) - stopped).abs() < 0.01,
        "期限切れ後も進んでいる"
    );
    assert!(host.call(Request::new("stop")).ok); // tokenなしでも停止可能
    host.wait(|s| s["running"].as_bool() == Some(false));
    assert!(
        host.call(Request {
            token: Some(token.clone()),
            text: Some("disconnect".into()),
            ..Request::new("fault")
        })
        .ok
    );
    host.wait(|s| s["connected"].as_bool() == Some(false));
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
    assert!(!host.request("run", &token).ok);
    host.origins(&token);
    assert!(host.request("run", &token).ok);
    host.wait(|s| s["running"].as_bool() == Some(true));
    assert!(host.request("release", &token).ok);
    let state = host.wait(|s| s["ai_active"].as_bool() == Some(false));
    assert_eq!(state["running"].as_bool(), Some(false));
}

#[test]
fn settings_are_temporary_until_saved_and_rejected_changes_block_run() {
    let host = Host::new();
    let token = host.call(Request::new("claim")).token.unwrap();
    let before = fs::read_to_string(host.dir.join("machine.toml")).unwrap();
    let config = host.call(Request::new("config")).data;
    let edited = config.replace("speed_per_second = 10.0", "speed_per_second = 8.0");
    assert!(
        host.call(Request {
            token: Some(token.clone()),
            text: Some(edited),
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
    assert!(after.contains("speed_per_second = 8.0"));
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
    host.wait(|s| s["configuration"].as_str() == Some("反映失敗"));
    assert_eq!(host.status()["running"].as_bool(), Some(false));
    assert!(!host.request("run", &token).ok);
}

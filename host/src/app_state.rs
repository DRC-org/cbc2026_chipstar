//! GUI・ローカルAPI・ワーカーが共有する操作受付とスナップショット。
use crate::{
    control_api::{Reply, Request},
    machine::{MachineProfile, OriginState},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub serial_device: String,
    pub baud_rate: u32,
}

#[derive(Clone)]
pub struct BridgeConfig {
    pub serial_device: String,
    pub baud_rate: u32,
    pub rate_hz: f64,
    pub machine: MachineProfile,
    pub profile_path: PathBuf,
    pub simulate: bool,
}

#[derive(Clone, Default, Serialize)]
pub struct Status {
    pub simulated: bool,
    pub connected: bool,
    pub configured: bool,
    pub ai_active: bool,
    pub running: bool,
    pub board_mode: String,
    pub reason: String,
    pub error: String,
    pub slow: bool,
    pub origin_adjustment: bool,
    pub gamepad: String,
    pub axes: [f32; 6],
    pub origins: Vec<OriginState>,
    pub telemetry_age_ms: u64,
    pub tx_count: u64,
    pub parameters_confirmed: usize,
    pub parameters_expected: usize,
    pub configuration: String,
    pub peripherals: std::collections::BTreeMap<String, String>,
    pub saved: bool,
    pub logs: VecDeque<String>,
}

pub struct Pending {
    pub request: Request,
    pub manual: bool,
    pub deadline: std::time::Instant,
    pub reply: mpsc::Sender<Reply>,
}

pub struct Shared {
    config: Mutex<BridgeConfig>,
    status: Mutex<Status>,
    pending: Mutex<VecDeque<Pending>>,
    alive: AtomicBool,
}
impl Shared {
    pub fn new(config: BridgeConfig) -> Self {
        Self {
            status: Mutex::new(Status {
                simulated: config.simulate,
                saved: true,
                reason: "接続待ち".into(),
                ..Default::default()
            }),
            config: Mutex::new(config),
            pending: Mutex::new(VecDeque::new()),
            alive: AtomicBool::new(true),
        }
    }
    pub fn config(&self) -> BridgeConfig {
        self.config.lock().unwrap().clone()
    }
    pub fn set_config(&self, config: BridgeConfig) {
        *self.config.lock().unwrap() = config;
    }
    pub fn status_snapshot(&self) -> Status {
        self.status.lock().unwrap().clone()
    }
    pub fn update_status(&self, f: impl FnOnce(&mut Status)) {
        f(&mut self.status.lock().unwrap());
    }
    pub fn log(&self, line: String) {
        self.update_status(|s| {
            if s.logs.len() >= 300 {
                s.logs.pop_front();
            }
            s.logs.push_back(line);
        });
    }
    pub fn is_running(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }
    pub fn request_stop(&self) {
        self.alive.store(false, Ordering::Release);
    }
    pub fn take_requests(&self) -> Vec<Pending> {
        self.pending.lock().unwrap().drain(..).collect()
    }
    pub fn submit(&self, request: Request, manual: bool) -> Reply {
        match request.action.as_str() {
            "status" => {
                return Reply::data(toml::to_string(&self.status_snapshot()).unwrap_or_default());
            }
            "config" => {
                return Reply::data(
                    toml::to_string_pretty(&self.config().machine).unwrap_or_default(),
                );
            }
            _ => {}
        }
        let (tx, rx) = mpsc::channel();
        {
            let mut queue = self.pending.lock().unwrap();
            if request.action == "stop" {
                for item in queue.drain(..) {
                    let _ = item.reply.send(Reply::error("STOPにより取消"));
                }
                queue.push_front(Pending {
                    request,
                    manual,
                    deadline: std::time::Instant::now() + Duration::from_secs(1),
                    reply: tx,
                });
            } else {
                if queue.len() >= 32 {
                    return Reply::error("操作受付が混雑しています");
                }
                queue.push_back(Pending {
                    request,
                    manual,
                    deadline: std::time::Instant::now() + Duration::from_secs(1),
                    reply: tx,
                });
            }
        }
        // 実行期限を超えた要求はワーカーが取り消す。
        rx.recv_timeout(Duration::from_secs(2))
            .unwrap_or_else(|_| Reply::error("応答期限切れ。状態を確認してください"))
    }
}

//! GUI・ローカルAPI・ワーカーが共有する操作受付とスナップショット。
use crate::{
    application::command::{Reply, Request},
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
    #[serde(default)]
    pub simulate: Option<bool>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Court {
    Red,
    Blue,
}
impl Court {
    pub fn homing_theta(self) -> f32 {
        match self {
            Self::Red => -90.0,
            Self::Blue => 90.0,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Red => "赤コート",
            Self::Blue => "青コート",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreparationStep {
    #[default]
    Court,
    Connection,
    Home,
    Position,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommunicationHealth {
    Healthy,
    Warning,
    Fault,
    #[default]
    Unknown,
}

#[derive(Clone, Default, Serialize)]
pub struct CanBusStatus {
    pub bus: u8,
    pub label: String,
    pub health: CommunicationHealth,
    pub detail: String,
    pub age_ms: Option<u64>,
    pub started: Option<bool>,
    pub bus_off: Option<bool>,
    pub lec: Option<u8>,
    pub tec: Option<u8>,
    pub rec: Option<u8>,
    pub cel: Option<u8>,
    pub tx_failed: Option<u32>,
}

#[derive(Clone, Default, Serialize)]
pub struct CanDeviceStatus {
    pub name: String,
    pub bus: u8,
    pub address: String,
    pub health: CommunicationHealth,
    pub detail: String,
    pub age_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreparationPhase {
    #[default]
    Setting,
    Waiting,
    Active,
    Recovery,
}
impl PreparationPhase {
    pub fn locked(self) -> bool {
        matches!(self, Self::Waiting | Self::Recovery)
    }
}

#[derive(Clone, Default, Serialize)]
pub struct Status {
    pub court: Option<Court>,
    pub preparation: PreparationPhase,
    pub preparation_blocker: String,
    pub pad_guide: bool,
    pub preparation_step: PreparationStep,
    pub guide_release: bool,
    pub operation_sound_available: bool,
    pub sequence: super::sequence::Status,
    pub sequence_saved: bool,
    pub homing: Option<String>,
    pub homing_ready: bool,
    pub homing_confirmation: Option<f32>,
    pub ee_targets: std::collections::BTreeMap<String, f32>,
    pub bonus: BonusStatus,
    pub sts: crate::application::sts::Status,
    pub test_mode: bool,
    pub test_target: String,
    pub test_active: bool,
    pub test_ready: bool,
    pub test_kind: String,
    pub emergency: bool,
    pub outputs_active: bool,
    pub operating_state: String,
    pub simulated: bool,
    pub screen_control: bool,
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
    pub gamepad_input: Option<crate::input::ControllerState>,
    pub axes: [f32; 6],
    pub origins: Vec<OriginState>,
    pub telemetry_age_ms: u64,
    pub tx_count: u64,
    pub parameters_confirmed: usize,
    pub parameters_expected: usize,
    pub configuration: String,
    pub peripherals: std::collections::BTreeMap<String, String>,
    pub can_buses: Vec<CanBusStatus>,
    pub can_devices: Vec<CanDeviceStatus>,
    pub saved: bool,
    pub logs: VecDeque<String>,
}

#[derive(Clone, Default, Serialize)]
pub struct BonusStatus {
    pub configured: bool,
    pub semi_auto: bool,
    pub handoff_captured: bool,
    pub encoder_count: Option<i32>,
    pub position_counts: Option<i32>,
    pub handoff_limit_configured: bool,
    pub handoff_limit: Option<bool>,
    pub selected_box: usize,
    pub box_names: Vec<String>,
    pub loaded: u8,
    pub capacity: u8,
    pub active: bool,
    pub phase: String,
}

pub struct Pending {
    pub request: Request,
    pub manual: bool,
    pub deadline: std::time::Instant,
    pub reply: mpsc::Sender<Reply>,
}

pub struct Shared {
    pub response_history: Mutex<super::response_history::History>,
    config: Mutex<BridgeConfig>,
    sequence_config: Mutex<super::sequence::Config>,
    status: Mutex<Status>,
    pending: Mutex<VecDeque<Pending>>,
    alive: AtomicBool,
    emergency_pending: AtomicBool,
}
impl Shared {
    pub fn new(config: BridgeConfig) -> Self {
        let path = super::sequence::config_path(&config.profile_path);
        let mut sequence_message = String::new();
        let sequence_config = if path.exists() {
            super::sequence::load(&path).unwrap_or_else(|error| {
                sequence_message = format!("シーケンス設定の読込み失敗: {error}");
                super::sequence::Config::from_machine(&config.machine)
            })
        } else {
            super::sequence::Config::from_machine(&config.machine)
        };
        Self {
            sequence_config: Mutex::new(sequence_config),
            response_history: Mutex::new(super::response_history::History::default()),
            status: Mutex::new(Status {
                sequence_saved: path.exists() && sequence_message.is_empty(),
                sequence: super::sequence::Status {
                    message: sequence_message,
                    ..Default::default()
                },
                simulated: config.simulate,
                saved: true,
                reason: "接続待ち".into(),
                ..Default::default()
            }),
            config: Mutex::new(config),
            pending: Mutex::new(VecDeque::new()),
            alive: AtomicBool::new(true),
            emergency_pending: AtomicBool::new(false),
        }
    }
    pub fn sequence_config(&self) -> super::sequence::Config {
        self.sequence_config.lock().unwrap().clone()
    }
    pub fn set_sequence_config(&self, config: super::sequence::Config) {
        *self.sequence_config.lock().unwrap() = config;
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
    pub fn take_emergency(&self) -> bool {
        self.emergency_pending.swap(false, Ordering::AcqRel)
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
        if request.action == "estop" {
            self.emergency_pending.store(true, Ordering::Release);
        }
        let (tx, rx) = mpsc::channel();
        {
            let mut queue = self.pending.lock().unwrap();
            if matches!(request.action.as_str(), "stop" | "estop") {
                for item in queue.drain(..) {
                    let _ = item.reply.send(Reply::error("停止要求により取消"));
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn emergency_cancels_queued_motion_and_signals_before_worker_execution() {
        let shared = std::sync::Arc::new(Shared::new(BridgeConfig {
            serial_device: "unused".into(),
            baud_rate: 115200,
            rate_hz: 20.0,
            machine: MachineProfile::embedded().unwrap(),
            profile_path: "/dev/null".into(),
            simulate: true,
        }));
        let (tx, rx) = mpsc::channel();
        shared.pending.lock().unwrap().push_back(Pending {
            request: Request::new("run"),
            manual: true,
            deadline: std::time::Instant::now() + Duration::from_secs(1),
            reply: tx,
        });
        let caller = shared.clone();
        let thread = std::thread::spawn(move || caller.submit(Request::new("estop"), false));
        assert!(!rx.recv_timeout(Duration::from_secs(1)).unwrap().ok);
        assert!(shared.take_emergency());
        let mut pending = shared.take_requests();
        assert_eq!(pending.len(), 1);
        let stop = pending.pop().unwrap();
        assert_eq!(stop.request.action, "estop");
        stop.reply.send(Reply::accepted()).unwrap();
        assert!(thread.join().unwrap().ok);
    }
}

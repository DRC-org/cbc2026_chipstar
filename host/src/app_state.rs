//! GUI（メインスレッド）とブリッジ処理（ワーカースレッド）が共有する状態。
//!
//! - 設定は `Mutex<BridgeConfig>`。変更時に `config_gen` を進め、ワーカーが再接続する。
//! - 実行状況は `Mutex<Status>` にスナップショットとして書き込む。

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// シリアルブリッジの設定。
#[derive(Clone)]
pub struct BridgeConfig {
    pub serial_device: String,
    pub baud_rate: u32,
    pub rate_hz: f64,
}

/// GUI 表示用の実行状況スナップショット。
#[derive(Clone, Default)]
pub struct Status {
    pub gamepad_connected: bool,
    pub gamepad_name: Option<String>,
    pub serial_connected: bool,
    pub last_error: Option<String>,
    pub tx_count: u64,
    pub last_line: String,
    pub axes: [f32; 6],
    pub buttons: [u8; 17],
}

/// スレッド間共有ハンドル。`Arc<Shared>` で持ち回る。
pub struct Shared {
    config: Mutex<BridgeConfig>,
    config_gen: AtomicU64,
    sending_enabled: AtomicBool,
    running: AtomicBool,
    status: Mutex<Status>,
}

impl Shared {
    pub fn new(config: BridgeConfig) -> Self {
        Self {
            config: Mutex::new(config),
            config_gen: AtomicU64::new(0),
            sending_enabled: AtomicBool::new(true),
            running: AtomicBool::new(true),
            status: Mutex::new(Status::default()),
        }
    }

    pub fn config(&self) -> BridgeConfig {
        self.config.lock().unwrap().clone()
    }

    /// 設定を差し替え、世代番号を進める（ワーカーが再接続を検知する）。
    pub fn set_config(&self, config: BridgeConfig) {
        *self.config.lock().unwrap() = config;
        self.config_gen.fetch_add(1, Ordering::Release);
    }

    pub fn config_generation(&self) -> u64 {
        self.config_gen.load(Ordering::Acquire)
    }

    pub fn sending_enabled(&self) -> bool {
        self.sending_enabled.load(Ordering::Relaxed)
    }

    pub fn set_sending_enabled(&self, enabled: bool) {
        self.sending_enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn request_stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }

    pub fn status_snapshot(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    /// ワーカーから状態を更新する。
    pub fn update_status(&self, f: impl FnOnce(&mut Status)) {
        f(&mut self.status.lock().unwrap());
    }
}

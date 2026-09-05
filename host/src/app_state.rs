//! GUI（メインスレッド）とブリッジ処理（ワーカースレッド）が共有する状態。
//!
//! - 設定は `Mutex<BridgeConfig>`。変更時に `config_gen` を進め、ワーカーが再接続する。
//! - 実行状況は `Mutex<Status>` にスナップショットとして書き込む。

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::device::DeviceInfo;
use crate::frame::Command;
use crate::machine::MachineProfile;
use crate::svmd;
use crate::telemetry::Telemetry;

/// シリアルブリッジの設定。
#[derive(Clone)]
pub struct BridgeConfig {
    pub serial_device: String,
    pub baud_rate: u32,
    pub rate_hz: f64,
    pub machine: MachineProfile,
}

/// GUI 表示用の実行状況スナップショット。
#[derive(Clone, Default)]
pub struct Status {
    pub dcmd: Option<crate::dcmd::Status>,
    pub dcmd_encoder: Option<crate::dcmd::EncoderStatus>,
    pub gamepad_connected: bool,
    pub gamepad_name: Option<String>,
    pub serial_connected: bool,
    pub serial_svmd_connected: bool,
    pub last_error: Option<String>,
    pub tx_count: u64,
    pub last_line: String,
    pub axes: [f32; 6],
    pub buttons: [u8; 17],
    /// cctl から受け取った最新のテレメトリ。未受信なら None。
    pub telemetry: Option<Telemetry>,
    pub telemetry_count: u64,
    pub device: Option<DeviceInfo>,
    pub serial_svmd_device: Option<DeviceInfo>,
}

/// スレッド間共有ハンドル。`Arc<Shared>` で持ち回る。
pub struct Shared {
    pub tests: crate::fw_test_gui::Control,
    config: Mutex<BridgeConfig>,
    config_gen: AtomicU64,
    sending_enabled: AtomicBool,
    running: AtomicBool,
    status: Mutex<Status>,
    /// GUI が積み、ワーカーが送る指令行。
    commands: Mutex<VecDeque<String>>,
    serial_svmd_commands: Mutex<VecDeque<String>>,
}

impl Shared {
    pub fn new(config: BridgeConfig) -> Self {
        Self {
            tests: crate::fw_test_gui::Control::default(),
            config: Mutex::new(config),
            config_gen: AtomicU64::new(0),
            sending_enabled: AtomicBool::new(true),
            running: AtomicBool::new(true),
            status: Mutex::new(Status::default()),
            commands: Mutex::new(VecDeque::new()),
            serial_svmd_commands: Mutex::new(VecDeque::new()),
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
        self.sending_enabled
            .store(enabled && !self.tests.enabled(), Ordering::Relaxed);
    }

    pub fn start_test(&self, config: crate::fw_test_gui::Config) {
        self.set_sending_enabled(false);
        self.tests.start(config);
        self.take_commands();
        self.take_serial_svmd_commands();
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

    /// 指令を送信待ちに積む。送信停止中でも送るので、STOP は必ず届く。
    pub fn queue_command(&self, command: Command) -> bool {
        if self.tests.enabled() {
            if matches!(command, Command::Stop | Command::Safe) {
                self.tests.command("stop".into());
                return true;
            }
            self.update_status(|s| s.last_error = Some("基板テスト中は通常操作できません".into()));
            return false;
        }
        if command == Command::Run {
            let status = self.status.lock().unwrap();
            if !self.config().machine.dc_motors.is_empty() && status.dcmd.is_none() {
                drop(status);
                self.update_status(|s| {
                    s.last_error = Some("DCMDの状態応答を待っています".to_owned())
                });
                return false;
            }
            let Some(device) = &status.device else {
                drop(status);
                self.update_status(|status| {
                    status.last_error = Some("FWの能力確認が完了していません".to_owned());
                });
                return false;
            };
            if device.protocol != 1 || device.board != "cctl" || device.slots < 3 {
                drop(status);
                self.update_status(|status| {
                    status.last_error = Some("cctlの能力またはバージョンが不一致です".to_owned());
                });
                return false;
            }
            if self.config.lock().unwrap().machine.requires_can_bus_2()
                && !device.can_buses.contains(&2)
            {
                drop(status);
                self.update_status(|status| {
                    status.last_error = Some("FWに必要なCAN bus 2がありません".to_owned());
                });
                return false;
            }
            if self.config.lock().unwrap().machine.requires_serial_svmd()
                && !status.serial_svmd_device.as_ref().is_some_and(|device| {
                    device.protocol == 1 && device.board == "serial_svmd" && device.slots >= 16
                })
            {
                drop(status);
                self.update_status(|status| {
                    status.last_error = Some("serial_svmdの能力確認が完了していません".to_owned());
                });
                return false;
            }
        }
        self.queue_line(command.to_line());
        let config = self.config();
        if !config.machine.dc_motors.is_empty() {
            match command {
                Command::Run => {
                    self.queue_line(crate::dcmd::line(0, 0, 0));
                    let mut mask = 0;
                    for motor in &config.machine.dc_motors {
                        self.queue_line(crate::dcmd::line(4, motor.channel, 0));
                        mask |= 1 << motor.channel;
                    }
                    self.queue_line(crate::dcmd::line(2, mask, 0));
                }
                Command::Safe | Command::Stop => self.queue_line(crate::dcmd::line(3, 0, 0)),
                _ => {}
            }
        }
        if matches!(command, Command::Stop | Command::Safe)
            && !self.config.lock().unwrap().machine.pwm_servos.is_empty()
        {
            self.queue_line(svmd::Command::Stop.to_cctl_line());
        }
        if command == Command::Run {
            for servo in &self.config().machine.pwm_servos {
                self.queue_line(
                    svmd::Command::Enable {
                        channel: servo.channel,
                        enabled: servo.enabled,
                    }
                    .to_cctl_line(),
                );
            }
        }
        if self.config.lock().unwrap().machine.requires_serial_svmd()
            && matches!(command, Command::Stop | Command::Run | Command::Safe)
        {
            self.serial_svmd_commands
                .lock()
                .unwrap()
                .push_back(command.to_line());
        }
        true
    }

    /// cctl へそのまま送る行を積む。コマンドラインからの入力に使う。
    pub fn queue_line(&self, line: String) {
        if self.tests.enabled() {
            return;
        }
        self.commands.lock().unwrap().push_back(line);
    }

    /// 送信待ちの行を全て取り出す。
    pub fn take_commands(&self) -> Vec<String> {
        self.commands.lock().unwrap().drain(..).collect()
    }

    pub fn take_serial_svmd_commands(&self) -> Vec<String> {
        self.serial_svmd_commands
            .lock()
            .unwrap()
            .drain(..)
            .collect()
    }

    /// ワーカーから状態を更新する。
    pub fn update_status(&self, f: impl FnOnce(&mut Status)) {
        f(&mut self.status.lock().unwrap());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_blocks_normal_commands_and_does_not_resume_sending() {
        let shared = Shared::new(BridgeConfig {
            serial_device: "/dev/null".into(),
            baud_rate: 115200,
            rate_hz: 20.0,
            machine: pwm_profile(),
        });
        shared.queue_line("RUN".into());
        shared.start_test(crate::fw_test_gui::Config {
            board: crate::fw_test::Board::Dcmd,
            device: "/dev/null".into(),
            baud: 115200,
            seconds: 5,
        });
        assert!(shared.take_commands().is_empty());
        shared.queue_line("TARGET 0 1".into());
        assert!(shared.take_commands().is_empty());
        assert!(!shared.queue_command(Command::Run));
        assert!(shared.queue_command(Command::Stop));
        assert!(shared.take_commands().is_empty());
        shared.set_sending_enabled(true);
        assert!(!shared.sending_enabled());
        shared.tests.end();
        shared.tests.finish();
        assert!(!shared.sending_enabled());
    }

    fn pwm_profile() -> MachineProfile {
        MachineProfile::parse(
            r#"
protocol_version = 1

[[pwm_servos]]
name = "gripper"
channel = 0
speed_us_per_second = 0.0
minimum_us = 900
maximum_us = 2100
initial_us = 1500
enabled = true
"#,
        )
        .unwrap()
    }

    fn serial_profile() -> MachineProfile {
        MachineProfile::parse(
            r#"
protocol_version = 1

[serial_svmd]
device = "/dev/ttyUSB0"

[[serial_svmd.servos]]
name = "arm"
id = 1
speed_position_per_second = 0.0
minimum_position = 1000
maximum_position = 3000
initial_position = 2000
move_speed = 400
acceleration = 30
enabled = true
"#,
        )
        .unwrap()
    }

    #[test]
    fn stop_is_forwarded_to_cctl_and_svmd() {
        let shared = Shared::new(BridgeConfig {
            serial_device: "/dev/null".to_owned(),
            baud_rate: 115_200,
            rate_hz: 20.0,
            machine: pwm_profile(),
        });

        assert!(shared.queue_command(Command::Stop));
        assert_eq!(
            shared.take_commands(),
            vec!["STOP", "CAN 2 768 0100000000000000"]
        );
        assert!(shared.queue_command(Command::Safe));
        assert_eq!(
            shared.take_commands(),
            vec!["SAFE", "CAN 2 768 0100000000000000"]
        );
        shared.update_status(|status| {
            status.device = crate::device::parse_device_info(
                "DEVICE protocol=2 board=cctl slots=3 can=2 watchdog_ms=250",
            );
        });
        assert!(!shared.queue_command(Command::Run));
        assert!(shared.take_commands().is_empty());
        shared.update_status(|status| {
            status.device.as_mut().unwrap().protocol = 1;
        });
        assert!(shared.queue_command(Command::Run));
        assert_eq!(
            shared.take_commands(),
            vec!["RUN", "CAN 2 768 0102000100000000"]
        );
    }

    #[test]
    fn common_state_commands_are_forwarded_to_serial_svmd() {
        let shared = Shared::new(BridgeConfig {
            serial_device: "/dev/null".to_owned(),
            baud_rate: 115_200,
            rate_hz: 20.0,
            machine: serial_profile(),
        });

        assert!(shared.queue_command(Command::Safe));
        assert!(shared.queue_command(Command::Home { slots: 7 }));
        assert_eq!(shared.take_commands(), vec!["SAFE", "HOME 7"]);
        assert_eq!(shared.take_serial_svmd_commands(), vec!["SAFE"]);
    }
}

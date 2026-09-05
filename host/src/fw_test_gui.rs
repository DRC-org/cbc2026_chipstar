//! GUIのテスト操作キュー。シリアルとSessionは既存ワーカーだけが所有する。
use crate::{
    app_state::Shared,
    fw_test::{Board, Session},
    fw_test_transport,
    serial::SerialLink,
};
use eframe::egui;
use std::{
    collections::VecDeque,
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct Config {
    pub board: Board,
    pub device: String,
    pub baud: u32,
    pub seconds: u64,
}

#[derive(Clone, Default)]
pub struct Snapshot {
    pub inputs: Option<crate::inputs::InputState>,
    pub input_rx: Option<Instant>,
    pub outputs: Vec<u8>,
    pub ready: bool,
    pub switches: Vec<String>,
    pub watchdog: bool,
    pub error: Option<String>,
    pub logs: VecDeque<String>,
    pub last_rx: Option<Instant>,
}

#[derive(Default)]
struct State {
    active: bool,
    config: Option<Config>,
    commands: VecDeque<String>,
    snapshot: Snapshot,
}

#[derive(Default)]
pub struct Control(Mutex<State>);

impl Control {
    pub fn config(&self) -> Option<Config> {
        self.0.lock().unwrap().config.clone()
    }
    pub fn enabled(&self) -> bool {
        self.0.lock().unwrap().active
    }
    pub fn snapshot(&self) -> Snapshot {
        self.0.lock().unwrap().snapshot.clone()
    }
    pub fn start(&self, config: Config) {
        let mut state = self.0.lock().unwrap();
        if state.active {
            return;
        }
        *state = State {
            active: true,
            config: Some(config),
            ..Default::default()
        };
    }
    pub fn end(&self) {
        let mut state = self.0.lock().unwrap();
        state.config = None;
        state.commands.clear();
        state.snapshot.ready = false;
    }
    pub fn finish(&self) {
        let mut state = self.0.lock().unwrap();
        state.active = false;
        state.config = None;
        state.commands.clear();
    }
    pub fn command(&self, command: String) {
        let mut state = self.0.lock().unwrap();
        if command == "stop" {
            state.commands.clear();
            state.commands.push_back(command);
        } else if state.config.is_some()
            && state.snapshot.ready
            && !state.snapshot.watchdog
            && state.commands.len() < 16
        {
            state.commands.push_back(command);
        }
    }
    fn take(&self) -> Option<String> {
        self.0.lock().unwrap().commands.pop_front()
    }
    fn update(&self, f: impl FnOnce(&mut Snapshot)) {
        f(&mut self.0.lock().unwrap().snapshot);
    }
}

pub fn run(shared: &Shared, config: Config) {
    let mut link = SerialLink::new(config.device, config.baud);
    let mut session = Session::new(config.board, Duration::from_secs(config.seconds));
    let result = (|| -> anyhow::Result<()> {
        fw_test_transport::connect(&mut link, config.board)?;
        fw_test_transport::prepare(&mut link, config.board, &mut session)?;
        shared.tests.update(|s| s.ready = true);
        while shared.is_running() && shared.tests.config().is_some() {
            let now = Instant::now();
            if let Some(command) = shared.tests.take() {
                match session.command(&command, now) {
                    Ok(lines) => {
                        fw_test_transport::send(&mut link, lines)?;
                        shared.tests.update(|s| s.error = None);
                    }
                    Err(error) => shared.tests.update(|s| s.error = Some(error.to_string())),
                }
            }
            fw_test_transport::send(&mut link, session.tick(now))?;
            shared.tests.update(|s| {
                s.switches = session.switches();
                s.outputs = session.active_outputs();
                s.watchdog = session.watchdog_running();
            });
            for line in link.read_lines() {
                if let Some(state) = crate::inputs::parse(&line, config.board) {
                    if state.trip && !session.active_outputs().is_empty() {
                        shared.tests.command("stop".into());
                        fw_test_transport::send(&mut link, session.stop())?;
                    }
                    shared.tests.update(|s| {
                        s.inputs = Some(state);
                        s.input_rx = Some(Instant::now());
                    });
                }
                if line.starts_with("ERR ")
                    || line.starts_with("CAN_RX bus=2 id=769 data=0101")
                    || line.starts_with("CAN_RX bus=2 id=785 data=0101")
                {
                    anyhow::bail!("FWが指令を拒否しました: {line}");
                }
                shared.tests.update(|s| {
                    s.last_rx = Some(Instant::now());
                    if session.visible(&line) {
                        s.logs.push_back(crate::fw_test::describe(&line));
                        while s.logs.len() > 120 {
                            s.logs.pop_front();
                        }
                    }
                });
            }
            thread::sleep(Duration::from_millis(50));
        }
        Ok(())
    })();
    let stopped = fw_test_transport::send(&mut link, session.stop());
    shared.tests.update(|s| {
        s.ready = false;
        s.switches.clear();
        s.outputs.clear();
        s.watchdog = false;
        if let Err(error) = result.and(stopped) {
            s.error = Some(format!("{error:#}"));
        }
    });
    // エラー後も通常操作へ切り替えず、「終了」の明示操作を待つ。
    while shared.is_running() && shared.tests.config().is_some() {
        thread::sleep(Duration::from_millis(50));
    }
}

pub struct Panel {
    config: Config,
    motors: Vec<(u8, f32)>,
    new_id: u8,
    guard_mask: u8,
    guard_high: u8,
}

impl Panel {
    pub fn new(device: String, baud: u32) -> Self {
        Self {
            config: Config {
                board: Board::Cctl,
                device,
                baud,
                seconds: 5,
            },
            motors: vec![(0, 0.0), (1, 0.0), (2, 0.0)],
            new_id: 1,
            guard_mask: 0,
            guard_high: 0,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, shared: &Shared) {
        ui.heading("基板テスト — 駆動・読取り・通信");
        ui.label("通常操作と排他。開始時に出力を停止します。配線変更は電源OFFで行ってください。");
        let enabled = shared.tests.enabled();
        let snapshot = shared.tests.snapshot();
        ui.add_enabled_ui(!enabled, |ui| {
            let previous = self.config.board;
            ui.horizontal(|ui| {
                for (board, label) in [
                    (Board::Cctl, "cctl"),
                    (Board::Svmd, "svmd"),
                    (Board::SerialSvmd, "serial_svmd"),
                    (Board::Dcmd, "DCMD"),
                ] {
                    ui.selectable_value(&mut self.config.board, board, label);
                }
            });
            if self.config.board != previous {
                self.guard_mask = 0;
                self.guard_high = 0;
                let cfg = shared.config();
                self.config.device = if self.config.board == Board::SerialSvmd {
                    cfg.machine
                        .serial_svmd
                        .as_ref()
                        .map(|b| b.device.clone())
                        .unwrap_or("/dev/ttyUSB0".into())
                } else {
                    cfg.serial_device
                };
                self.config.baud = if self.config.board == Board::SerialSvmd {
                    38400
                } else {
                    cfg.baud_rate
                };
                self.motors = match self.config.board {
                    Board::Cctl => vec![(0, 0.0), (1, 0.0), (2, 0.0)],
                    Board::Svmd => (0..4).map(|id| (id, 1500.0)).collect(),
                    Board::SerialSvmd => vec![(1, 2048.0)],
                    Board::Dcmd => vec![(0, 0.0)],
                };
            }
            ui.horizontal(|ui| {
                ui.label("接続先");
                ui.text_edit_singleline(&mut self.config.device);
                ui.label("baud");
                ui.add(egui::DragValue::new(&mut self.config.baud).range(1200..=1_000_000));
            });
            ui.horizontal(|ui| {
                ui.label("自動OFF [秒]");
                ui.add(egui::DragValue::new(&mut self.config.seconds).range(1..=30));
                if ui.button("テスト接続を開始").clicked() {
                    shared.start_test(self.config.clone());
                }
            });
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(enabled, egui::Button::new("全出力STOP"))
                .clicked()
            {
                shared.tests.command("stop".into());
            }
            if ui
                .add_enabled(enabled, egui::Button::new("停止してテスト終了"))
                .clicked()
            {
                shared.tests.end();
            }
            ui.label(if snapshot.ready {
                "接続確認済み"
            } else if enabled && snapshot.error.is_none() {
                "接続確認中…"
            } else {
                "停止中"
            });
        });
        if let Some(error) = &snapshot.error {
            ui.colored_label(egui::Color32::RED, error);
        }
        ui.label(match snapshot.last_rx {
            Some(time) => format!(
                "最終受信: {:.1}秒前（実機の動作成功を保証するものではありません）",
                time.elapsed().as_secs_f32()
            ),
            None => "受信データなし".into(),
        });
        ui.separator();
        ui.add_enabled_ui(enabled && snapshot.ready && !snapshot.watchdog, |ui| {
            ui.label("ON表示は指令状態。値を変更した後の「適用」で目標と停止期限を更新します。");
            for (id, value) in &mut self.motors {
                ui.horizontal(|ui| {
                    let (min, max, unit) = match self.config.board {
                        Board::Cctl if *id == 1 => (-26000.0, 26000.0, "motor deg"),
                        Board::Cctl => (-12.5, 12.5, "rad"),
                        Board::Svmd => (500.0, 2500.0, "us"),
                        Board::SerialSvmd => (0.0, 4095.0, "position"),
                        Board::Dcmd => (-100.0, 100.0, "permille (最大10%)"),
                    };
                    ui.label(format!("motor{id}"));
                    let mut drag = egui::DragValue::new(value).range(min..=max).speed(
                        if self.config.board == Board::Cctl {
                            0.1
                        } else {
                            1.0
                        },
                    );
                    if self.config.board != Board::Cctl {
                        drag = drag.max_decimals(0);
                    }
                    ui.add(drag);
                    ui.label(unit);
                    let name = format!("motor{id}");
                    let mut on = snapshot.switches.contains(&name);
                    if ui.checkbox(&mut on, "ON").changed() {
                        shared.tests.command(if on {
                            format!("on {name} {value}")
                        } else {
                            format!("off {name}")
                        });
                    }
                    if ui.add_enabled(on, egui::Button::new("適用")).clicked() {
                        shared.tests.command(format!("on {name} {value}"));
                    }
                    if self.config.board == Board::SerialSvmd {
                        toggle(ui, shared, &snapshot, &format!("read{id}"), "位置読取り");
                    }
                });
            }
            if self.config.board == Board::SerialSvmd {
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut self.new_id).range(1..=253));
                    if ui
                        .add_enabled(self.motors.len() < 16, egui::Button::new("ID行を追加"))
                        .clicked()
                        && !self.motors.iter().any(|(id, _)| *id == self.new_id)
                    {
                        self.motors.push((self.new_id, 2048.0));
                    }
                });
            }
            ui.horizontal(|ui| {
                toggle(
                    ui,
                    shared,
                    &snapshot,
                    "communication",
                    "通信確認・全受信表示",
                );
                if self.config.board != Board::SerialSvmd {
                    toggle(ui, shared, &snapshot, "status", "状態読取り");
                }
                if self.config.board == Board::Dcmd {
                    toggle(ui, shared, &snapshot, "encoder", "ENC1読取り");
                }
            });
            if ui
                .add_enabled(
                    !snapshot.outputs.is_empty(),
                    egui::Button::new("通信断テスト（500ms無送信）"),
                )
                .clicked()
            {
                shared.tests.command("watchdog".into());
            }
        });
        if snapshot.watchdog {
            ui.label("通信断テスト中。終了後の自動再始動はありません。");
        }
        ui.separator();
        ui.add_enabled_ui(enabled && snapshot.ready && !snapshot.watchdog, |ui| {
            toggle(ui, shared, &snapshot, "inputs", "スイッチ・DIP読取り");
            if let Some(state) = &snapshot.inputs {
                ui.label(format!("DIP={:04b}  FW停止mask={:06b}  開接点停止mask={:06b}  入力応答={:.1}秒前",
                    state.dip, state.guard, state.high, snapshot.input_rx.map(|t| t.elapsed().as_secs_f32()).unwrap_or(0.0)));
                if state.trip { ui.colored_label(egui::Color32::RED, "入力保護作動：出力禁止。原因を除き、停止条件を再設定して解除してください。"); }
                for index in 0..6 {
                    let bit = 1 << index;
                    if state.available & bit == 0 { continue; }
                    ui.horizontal(|ui| {
                        let name = if self.config.board == Board::Dcmd { ["SW_A", "SW_B", "SW_C"][index] .to_owned() } else { format!("SW{}", index + 1) };
                        ui.label(format!("{name}: {}（安定値 {}）", if state.raw & bit != 0 { "閉/LOW" } else { "開/HIGH" }, if state.stable & bit != 0 { "閉" } else { "開" }));
                        let mut selected = self.guard_mask & bit != 0;
                        if ui.checkbox(&mut selected, "停止入力").changed() {
                            if selected { self.guard_mask |= bit; } else { self.guard_mask &= !bit; self.guard_high &= !bit; }
                        }
                        let mut high = self.guard_high & bit != 0;
                        if ui.add_enabled(selected, egui::Checkbox::new(&mut high, "開接点で停止")).changed() {
                            if high { self.guard_high |= bit; } else { self.guard_high &= !bit; }
                        }
                    });
                }
                if state.available != 0 {
                    ui.label("未選択は監視のみ。停止条件は基板全出力に作用し、電源再投入で解除されます。");
                    if ui.add_enabled(snapshot.outputs.is_empty(), egui::Button::new("停止条件を設定・ラッチ解除（全出力停止）")).clicked() {
                        shared.tests.command(format!("guard {} {}", self.guard_mask, self.guard_high));
                    }
                } else { ui.label("この基板に外部SW入力はありません。DIPの状態のみ読み取れます。"); }
            } else { ui.label("読取りをONにすると入力状態と停止条件を表示します。対応FWへの更新が必要です。"); }
        });
        ui.collapsing("テスト機能と単位", |ui| {
            ui.label(Session::new(self.config.board, Duration::from_secs(5)).help());
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("test_log")
            .max_height(220.0)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in &snapshot.logs {
                    ui.monospace(line);
                }
            });
    }
}

fn toggle(ui: &mut egui::Ui, shared: &Shared, snapshot: &Snapshot, name: &str, label: &str) {
    let mut enabled = snapshot.switches.iter().any(|s| s == name);
    if ui.checkbox(&mut enabled, label).changed() {
        shared
            .tests
            .command(format!("{} {name}", if enabled { "on" } else { "off" }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> Config {
        Config {
            board: Board::Dcmd,
            device: "/dev/null".into(),
            baud: 115200,
            seconds: 5,
        }
    }

    #[test]
    fn stop_discards_pending_output_commands() {
        let control = Control::default();
        control.start(config());
        control.update(|s| s.ready = true);
        control.command("on motor0 10".into());
        control.command("stop".into());
        assert_eq!(control.take().as_deref(), Some("stop"));
        assert!(control.take().is_none());
    }

    #[test]
    fn closing_session_cannot_be_replaced_until_worker_releases_port() {
        let control = Control::default();
        control.start(config());
        control.update(|s| s.ready = true);
        control.end();
        control.start(config());
        assert!(control.enabled());
        assert!(control.config().is_none());
        control.command("on motor0 10".into());
        assert!(control.take().is_none());
        control.finish();
        assert!(!control.enabled());
        control.start(config());
        assert!(control.config().is_some());
        assert!(!control.snapshot().ready);
    }

    #[test]
    fn handshake_and_watchdog_block_output_requests() {
        let control = Control::default();
        control.start(config());
        control.command("on motor0 10".into());
        assert!(control.take().is_none());
        control.update(|s| {
            s.ready = true;
            s.watchdog = true;
        });
        control.command("on motor0 10".into());
        assert!(control.take().is_none());
        control.command("stop".into());
        assert_eq!(control.take().as_deref(), Some("stop"));
    }

    #[test]
    fn panel_renders_all_boards_without_hardware() {
        let shared = Shared::new(crate::app_state::BridgeConfig {
            serial_device: "/dev/null".into(),
            baud_rate: 115200,
            rate_hz: 20.0,
            machine: crate::machine::MachineProfile::load(None).unwrap(),
        });
        let ctx = egui::Context::default();
        let mut panel = Panel::new("/dev/null".into(), 115200);
        for board in [Board::Cctl, Board::Svmd, Board::SerialSvmd, Board::Dcmd] {
            panel.config.board = board;
            let mut output = ctx.run_ui(Default::default(), |ui| panel.ui(ui, &shared));
            output.textures_delta.clear();
        }
    }
}

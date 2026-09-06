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
    /// 出力中の番号と自動OFFの期限。
    pub outputs: Vec<(String, Instant)>,
    /// ネットワーク確認で応答しなかった基板。
    pub missing: Vec<String>,
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
        let reachable = fw_test_transport::prepare(&mut link, config.board, &mut session)?;
        session.retain_boards(&reachable);
        let missing: Vec<String> = config
            .board
            .members()
            .into_iter()
            .filter(|board| {
                matches!(board, Board::Svmd | Board::Dcmd) && !reachable.contains(board)
            })
            .map(|board| board.key().to_owned())
            .collect();
        shared.tests.update(|s| {
            s.ready = true;
            s.missing = missing;
        });
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
                s.outputs = session.output_expiries();
                s.watchdog = session.watchdog_running();
            });
            for line in link.read_lines() {
                if let Some(state) = crate::inputs::parse(&line, config.board) {
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
    motors: Vec<(Board, u8, f32)>,
    new_id: u8,
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
            motors: default_motors(Board::Cctl),
            new_id: 1,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, shared: &Shared) {
        let enabled = shared.tests.enabled();
        let snapshot = shared.tests.snapshot();

        ui.heading("動作テスト");
        ui.label(
            "基板に繋いだデバイスを機能ごとに動かして、配線と通信を確かめます。通常操作とは排他です。",
        );
        ui.add_space(6.0);
        state_banner(ui, enabled, &snapshot);
        ui.add_space(10.0);

        self.connection_ui(ui, shared, enabled);
        if enabled {
            ui.add_space(10.0);
            self.output_ui(ui, shared, &snapshot);
            ui.add_space(10.0);
            self.monitor_ui(ui, shared, &snapshot);
            ui.add_space(10.0);
            control_ui(ui, shared, &snapshot);
        }
        ui.add_space(10.0);
        log_ui(ui, &snapshot);
        reference_ui(ui, self.config.board);
    }

    /// 1. どの基板へどう繋ぐか。開始後は変更できない。
    fn connection_ui(&mut self, ui: &mut egui::Ui, shared: &Shared, enabled: bool) {
        section(ui, "1. 接続");
        if enabled {
            ui.label(format!(
                "{} — {} @ {} baud（テスト中は変更できません）",
                board_name(self.config.board),
                self.config.device,
                self.config.baud
            ));
            return;
        }
        let previous = self.config.board;
        ui.horizontal(|ui| {
            ui.label("対象");
            for board in [
                Board::Network,
                Board::Cctl,
                Board::Svmd,
                Board::SerialSvmd,
                Board::Dcmd,
            ] {
                ui.selectable_value(&mut self.config.board, board, board_name(board));
            }
        });
        if self.config.board != previous {
            self.reset_for_board(shared);
        }
        ui.label(connection_hint(self.config.board));
        ui.horizontal(|ui| {
            ui.label("接続先");
            ui.text_edit_singleline(&mut self.config.device);
            ui.label("baud");
            ui.add(egui::DragValue::new(&mut self.config.baud).range(1200..=1_000_000));
        });
        ui.horizontal(|ui| {
            ui.label("出力の自動OFF");
            ui.add(egui::DragValue::new(&mut self.config.seconds).range(1..=30));
            ui.label("秒");
        });
        if ui.button("テストを開始").clicked() {
            shared.start_test(self.config.clone());
        }
    }

    fn reset_for_board(&mut self, shared: &Shared) {
        let cfg = shared.config();
        let serial_svmd = self.config.board == Board::SerialSvmd;
        self.config.device = if serial_svmd {
            cfg.machine
                .serial_svmd
                .as_ref()
                .map(|board| board.device.clone())
                .unwrap_or("/dev/ttyUSB0".into())
        } else {
            cfg.serial_device
        };
        self.config.baud = if serial_svmd { 38400 } else { cfg.baud_rate };
        self.motors = default_motors(self.config.board);
    }

    /// 2. 出力。1行が1デバイスで、チェックで出力、値の変更は「送り直す」で反映する。
    fn output_ui(&mut self, ui: &mut egui::Ui, shared: &Shared, snapshot: &Snapshot) {
        section(ui, "2. 出力");
        let live = snapshot.ready && !snapshot.watchdog;
        if !live {
            ui.label("接続確認が済むまで出力できません。");
        }
        let network = self.config.board == Board::Network;
        ui.add_enabled_ui(live, |ui| {
            let mut shown = None;
            for (board, id, value) in &mut self.motors {
                if network && shown != Some(*board) {
                    shown = Some(*board);
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(board_name(*board)).strong());
                }
                let name = feature(network, *board, &format!("motor{id}"));
                let on = snapshot.switches.contains(&name);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{:<22}", output_label(*board, *id))).monospace(),
                    );
                    let (min, max, unit) = output_range(*board, *id);
                    let mut drag = egui::DragValue::new(value).range(min..=max);
                    drag = if *board == Board::Cctl {
                        drag.speed(0.1)
                    } else {
                        drag.speed(1.0).max_decimals(0)
                    };
                    ui.add(drag);
                    ui.label(unit);

                    let mut checked = on;
                    if ui.checkbox(&mut checked, "出力").changed() {
                        shared.tests.command(if checked {
                            format!("on {name} {value}")
                        } else {
                            format!("off {name}")
                        });
                    }
                    if ui
                        .add_enabled(on, egui::Button::new("送り直す"))
                        .on_hover_text("いまの値を送り直し、自動OFFまでの時間を延長します。")
                        .clicked()
                    {
                        shared.tests.command(format!("on {name} {value}"));
                    }
                    if let Some(remaining) = remaining_seconds(snapshot, &name) {
                        ui.label(format!("自動OFFまで {remaining:.0} 秒"));
                    }
                });
            }
            if self.config.board == Board::SerialSvmd {
                ui.horizontal(|ui| {
                    ui.label("IDを追加");
                    ui.add(egui::DragValue::new(&mut self.new_id).range(1..=253));
                    if ui
                        .add_enabled(self.motors.len() < 16, egui::Button::new("追加"))
                        .clicked()
                        && !self.motors.iter().any(|(_, id, _)| *id == self.new_id)
                    {
                        self.motors.push((Board::SerialSvmd, self.new_id, 2048.0));
                    }
                });
            }
        });
        ui.label("チェックはhostの指令状態で、実機が動いた保証ではありません。");
    }

    /// 3. 読取り。基板から返る値を見るための切り替え。
    fn monitor_ui(&mut self, ui: &mut egui::Ui, shared: &Shared, snapshot: &Snapshot) {
        section(ui, "3. 読取り");
        let live = snapshot.ready && !snapshot.watchdog;
        let target = self.config.board;
        let network = target == Board::Network;
        ui.add_enabled_ui(live, |ui| {
            for board in target.members() {
                ui.horizontal(|ui| {
                    if network {
                        ui.label(egui::RichText::new(format!("{:<8}", board_name(board))).monospace());
                    }
                    if board != Board::SerialSvmd {
                        let name = feature(network, board, "status");
                        toggle(ui, shared, snapshot, &name, "状態");
                    }
                    if board == Board::Dcmd {
                        let name = feature(network, board, "encoder");
                        toggle(ui, shared, snapshot, &name, "ENC1");
                        let name = feature(network, board, "inputs");
                        toggle(ui, shared, snapshot, &name, "接点・DIP");
                    }
                    if board == Board::SerialSvmd && crate::inputs::supported(board) {
                        let name = feature(network, board, "inputs");
                        toggle(ui, shared, snapshot, &name, "接点・DIP");
                    }
                });
            }
            toggle(ui, shared, snapshot, "communication", "受信をすべて表示");
            if target == Board::SerialSvmd {
                ui.horizontal(|ui| {
                    ui.label("位置読取り");
                    for (_, id, _) in &self.motors {
                        toggle(ui, shared, snapshot, &format!("read{id}"), &format!("ID {id}"));
                    }
                });
            }
            if target.members().contains(&Board::Cctl) {
                ui.label("cctlの接点とDIPは「状態」のSTATE行に sw= として出ます。");
            }
            if let Some(state) = &snapshot.inputs {
                contacts_ui(ui, Board::Dcmd, state, snapshot.input_rx);
            }
        });
    }
}

/// 機能名。ネットワーク確認では基板を前置きする。
fn feature(network: bool, board: Board, name: &str) -> String {
    if network {
        format!("{}.{name}", board.key())
    } else {
        name.to_owned()
    }
}

/// いま何ができる状態かを一行で示す。
fn state_banner(ui: &mut egui::Ui, enabled: bool, snapshot: &Snapshot) {
    let (text, color) = if let Some(error) = &snapshot.error {
        (error.clone(), egui::Color32::from_rgb(200, 60, 60))
    } else if !enabled {
        (
            "停止中 — 接続先を決めて「テストを開始」".to_owned(),
            egui::Color32::GRAY,
        )
    } else if snapshot.watchdog {
        (
            "通信断テスト中 — 送信を止めています".to_owned(),
            egui::Color32::from_rgb(200, 140, 0),
        )
    } else if snapshot.ready {
        (
            "テスト中 — 出力できます".to_owned(),
            egui::Color32::from_rgb(0, 150, 0),
        )
    } else {
        (
            "接続確認中… — 基板からの応答を待っています".to_owned(),
            egui::Color32::from_rgb(200, 140, 0),
        )
    };
    ui.colored_label(color, egui::RichText::new(text).strong());
    ui.label(match snapshot.last_rx {
        Some(at) => format!("最終受信: {:.1} 秒前", at.elapsed().as_secs_f32()),
        None => "受信データなし".into(),
    });
    if !snapshot.missing.is_empty() {
        ui.colored_label(
            egui::Color32::from_rgb(200, 140, 0),
            format!(
                "CAN先の応答なし: {}。電源・CAN配線・終端抵抗を確認してください。",
                snapshot.missing.join(", ")
            ),
        );
    }
}

/// 4. 停止と終了。危険側の操作をまとめて置く。
fn control_ui(ui: &mut egui::Ui, shared: &Shared, snapshot: &Snapshot) {
    section(ui, "4. 停止");
    ui.horizontal(|ui| {
        if ui.button("全出力STOP").clicked() {
            shared.tests.command("stop".into());
        }
        if ui
            .add_enabled(
                !snapshot.outputs.is_empty(),
                egui::Button::new("通信断テスト（500ms無送信）"),
            )
            .on_hover_text("送信を500ms止め、FWのWatchdogが出力を切ることを確かめます。")
            .clicked()
        {
            shared.tests.command("watchdog".into());
        }
        if ui.button("停止してテストを終了").clicked() {
            shared.tests.end();
        }
    });
    ui.label("Space でもSTOPを送れます（入力欄の編集中を除く）。");
}

/// GUIとCLIは同じSessionを使う。対応するコマンドを畳んで置いておく。
fn reference_ui(ui: &mut egui::Ui, board: Board) {
    ui.collapsing("CLI（cargo run --bin fw_test）での書き方", |ui| {
        ui.monospace(Session::new(board, Duration::from_secs(5)).help());
    });
}

fn log_ui(ui: &mut egui::Ui, snapshot: &Snapshot) {
    section(ui, "受信ログ");
    egui::ScrollArea::vertical()
        .id_salt("test_log")
        .max_height(200.0)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for line in &snapshot.logs {
                ui.monospace(line);
            }
        });
}

fn contacts_ui(
    ui: &mut egui::Ui,
    board: Board,
    state: &crate::inputs::InputState,
    received: Option<Instant>,
) {
    ui.horizontal(|ui| {
        for index in 0..8 {
            let bit = 1 << index;
            if state.available & bit == 0 {
                continue;
            }
            let name = if board == Board::Dcmd {
                ["SW_A", "SW_B", "SW_C"][index].to_owned()
            } else {
                format!("SW{}", index + 1)
            };
            let closed = state.raw & bit != 0;
            ui.colored_label(
                if closed {
                    egui::Color32::from_rgb(0, 150, 0)
                } else {
                    egui::Color32::GRAY
                },
                format!("{name} {}", if closed { "閉" } else { "開" }),
            );
        }
    });
    ui.label(format!(
        "DIP={:04b}  受信 {:.1} 秒前",
        state.dip,
        received.map(|at| at.elapsed().as_secs_f32()).unwrap_or(0.0)
    ));
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.label(egui::RichText::new(title).strong());
    ui.separator();
}

fn board_name(board: Board) -> &'static str {
    match board {
        Board::Cctl => "cctl",
        Board::Svmd => "svmd",
        Board::SerialSvmd => "serial_svmd",
        Board::Dcmd => "DCMD",
        Board::Network => "ネットワーク一括",
    }
}

/// 対象を選んだときの初期行。ネットワークでは全基板ぶんを並べる。
fn default_motors(board: Board) -> Vec<(Board, u8, f32)> {
    board
        .members()
        .into_iter()
        .flat_map(|board| match board {
            Board::Cctl => vec![(board, 0, 0.0), (board, 1, 0.0), (board, 2, 0.0)],
            Board::Svmd => (0..4).map(|channel| (board, channel, 1500.0)).collect(),
            Board::SerialSvmd => vec![(board, 1, 2048.0)],
            Board::Dcmd | Board::Network => vec![(board, 0, 0.0)],
        })
        .collect()
}

fn connection_hint(board: Board) -> &'static str {
    match board {
        Board::Network => {
            "cctlのUSB 1本で、cctl・svmd・DCMDを繋ぎ替えずにまとめて確認します。"
        }
        Board::Cctl => "USB CDCへ直接繋ぎます。",
        Board::Svmd => "cctlのFDCAN2経由。接続先はcctlのUSB CDCです。",
        Board::SerialSvmd => "USART2のUSBシリアル変換器へ直接繋ぎます（既定38400 baud）。",
        Board::Dcmd => "cctlのFDCAN2経由。接続先はcctlのUSB CDCです。",
    }
}

/// 行の見出しは機体の軸ではなく、基板に繋がるデバイスで表す。
fn output_label(board: Board, id: u8) -> String {
    match board {
        Board::Cctl => match id {
            0 => "slot 0 — EL05".into(),
            1 => "slot 1 — M3508+C620".into(),
            _ => "slot 2 — DM-S3519".into(),
        },
        Board::Svmd => format!("ch {id} — PWMサーボ"),
        Board::SerialSvmd => format!("ID {id} — STS3215"),
        Board::Dcmd | Board::Network => "PWM0 — DCモータ".into(),
    }
}

fn output_range(board: Board, id: u8) -> (f32, f32, &'static str) {
    match board {
        Board::Cctl if id == 1 => (-26000.0, 26000.0, "motor deg"),
        Board::Cctl => (-12.5, 12.5, "rad"),
        Board::Svmd => (500.0, 2500.0, "us"),
        Board::SerialSvmd => (0.0, 4095.0, "position"),
        Board::Dcmd | Board::Network => (-100.0, 100.0, "permille（上限10%）"),
    }
}

fn remaining_seconds(snapshot: &Snapshot, name: &str) -> Option<f32> {
    let (_, at) = snapshot.outputs.iter().find(|(output, _)| output == name)?;
    Some(at.saturating_duration_since(Instant::now()).as_secs_f32())
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

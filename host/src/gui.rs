//! eframe/egui ベースの GUI。
//!
//! 操作は vim 風のモーダル方式。Normal でキーがそのまま操作になり、`:` で
//! 開くコマンドラインからは cctl の指令行をそのまま送れる。
//! キー割当の解釈は crate::keymap にあり、GUI はその結果を実行するだけ。

use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{self, FontData};

use crate::app_state::{BridgeConfig, Shared};
use crate::frame::Command;
use crate::keymap::{self, Action, ExCommand, Key, Mode, Normal};
use crate::telemetry::{axis_bit, RunMode, Telemetry};

/// 画面（タブ）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Status,
    Config,
    Operation,
    Help,
}

const SCREENS: [(Screen, &str); 4] = [
    (Screen::Status, "1 ステータス"),
    (Screen::Config, "2 設定"),
    (Screen::Operation, "3 操作"),
    (Screen::Help, "4 ヘルプ"),
];

/// 操作画面で選べる項目。
#[derive(Clone, Copy)]
enum Operation {
    Run,
    Safe,
    Home,
    ToggleAxis(u8),
    Test(bool),
}

const OPERATIONS: [(Operation, &str); 8] = [
    (Operation::Run, "RUN（運転開始）"),
    (Operation::Safe, "SAFE（待機へ）"),
    (Operation::Home, "HOME（原点取り直し）"),
    (Operation::ToggleAxis(axis_bit::R), "r 軸 (EL05) を切り替え"),
    (Operation::ToggleAxis(axis_bit::THETA), "θ 軸 (M3508) を切り替え"),
    (Operation::ToggleAxis(axis_bit::Z), "z 軸 (DM) を切り替え"),
    (Operation::Test(true), "テスト動作 開始"),
    (Operation::Test(false), "テスト動作 停止"),
];

/// 設定画面で編集できる項目数。
const CONFIG_FIELDS: usize = 3;

pub struct BridgeApp {
    shared: Arc<Shared>,
    screen: Screen,
    // 設定画面の編集バッファ（適用するまで共有設定へは反映しない）。
    edit_device: String,
    edit_baud: u32,
    edit_rate: f64,

    mode: Mode,
    normal: Normal,
    command_input: String,
    /// 直前の操作結果。画面下部に出す。
    message: Option<String>,
    /// 現在の画面での選択位置。
    selection: usize,
}

impl BridgeApp {
    pub fn new(shared: Arc<Shared>) -> Self {
        let cfg = shared.config();
        Self {
            shared,
            screen: Screen::Status,
            edit_device: cfg.serial_device,
            edit_baud: cfg.baud_rate,
            edit_rate: cfg.rate_hz,
            mode: Mode::Normal,
            normal: Normal::default(),
            command_input: String::new(),
            message: None,
            selection: 0,
        }
    }

    /// 現在の画面で選択できる項目数。
    fn selection_len(&self) -> usize {
        match self.screen {
            Screen::Operation => OPERATIONS.len(),
            Screen::Config => CONFIG_FIELDS,
            _ => 0,
        }
    }

    fn screen_index(&self) -> usize {
        SCREENS
            .iter()
            .position(|(screen, _)| *screen == self.screen)
            .unwrap_or(0)
    }

    fn goto_screen(&mut self, index: usize) {
        if let Some((screen, _)) = SCREENS.get(index) {
            self.screen = *screen;
            self.selection = 0;
        }
    }

    // ---- キー入力 ---------------------------------------------------------

    fn handle_keys(&mut self, ctx: &egui::Context) {
        if self.mode != Mode::Normal {
            return;
        }

        // Normal では入力欄にフォーカスを残さない。キーが吸われるのを防ぐ。
        ctx.memory_mut(|memory| memory.stop_text_input());

        for key in take_keys(ctx) {
            let action = self.normal.press(key);
            self.apply(action, ctx);
        }
    }

    fn apply(&mut self, action: Action, _ctx: &egui::Context) {
        let len = self.selection_len();
        match action {
            Action::None => {}
            Action::NextScreen => {
                let next = (self.screen_index() + 1) % SCREENS.len();
                self.goto_screen(next);
            }
            Action::PrevScreen => {
                let prev = (self.screen_index() + SCREENS.len() - 1) % SCREENS.len();
                self.goto_screen(prev);
            }
            Action::GotoScreen(index) => self.goto_screen(index),
            Action::MoveDown => {
                if len > 0 {
                    self.selection = (self.selection + 1) % len;
                }
            }
            Action::MoveUp => {
                if len > 0 {
                    self.selection = (self.selection + len - 1) % len;
                }
            }
            Action::MoveTop => self.selection = 0,
            Action::MoveBottom => self.selection = len.saturating_sub(1),
            Action::Activate => self.activate(),
            Action::Stop => {
                self.shared.queue_command(Command::Stop);
                self.message = Some("STOP を送信しました。".to_owned());
            }
            Action::EnterCommand => {
                self.mode = Mode::Command;
                self.command_input.clear();
                self.message = None;
            }
            Action::EnterInsert => {
                if self.screen == Screen::Config {
                    self.mode = Mode::Insert;
                } else {
                    self.message = Some("この画面に編集できる項目はありません。".to_owned());
                }
            }
            Action::ShowHelp => self.screen = Screen::Help,
            Action::Cancel => self.message = None,
        }
    }

    /// 操作画面で選択中の項目を実行する。
    fn activate(&mut self) {
        if self.screen != Screen::Operation {
            return;
        }
        let Some((operation, label)) = OPERATIONS.get(self.selection) else {
            return;
        };

        let enabled_axes = self
            .shared
            .status_snapshot()
            .telemetry
            .as_ref()
            .map(|telemetry| telemetry.enabled_axes);

        let command = match *operation {
            Operation::Run => Command::Run,
            Operation::Safe => Command::Safe,
            Operation::Home => Command::Home,
            Operation::ToggleAxis(bit) => {
                // テレメトリが無い間は有効化する方向に倒す。
                let currently_enabled = enabled_axes.is_some_and(|axes| axes & bit != 0);
                Command::Enable {
                    axes: bit,
                    enabled: !currently_enabled,
                }
            }
            Operation::Test(enabled) => Command::Test(enabled),
        };

        self.shared.queue_command(command);
        self.message = Some(format!("{label} → {}", command.to_line()));
    }

    fn submit_command(&mut self, ctx: &egui::Context) {
        let input = std::mem::take(&mut self.command_input);
        self.mode = Mode::Normal;

        match keymap::parse_ex(&input) {
            ExCommand::Empty => {}
            ExCommand::Quit => {
                self.shared.request_stop();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ExCommand::Help => self.screen = Screen::Help,
            ExCommand::Send(line) => {
                self.message = Some(format!("送信: {line}"));
                self.shared.queue_line(line);
            }
        }
    }

    // ---- 各画面 -----------------------------------------------------------

    fn status_ui(&mut self, ui: &mut egui::Ui) {
        let status = self.shared.status_snapshot();
        ui.heading("ステータス");
        ui.separator();

        egui::Grid::new("status_grid")
            .num_columns(2)
            .spacing([16.0, 6.0])
            .show(ui, |ui| {
                ui.label("コントローラ");
                ui.label(match &status.gamepad_name {
                    Some(name) if status.gamepad_connected => format!("接続: {name}"),
                    _ => "未接続".to_owned(),
                });
                ui.end_row();

                ui.label("シリアル");
                ui.label(if status.serial_connected {
                    "接続"
                } else {
                    "未接続"
                });
                ui.end_row();

                ui.label("送信");
                ui.label(if self.shared.sending_enabled() {
                    "有効"
                } else {
                    "停止"
                });
                ui.end_row();

                ui.label("送信回数");
                ui.label(status.tx_count.to_string());
                ui.end_row();

                ui.label("最終エラー");
                ui.label(status.last_error.as_deref().unwrap_or("-"));
                ui.end_row();
            });

        ui.separator();
        ui.label("入力値");
        ui.monospace(format!(
            "axes: [{}]",
            status
                .axes
                .iter()
                .map(|value| format!("{value:+.2}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        ui.monospace(format!(
            "buttons: [{}]",
            status
                .buttons
                .iter()
                .map(|button| button.to_string())
                .collect::<Vec<_>>()
                .join("")
        ));
        ui.add_space(8.0);
        ui.monospace(format!("last line: {}", status.last_line));

        ui.separator();
        ui.label("機体の状態");
        match &status.telemetry {
            Some(telemetry) => telemetry_ui(ui, telemetry, status.telemetry_count),
            None => {
                ui.label("cctl からのテレメトリを受信していません。");
            }
        }
    }

    fn config_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("設定");
        ui.separator();
        ui.label("j / k で項目を移動、i で編集、Esc で編集を抜ける。");
        ui.add_space(4.0);

        let editing = self.mode == Mode::Insert;
        let selection = self.selection;
        let marker = |index: usize| if selection == index { "▶" } else { " " };

        egui::Grid::new("config_grid")
            .num_columns(3)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.monospace(marker(0));
                ui.label("シリアルデバイス");
                let response =
                    ui.add(egui::TextEdit::singleline(&mut self.edit_device).desired_width(240.0));
                if editing && selection == 0 && !response.has_focus() {
                    response.request_focus();
                }
                ui.end_row();

                ui.monospace(marker(1));
                ui.label("ボーレート");
                let response = ui.add(
                    egui::DragValue::new(&mut self.edit_baud)
                        .speed(100)
                        .range(1200..=1_000_000),
                );
                if editing && selection == 1 && !response.has_focus() {
                    response.request_focus();
                }
                ui.end_row();

                ui.monospace(marker(2));
                ui.label("送信周期 [Hz]");
                let response = ui.add(
                    egui::DragValue::new(&mut self.edit_rate)
                        .speed(1.0)
                        .range(1.0..=200.0),
                );
                if editing && selection == 2 && !response.has_focus() {
                    response.request_focus();
                }
                ui.end_row();
            });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("適用").clicked() {
                self.shared.set_config(BridgeConfig {
                    serial_device: self.edit_device.clone(),
                    baud_rate: self.edit_baud,
                    rate_hz: self.edit_rate,
                    machine: self.shared.config().machine,
                });
            }
            if ui.button("現在値に戻す").clicked() {
                let cfg = self.shared.config();
                self.edit_device = cfg.serial_device;
                self.edit_baud = cfg.baud_rate;
                self.edit_rate = cfg.rate_hz;
            }
        });
        ui.add_space(4.0);
        ui.label("「適用」でワーカーがシリアルを開き直します。");
    }

    fn operation_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("操作");
        ui.separator();

        let status = self.shared.status_snapshot();
        match &status.telemetry {
            Some(telemetry) => {
                ui.colored_label(
                    mode_color(telemetry.mode),
                    egui::RichText::new(telemetry.mode.label()).strong(),
                );
            }
            None => {
                ui.label("機体の状態は不明（テレメトリ未受信）");
            }
        }

        ui.add_space(4.0);
        ui.label("j / k で選び、Enter で実行。Space はいつでも STOP。");
        ui.add_space(8.0);

        let enabled_axes = status.telemetry.as_ref().map(|telemetry| telemetry.enabled_axes);
        let mut clicked = None;

        for (index, (operation, label)) in OPERATIONS.iter().enumerate() {
            let selected = index == self.selection;
            let suffix = match operation {
                Operation::ToggleAxis(bit) => match enabled_axes {
                    Some(axes) if axes & bit != 0 => "  [現在: 有効]",
                    Some(_) => "  [現在: 無効]",
                    None => "  [現在: 不明]",
                },
                _ => "",
            };

            let marker = if selected { "▶" } else { " " };
            let text = egui::RichText::new(format!("{marker} {label}{suffix}")).monospace();
            if ui.selectable_label(selected, text).clicked() {
                clicked = Some(index);
            }
        }

        if let Some(index) = clicked {
            self.selection = index;
            self.activate();
        }

        ui.add_space(12.0);
        ui.separator();
        let mut sending = self.shared.sending_enabled();
        if ui
            .checkbox(&mut sending, "コントローラ入力の送信を有効化")
            .changed()
        {
            self.shared.set_sending_enabled(sending);
        }
    }

    fn help_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("キー操作");
        ui.separator();

        const ROWS: [(&str, &str); 13] = [
            ("Space", "STOP を送る。どの画面でも効く"),
            ("j / k", "選択を下 / 上へ"),
            ("gg / G", "選択を先頭 / 末尾へ"),
            ("Enter", "選択中の項目を実行"),
            ("gt / gT", "次 / 前の画面へ"),
            ("1 - 4", "画面を直接選ぶ"),
            ("i", "設定画面で編集を始める"),
            ("Esc", "編集・コマンドラインを抜ける"),
            (":", "コマンドラインを開く"),
            ("?", "この画面"),
            (":q", "終了"),
            (":h", "この画面"),
            (":<指令>", "cctl へそのまま送る（例 :EN T 1）"),
        ];

        egui::Grid::new("help_grid")
            .num_columns(2)
            .spacing([24.0, 6.0])
            .striped(true)
            .show(ui, |ui| {
                for (key, description) in ROWS {
                    ui.monospace(key);
                    ui.label(description);
                    ui.end_row();
                }
            });

        ui.add_space(8.0);
        ui.label("コマンドラインは cctl の指令をそのまま受け取ります。");
        ui.monospace(":STOP   :RUN   :SAFE   :HOME   :EN RTZ 1   :TEST 1");
        ui.add_space(4.0);
        ui.label("GUI が知らない語は cctl へ送るので、ファーム側に指令が増えても");
        ui.label("host を更新せずに使えます。指令の一覧は docs/bringup.md を参照。");
    }

    /// 画面下部のモード表示とコマンドライン。
    fn status_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.mode == Mode::Command {
            ui.horizontal(|ui| {
                ui.monospace(":");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.command_input)
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Monospace),
                );
                response.request_focus();

                if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                    self.command_input.clear();
                    self.mode = Mode::Normal;
                } else if ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    self.submit_command(ctx);
                }
            });
            return;
        }

        if self.mode == Mode::Insert && ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.mode = Mode::Normal;
        }

        ui.horizontal(|ui| {
            let color = match self.mode {
                Mode::Normal => egui::Color32::from_rgb(120, 170, 220),
                Mode::Insert => egui::Color32::from_rgb(120, 200, 120),
                Mode::Command => egui::Color32::from_rgb(200, 200, 120),
            };
            ui.colored_label(color, egui::RichText::new(self.mode.label()).monospace());

            if let Some(pending) = self.normal.pending() {
                ui.monospace(format!("({pending})"));
            }
            if let Some(message) = &self.message {
                ui.separator();
                ui.label(message);
            }
        });
    }
}

impl eframe::App for BridgeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // 状態を定期的に反映するため再描画を予約する。
        ctx.request_repaint_after(Duration::from_millis(100));

        self.handle_keys(&ctx);

        egui::Panel::left("nav")
            .resizable(false)
            .exact_size(160.0)
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.heading("host");
                ui.separator();
                for (screen, label) in SCREENS {
                    ui.selectable_value(&mut self.screen, screen, label);
                }
                ui.add_space(12.0);
                ui.label("? でキー一覧");
            });

        egui::Panel::bottom("statusbar").show(ui, |ui| {
            self.status_bar(ui, &ctx);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| match self.screen {
                Screen::Status => self.status_ui(ui),
                Screen::Config => self.config_ui(ui),
                Screen::Operation => self.operation_ui(ui),
                Screen::Help => self.help_ui(ui),
            });
        });
    }

    fn on_exit(&mut self) {
        self.shared.request_stop();
    }
}

/// egui の入力イベントを keymap のキーへ変換し、消費したものを取り除く。
///
/// Normal ではキーボードを keymap が占有する。取り除かないと、同じ打鍵が
/// フォーカスのあるウィジェットにも届いてしまう（`:` がコマンドラインに
/// 入る、Space がボタンを押す、など）。マウス操作は残す。
fn take_keys(ctx: &egui::Context) -> Vec<Key> {
    ctx.input_mut(|input| {
        let mut keys = Vec::new();
        for event in &input.events {
            match event {
                // 空白は Key イベント側で拾うので、ここでは落とす。
                egui::Event::Text(text) => {
                    keys.extend(text.chars().filter(|ch| *ch != ' ').map(Key::Char));
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if !modifiers.ctrl && !modifiers.alt && !modifiers.command => match key {
                    egui::Key::Enter => keys.push(Key::Enter),
                    egui::Key::Escape => keys.push(Key::Escape),
                    egui::Key::Space => keys.push(Key::Space),
                    egui::Key::ArrowUp => keys.push(Key::Up),
                    egui::Key::ArrowDown => keys.push(Key::Down),
                    _ => {}
                },
                _ => {}
            }
        }

        input
            .events
            .retain(|event| !matches!(event, egui::Event::Text(_) | egui::Event::Key { .. }));

        keys
    })
}

fn mode_color(mode: RunMode) -> egui::Color32 {
    match mode {
        RunMode::Safe => egui::Color32::from_rgb(200, 170, 60),
        RunMode::Run => egui::Color32::from_rgb(90, 180, 90),
        RunMode::Stop => egui::Color32::from_rgb(200, 80, 80),
    }
}

/// 目標値と実測値を並べて表示する。
fn telemetry_ui(ui: &mut egui::Ui, telemetry: &Telemetry, count: u64) {
    ui.colored_label(
        mode_color(telemetry.mode),
        egui::RichText::new(telemetry.mode.label()).strong(),
    );

    egui::Grid::new("telemetry_grid")
        .num_columns(4)
        .spacing([16.0, 6.0])
        .striped(true)
        .show(ui, |ui| {
            ui.label("軸");
            ui.label("目標");
            ui.label("実測");
            ui.label("誤差");
            ui.end_row();

            for (bit, name, axis, unit) in [
                (axis_bit::R, "r", telemetry.r, "mm"),
                (axis_bit::THETA, "θ", telemetry.theta, "deg"),
                (axis_bit::Z, "z", telemetry.z, "mm"),
            ] {
                let enabled = telemetry.axis_enabled(bit);
                ui.label(format!("{name} {}", if enabled { "●" } else { "○" }));
                ui.monospace(format!("{:+8.3} {unit}", axis.target));
                ui.monospace(format!("{:+8.3} {unit}", axis.measured));
                ui.monospace(format!("{:+8.3}", axis.error()));
                ui.end_row();
            }
        });

    ui.add_space(4.0);
    ui.monospace(format!(
        "uptime: {} ms / err: {:02X} / 受信: {} 行",
        telemetry.uptime_ms, telemetry.error_bits, count
    ));
    if telemetry.error_bits != 0 {
        ui.colored_label(
            egui::Color32::from_rgb(200, 80, 80),
            "z 軸ドライバがエラーを報告しています。",
        );
    }
}

/// システムにある日本語対応フォントを探して egui に登録する。
/// 見つからない場合は既定フォント（日本語は豆腐表示）にフォールバックする。
pub fn install_japanese_font(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "noto_sans_jp".to_owned(),
        FontData::from_static(include_bytes!("../assets/NotoSansJP-Regular.ttf")).into(),
    );
    fonts.font_data.insert(
        "source_han_code_jp".to_owned(),
        FontData::from_static(include_bytes!("../assets/SourceHanCodeJP-Regular.otf")).into(),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "noto_sans_jp".to_owned());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "source_han_code_jp".to_owned());
    ctx.set_fonts(fonts);
}

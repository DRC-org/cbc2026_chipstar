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
use crate::machine::MachineProfile;
use crate::telemetry::{RunMode, Telemetry, slot_bit};

/// 画面（タブ）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Status,
    Config,
    Operation,
    Help,
    Test,
    Wiring,
    Parameters,
}

/// パラメータ画面の並び。用途ごとにまとめて、調整する塊が分かるようにする。
const PARAMETER_GROUPS: [(&str, u8, u8); 6] = [
    ("M3508 + C620（θ軸）", 0, 7),
    ("EL05（r軸・ドライバへ書き込む）", 8, 10),
    ("DM-S3519（z軸）", 11, 14),
    ("slotの絶対可動域（ネイティブ単位）", 15, 20),
    ("モータのCAN ID（SAFE中のみ変更可）", 21, 25),
    ("制御周期と監視 [ms / degC]", 26, 32),
];

const SCREENS: [(Screen, &str); 7] = [
    (Screen::Status, "1 ステータス"),
    (Screen::Config, "2 設定"),
    (Screen::Operation, "3 操作"),
    (Screen::Help, "4 ヘルプ"),
    (Screen::Test, "5 動作テスト"),
    (Screen::Wiring, "6 配線"),
    (Screen::Parameters, "7 パラメータ"),
];

/// 配線ガイドは docs/wiring.md を唯一の出典として埋め込む。
const WIRING_GUIDE: &str = include_str!("../../docs/wiring.md");

/// 操作画面で選べる項目。
#[derive(Clone, Copy)]
enum Operation {
    Run,
    Safe,
    Home,
    ToggleSlot(u8),
}

const OPERATIONS: [(Operation, &str); 6] = [
    (Operation::Run, "RUN（運転開始）"),
    (Operation::Safe, "SAFE（待機へ）"),
    (Operation::Home, "全slotの原点取り直し"),
    (
        Operation::ToggleSlot(slot_bit::SLOT0),
        "r 軸 (slot 0 / EL05) を切り替え",
    ),
    (
        Operation::ToggleSlot(slot_bit::SLOT1),
        "θ 軸 (slot 1 / M3508) を切り替え",
    ),
    (
        Operation::ToggleSlot(slot_bit::SLOT2),
        "z 軸 (slot 2 / DM) を切り替え",
    ),
];

/// 設定画面で編集できる項目数。
const CONFIG_FIELDS: usize = 3;

pub struct BridgeApp {
    test_panel: crate::fw_test_gui::Panel,
    shared: Arc<Shared>,
    screen: Screen,
    // 設定画面の編集バッファ（適用するまで共有設定へは反映しない）。
    edit_device: String,
    edit_baud: u32,
    edit_rate: f64,

    mode: Mode,
    normal: Normal,
    command_input: String,
    /// パラメータ画面の編集値。FWから読んだ値で初期化する。
    param_edit: Vec<f32>,
    param_seeded: bool,
    /// 直前の操作結果。画面下部に出す。
    message: Option<String>,
    /// 現在の画面での選択位置。
    selection: usize,
}

impl BridgeApp {
    pub fn new(shared: Arc<Shared>) -> Self {
        let cfg = shared.config();
        Self {
            test_panel: crate::fw_test_gui::Panel::new(cfg.serial_device.clone(), cfg.baud_rate),
            shared,
            screen: Screen::Status,
            edit_device: cfg.serial_device,
            edit_baud: cfg.baud_rate,
            edit_rate: cfg.rate_hz,
            mode: Mode::Normal,
            normal: Normal::default(),
            command_input: String::new(),
            param_edit: vec![0.0; crate::machine::PARAMETER_NAMES.len()],
            param_seeded: false,
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

        // 動作テスト画面には数値・接続先の入力欄がある。編集中だけはキーを奪わず、
        // Esc でフォーカスを外して通常のキー操作へ戻す。
        if self.screen == Screen::Test && ctx.egui_wants_keyboard_input() {
            if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
                ctx.memory_mut(|memory| memory.stop_text_input());
            }
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

        let enabled_slots = self
            .shared
            .status_snapshot()
            .telemetry
            .as_ref()
            .map(|telemetry| telemetry.enabled_slots);

        let command = match *operation {
            Operation::Run => Command::Run,
            Operation::Safe => Command::Safe,
            Operation::Home => Command::Home {
                slots: slot_bit::ALL,
            },
            Operation::ToggleSlot(bit) => {
                // テレメトリが無い間は有効化する方向に倒す。
                let currently_enabled = enabled_slots.is_some_and(|slots| slots & bit != 0);
                Command::Enable {
                    slots: bit,
                    enabled: !currently_enabled,
                }
            }
        };

        if self.shared.queue_command(command) {
            // RUNしても動かない典型的な原因を、その場で指摘する。
            let mut missing = Vec::new();
            if command == Command::Run {
                if !self.shared.sending_enabled() {
                    missing.push("コントローラ入力の送信が無効です");
                }
                if enabled_slots == Some(0) {
                    missing.push("有効な軸がありません");
                }
            }
            self.message = Some(if missing.is_empty() {
                format!("{label} → {}", command.to_line())
            } else {
                format!("RUN を送りましたが動きません: {}。", missing.join("、"))
            });
        } else {
            self.message = Some("FWの能力確認を待っています。".to_owned());
        }
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
                match line.to_ascii_uppercase().as_str() {
                    "STOP" => {
                        self.shared.queue_command(Command::Stop);
                    }
                    "SAFE" => {
                        self.shared.queue_command(Command::Safe);
                    }
                    "RUN" => {
                        self.shared.queue_command(Command::Run);
                    }
                    _ => self.shared.queue_line(line),
                }
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

                if self.shared.config().machine.requires_serial_svmd() {
                    ui.label("serial_svmd");
                    ui.label(if status.serial_servos.is_empty() {
                        "cctl の FDCAN2 経由（応答なし）".to_owned()
                    } else {
                        status
                            .serial_servos
                            .values()
                            .map(|servo| {
                                format!(
                                    "ID{}={}{}",
                                    servo.id,
                                    servo.position,
                                    if servo.enabled { "" } else { "(トルクOFF)" }
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("  ")
                    });
                    ui.end_row();
                }

                ui.label("デバイス");
                ui.label(match &status.device {
                    Some(device) => format!("{} / protocol {}", device.board, device.protocol),
                    None => "能力確認待ち".to_owned(),
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
        if let Some(encoder) = &status.dcmd_encoder {
            ui.monospace(format!(
                "DCMD ENC1 count={} index={}",
                encoder.count, encoder.index_count
            ));
        }
        if let Some(dcmd) = &status.dcmd {
            ui.monospace(format!(
                "DCMD mode={} enabled={} duty={:?} result={}",
                dcmd.mode, dcmd.enabled, dcmd.duty, dcmd.result
            ));
        }
        match &status.telemetry {
            Some(telemetry) => telemetry_ui(
                ui,
                telemetry,
                &self.shared.config().machine,
                status.telemetry_count,
            ),
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
        if self.shared.tests.enabled() {
            ui.label(
                "動作テスト中は通常操作を停止しています。「5 動作テスト」で終了してください。",
            );
            if ui.button("テスト出力STOP").clicked() {
                self.shared.queue_command(Command::Stop);
            }
            return;
        }
        ui.heading("操作");
        ui.separator();

        let status = self.shared.status_snapshot();
        match &status.telemetry {
            Some(telemetry) => {
                ui.colored_label(
                    mode_color(telemetry.mode),
                    egui::RichText::new(telemetry.mode.label()).strong(),
                );
                if telemetry.stale_slots != 0 {
                    ui.colored_label(
                        egui::Color32::from_rgb(200, 60, 60),
                        format!(
                            "モータの応答が途絶えました（slot mask {:03b}）。\
                             CAN配線と電源を確認し、再度有効化してください。",
                            telemetry.stale_slots
                        ),
                    );
                }
            }
            None => {
                ui.label("機体の状態は不明（テレメトリ未受信）");
            }
        }

        ui.add_space(4.0);
        ui.label("j / k で選び、Enter で実行。Space はいつでも STOP。");
        ui.add_space(8.0);

        let enabled_slots = status
            .telemetry
            .as_ref()
            .map(|telemetry| telemetry.enabled_slots);

        // RUN中に有効な軸がないと、指令が出ずフィードバックも返らない。
        // 「動かない」ときの原因として一番多いので、常時見えるようにする。
        if let Some(telemetry) = &status.telemetry
            && telemetry.mode == RunMode::Run
            && telemetry.enabled_slots == 0
        {
            ui.colored_label(
                egui::Color32::from_rgb(200, 140, 0),
                "RUN 中ですが有効な軸がありません。動かす軸を有効にしてください。",
            );
        }

        let mut clicked = None;

        for (index, (operation, label)) in OPERATIONS.iter().enumerate() {
            let selected = index == self.selection;
            let suffix = match operation {
                Operation::ToggleSlot(bit) => match enabled_slots {
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
        self.origin_ui(ui, &status);

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

    /// 原点出し。スイッチのある軸はジョグで当てれば自動で採用され、
    /// スイッチのない軸は姿勢を合わせてからボタンで採用する。
    fn origin_ui(&mut self, ui: &mut egui::Ui, status: &crate::app_state::Status) {
        ui.label(egui::RichText::new("原点").strong());
        if status.origins.is_empty() {
            ui.label("軸が設定されていません。");
            return;
        }
        if status
            .telemetry
            .as_ref()
            .is_none_or(|telemetry| telemetry.contacts.is_none())
        {
            ui.colored_label(
                egui::Color32::from_rgb(200, 140, 0),
                "接点の状態が届いていません。cctlのFWが古い可能性があります。",
            );
        }

        for (index, origin) in status.origins.iter().enumerate() {
            ui.horizontal(|ui| {
                let (mark, color) = if origin.captured {
                    ("採用済み", egui::Color32::from_rgb(0, 150, 0))
                } else if origin.lost {
                    ("要再設定", egui::Color32::from_rgb(200, 60, 60))
                } else {
                    ("未採用", egui::Color32::from_rgb(200, 140, 0))
                };
                ui.colored_label(color, format!("{:<6}{mark}", origin.name));
                ui.label(format!("{:8.2} {}", origin.position, origin.unit));
                match origin.at_limit {
                    Some(true) => {
                        ui.colored_label(egui::Color32::from_rgb(200, 60, 60), "リミット到達")
                    }
                    Some(false) => ui.label("リミット手前"),
                    None => ui.label("スイッチなし"),
                };
                if ui
                    .button("現在位置を原点に")
                    .on_hover_text("いまの姿勢にこの軸の原点位置を割り当てます。")
                    .clicked()
                {
                    self.shared.request_origin(index);
                }
            });
        }
        if status.origins.iter().any(|origin| origin.lost) {
            ui.colored_label(
                egui::Color32::from_rgb(200, 60, 60),
                "モータの電源が入り直したため原点が無効になりました。\
                 モータは電源投入時の姿勢を0とするので、採り直すまで可動域は信用できません。",
            );
        }
        ui.label("未採用の軸は可動域の制限が効きません。低速で当ててください。");

        ui.horizontal(|ui| {
            let mut soft_limits = self.shared.soft_limits();
            if ui
                .checkbox(&mut soft_limits, "可動域で止める")
                .on_hover_text(
                    "外すと、いまの原点から見た可動域の外へもジョグできます。原点を採り\
                     直す位置まで動かすときに使います。基板側のslot可動域は効いたままです。",
                )
                .changed()
            {
                self.shared.set_soft_limits(soft_limits);
            }
            if ui
                .button("モータを再初期化")
                .on_hover_text(
                    "モータ側の制御モードと速度・電流制限を入れ直します。モータの電源を\
                     入れ直したあとに実行してください。SAFEのときだけ受け付けます。",
                )
                .clicked()
            {
                self.shared.queue_line("REINIT 7".to_owned());
            }
        });
    }


    /// 実行時パラメータの表示と変更。cctlは変更を基板のFlashへ書き戻すので、
    /// 電源を入れ直しても詰めた値のまま立ち上がる。
    fn parameters_ui(&mut self, ui: &mut egui::Ui) {
        use crate::machine::PARAMETER_NAMES;
        ui.heading("実行時パラメータ");
        ui.label(
            "ゲイン・上限・CAN ID・周期を書き込みなしで変更します。変更はSAFEのあいだに\
             基板へ保存され、電源を入れ直しても残ります。config/rtheta.toml の \
             [parameters] は、保存が無いときの初期値として使われます。",
        );

        let status = self.shared.status_snapshot();
        match status.device.as_ref().map(|device| device.parameters_stored) {
            Some(true) => ui.label("基板は保存済みの値で動いています。"),
            Some(false) => ui.label("基板に保存はありません。機体プロファイルの値が入っています。"),
            None => ui.label("基板の状態が不明です。"),
        };
        if self.shared.tests.enabled() {
            ui.label("動作テスト中は変更できません。");
            return;
        }

        ui.horizontal(|ui| {
            if ui.button("FWから読み出す").clicked() {
                for id in 0..PARAMETER_NAMES.len() {
                    self.shared.queue_line(format!("PARAM {id}"));
                }
            }
            if ui
                .add_enabled(!status.parameters.is_empty(), egui::Button::new("読んだ値を編集欄へ"))
                .clicked()
            {
                for (id, value) in &status.parameters {
                    if let Some(slot) = self.param_edit.get_mut(usize::from(*id)) {
                        *slot = *value;
                    }
                }
            }
            ui.label(format!("読み出し済み: {} / {}", status.parameters.len(), PARAMETER_NAMES.len()));
        });
        if ui
            .button("保存を消して既定値に戻す")
            .on_hover_text(
                "基板の保存を消し、FWの既定値へ戻します。次の接続で機体プロファイルの\
                 値が入ります。SAFEのときだけ受け付けます。",
            )
            .clicked()
        {
            self.shared.queue_line("PARAMDEF".to_owned());
        }

        // 最初にFWの値が届いた時点で編集欄を埋める。毎フレーム上書きすると
        // 入力中の値が戻ってしまうので一度だけにする。
        if !self.param_seeded && status.parameters.len() == PARAMETER_NAMES.len() {
            for (id, value) in &status.parameters {
                if let Some(slot) = self.param_edit.get_mut(usize::from(*id)) {
                    *slot = *value;
                }
            }
            self.param_seeded = true;
        }

        let safe = status
            .telemetry
            .as_ref()
            .is_none_or(|telemetry| telemetry.mode == RunMode::Safe);

        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (title, first, last) in PARAMETER_GROUPS {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(title).strong());
                let needs_safe = first == 21;
                if needs_safe && !safe {
                    ui.label("SAFE でないため変更できません。");
                }
                for id in first..=last {
                    let index = usize::from(id);
                    let Some(name) = PARAMETER_NAMES.get(index) else {
                        continue;
                    };
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("{id:>2} {name:<24}")).monospace());
                        ui.add(
                            egui::DragValue::new(&mut self.param_edit[index])
                                .speed(0.05)
                                .max_decimals(5),
                        );
                        let enabled = !needs_safe || safe;
                        if ui.add_enabled(enabled, egui::Button::new("送信")).clicked() {
                            self.shared
                                .queue_line(format!("PARAM {id} {:.5}", self.param_edit[index]));
                            self.shared.queue_line(format!("PARAM {id}"));
                        }
                        match status.parameters.get(&id) {
                            Some(value) => ui.label(format!("FW: {value:.5}")),
                            None => ui.label("FW: 未読"),
                        };
                    });
                }
            }
        });
    }

    fn help_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("キー操作");
        ui.separator();

        const ROWS: [(&str, &str); 14] = [
            ("Space", "STOP を送る。どの画面でも効く"),
            ("j / k", "選択を下 / 上へ"),
            ("gg / G", "選択を先頭 / 末尾へ"),
            ("Enter", "選択中の項目を実行"),
            ("gt / gT", "次 / 前の画面へ"),
            ("1 - 7", "画面を直接選ぶ"),
            ("i", "設定画面で編集を始める"),
            ("Esc", "編集・コマンドラインを抜ける"),
            (
                "",
                "動作テスト画面では入力欄の編集中だけキーが入力になる。Esc で戻る",
            ),
            (":", "コマンドラインを開く"),
            ("?", "この画面"),
            (":q", "終了"),
            (":h", "この画面"),
            (":<指令>", "cctl へそのまま送る（例 :ENABLE 2 1）"),
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
        ui.monospace(":STOP   :RUN   :SAFE   :HOME 7   :ENABLE 7 1");
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
                Screen::Test => self.test_panel.ui(ui, &self.shared),
                Screen::Wiring => wiring_ui(ui),
                Screen::Parameters => self.parameters_ui(ui),
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

/// 配線ガイド。docs/wiring.md を見出し・表・図に分けて描くだけの簡易表示。
fn wiring_ui(ui: &mut egui::Ui) {
    ui.heading("配線ガイド");
    ui.label("出典は docs/wiring.md。回路図から起こした想定配線です。");
    ui.separator();

    let mut in_block = false;
    for raw in WIRING_GUIDE.lines() {
        if raw.starts_with("```") {
            in_block = !in_block;
            continue;
        }
        if in_block {
            ui.monospace(raw);
            continue;
        }
        let line = plain_text(raw);
        if raw.starts_with('|') {
            ui.monospace(line);
        } else if let Some(text) = line.strip_prefix("### ") {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(text).strong());
        } else if let Some(text) = line.strip_prefix("## ") {
            ui.add_space(10.0);
            ui.heading(text);
            ui.separator();
        } else if line.starts_with("# ") {
            // 文書題名は画面見出しと重複するので出さない。
        } else if line.trim().is_empty() {
            ui.add_space(4.0);
        } else {
            ui.label(line);
        }
    }
}

/// 強調とリンク記法を落として読み文にする。
fn plain_text(line: &str) -> String {
    let mut out = line.replace("**", "");
    while let Some(open) = out.find('[') {
        let Some(close) = out[open..].find("](") else {
            break;
        };
        let Some(end) = out[open + close..].find(')') else {
            break;
        };
        let label = out[open + 1..open + close].to_owned();
        out.replace_range(open..open + close + end + 1, &label);
    }
    out
}

fn mode_color(mode: RunMode) -> egui::Color32 {
    match mode {
        RunMode::Safe => egui::Color32::from_rgb(200, 170, 60),
        RunMode::Run => egui::Color32::from_rgb(90, 180, 90),
        RunMode::Stop => egui::Color32::from_rgb(200, 80, 80),
    }
}

/// 目標値と実測値を並べて表示する。
fn telemetry_ui(ui: &mut egui::Ui, telemetry: &Telemetry, profile: &MachineProfile, count: u64) {
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

            for axis in &profile.axes {
                let state = telemetry.slots[axis.slot as usize];
                let target = state.target / axis.native_per_unit;
                let measured = state.measured / axis.native_per_unit;
                let enabled = telemetry.slot_enabled(axis.slot);
                ui.label(format!("{} {}", axis.name, if enabled { "●" } else { "○" }));
                ui.monospace(format!("{target:+8.3} {}", axis.unit));
                ui.monospace(format!("{measured:+8.3} {}", axis.unit));
                ui.monospace(format!("{:+8.3}", target - measured));
                ui.end_row();
            }
        });

    ui.add_space(4.0);
    ui.monospace(format!(
        "uptime: {} ms / err: {} / 受信: {} 行",
        telemetry.uptime_ms,
        telemetry
            .error_bits
            .iter()
            .map(|bits| format!("{bits:02X}"))
            .collect::<Vec<_>>()
            .join(","),
        count
    ));
    for (slot, bits) in telemetry.error_bits.iter().enumerate() {
        let mut causes = Vec::new();
        if bits & crate::telemetry::error_bit::FEEDBACK_LOST != 0 {
            causes.push("応答途絶");
        }
        if bits & crate::telemetry::error_bit::OVER_TEMPERATURE != 0 {
            causes.push("過熱");
        }
        let driver = bits & !(crate::telemetry::error_bit::FEEDBACK_LOST
            | crate::telemetry::error_bit::OVER_TEMPERATURE);
        if driver != 0 {
            causes.push("ドライバ異常");
        }
        if !causes.is_empty() {
            ui.colored_label(
                egui::Color32::from_rgb(200, 60, 60),
                format!("slot {slot}: {}（err={bits:02X}）", causes.join(" / ")),
            );
        }
    }
    if telemetry.error_bits.iter().any(|bits| *bits != 0) {
        ui.colored_label(
            egui::Color32::from_rgb(200, 80, 80),
            "アクチュエータがエラーを報告しています。",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wiring_guide_is_embedded_and_covers_every_board() {
        for board in ["cctl", "serial_svmd", "DCMD", "svmd"] {
            assert!(WIRING_GUIDE.contains(board), "{board} が配線ガイドにない");
        }
        assert!(WIRING_GUIDE.contains("FDCAN2"));
    }

    #[test]
    fn parameter_groups_cover_every_id_exactly_once() {
        // 表示漏れや重複があると、調整できない値が残る。
        let mut seen = vec![false; crate::machine::PARAMETER_NAMES.len()];
        for (_, first, last) in PARAMETER_GROUPS {
            for id in first..=last {
                let index = usize::from(id);
                assert!(index < seen.len(), "id {id} は名前表にない");
                assert!(!seen[index], "id {id} が重複している");
                seen[index] = true;
            }
        }
        assert!(seen.iter().all(|covered| *covered), "表示されないidがある");
    }

    #[test]
    fn strips_markdown_emphasis_and_links() {
        assert_eq!(plain_text("**J12**（USB CDC）"), "J12（USB CDC）");
        assert_eq!(
            plain_text("詳細は [board_dcmd.md](board_dcmd.md) を参照"),
            "詳細は board_dcmd.md を参照"
        );
        assert_eq!(plain_text("素の行"), "素の行");
    }
}

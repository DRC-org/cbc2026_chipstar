//! eframe/egui ベースの GUI。
//!
//! 左のナビゲーションで「ステータス / 設定 / 操作 / その他」の各画面を切り替える。
//! 各画面の詳細は今後実装するため、ここでは骨組みと共有状態への接続のみ用意する。

use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{self, FontData};

use crate::app_state::{BridgeConfig, Shared};
use crate::frame::Command;
use crate::telemetry::{axis_bit, RunMode, Telemetry};

/// 画面（タブ）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Status,
    Config,
    Operation,
    Other,
}

const SCREENS: [(Screen, &str); 4] = [
    (Screen::Status, "ステータス"),
    (Screen::Config, "設定"),
    (Screen::Operation, "操作"),
    (Screen::Other, "その他"),
];

pub struct BridgeApp {
    shared: Arc<Shared>,
    screen: Screen,
    // 設定画面の編集バッファ（適用するまで共有設定へは反映しない）。
    edit_device: String,
    edit_baud: u32,
    edit_rate: f64,
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
        }
    }

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
                .map(|v| format!("{v:+.2}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        ui.monospace(format!(
            "buttons: [{}]",
            status
                .buttons
                .iter()
                .map(|b| b.to_string())
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

        egui::Grid::new("config_grid")
            .num_columns(2)
            .spacing([16.0, 8.0])
            .show(ui, |ui| {
                ui.label("シリアルデバイス");
                ui.add(egui::TextEdit::singleline(&mut self.edit_device).desired_width(240.0));
                ui.end_row();

                ui.label("ボーレート");
                ui.add(
                    egui::DragValue::new(&mut self.edit_baud)
                        .speed(100)
                        .range(1200..=1_000_000),
                );
                ui.end_row();

                ui.label("送信周期 [Hz]");
                ui.add(
                    egui::DragValue::new(&mut self.edit_rate)
                        .speed(1.0)
                        .range(1.0..=200.0),
                );
                ui.end_row();
            });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("適用").clicked() {
                self.shared.set_config(BridgeConfig {
                    serial_device: self.edit_device.clone(),
                    baud_rate: self.edit_baud,
                    rate_hz: self.edit_rate,
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

        // 非常停止。押した瞬間に送るため、送信停止中でも効く。
        let stop = egui::Button::new(egui::RichText::new("STOP").size(22.0).strong())
            .fill(egui::Color32::from_rgb(160, 40, 40))
            .min_size(egui::vec2(200.0, 56.0));
        if ui.add(stop).clicked() {
            self.shared.queue_command(Command::Stop);
        }
        ui.label("全軸のトルクを切ります。送信を停止していても届きます。");

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button("RUN（運転開始）").clicked() {
                self.shared.queue_command(Command::Run);
            }
            if ui.button("SAFE（待機へ）").clicked() {
                self.shared.queue_command(Command::Safe);
            }
            if ui.button("HOME（原点取り直し）").clicked() {
                self.shared.queue_command(Command::Home);
            }
        });

        ui.add_space(12.0);
        ui.label("軸ごとの有効・無効");
        ui.label("1 軸ずつ確かめながら立ち上げるために使います。");
        let enabled_axes = status.telemetry.as_ref().map(|t| t.enabled_axes);
        for (bit, label) in [
            (axis_bit::R, "r 軸 (EL05)"),
            (axis_bit::THETA, "θ 軸 (M3508)"),
            (axis_bit::Z, "z 軸 (DM)"),
        ] {
            ui.horizontal(|ui| {
                ui.label(label);
                if ui.button("有効").clicked() {
                    self.shared.queue_command(Command::Enable {
                        axes: bit,
                        enabled: true,
                    });
                }
                if ui.button("無効").clicked() {
                    self.shared.queue_command(Command::Enable {
                        axes: bit,
                        enabled: false,
                    });
                }
                ui.label(match enabled_axes {
                    Some(axes) if axes & bit != 0 => "現在: 有効",
                    Some(_) => "現在: 無効",
                    None => "現在: 不明",
                });
            });
        }

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button("テスト動作 開始").clicked() {
                self.shared.queue_command(Command::Test(true));
            }
            if ui.button("テスト動作 停止").clicked() {
                self.shared.queue_command(Command::Test(false));
            }
        });

        ui.add_space(12.0);
        ui.separator();
        let mut sending = self.shared.sending_enabled();
        if ui.checkbox(&mut sending, "コントローラ入力の送信を有効化").changed() {
            self.shared.set_sending_enabled(sending);
        }
    }

    fn other_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("その他");
        ui.separator();
        ui.label("この画面の内容は今後実装します。");
    }
}

impl eframe::App for BridgeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // 状態を定期的に反映するため再描画を予約する。
        ui.ctx().request_repaint_after(Duration::from_millis(100));

        egui::Panel::left("nav")
            .resizable(false)
            .exact_size(140.0)
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.heading("host");
                ui.separator();
                for (screen, label) in SCREENS {
                    ui.selectable_value(&mut self.screen, screen, label);
                }
            });

        egui::CentralPanel::default().show(ui, |ui| match self.screen {
            Screen::Status => self.status_ui(ui),
            Screen::Config => self.config_ui(ui),
            Screen::Operation => self.operation_ui(ui),
            Screen::Other => self.other_ui(ui),
        });
    }

    fn on_exit(&mut self) {
        self.shared.request_stop();
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

/// 目標値と実測値を並べて表示する。
fn telemetry_ui(ui: &mut egui::Ui, telemetry: &Telemetry, count: u64) {
    let mode_color = match telemetry.mode {
        RunMode::Safe => egui::Color32::from_rgb(200, 170, 60),
        RunMode::Run => egui::Color32::from_rgb(90, 180, 90),
        RunMode::Stop => egui::Color32::from_rgb(200, 80, 80),
    };
    ui.colored_label(mode_color, egui::RichText::new(telemetry.mode.label()).strong());

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

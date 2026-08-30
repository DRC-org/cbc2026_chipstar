//! eframe/egui ベースの GUI。
//!
//! 左のナビゲーションで「ステータス / 設定 / 操作 / その他」の各画面を切り替える。
//! 各画面の詳細は今後実装するため、ここでは骨組みと共有状態への接続のみ用意する。

use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{self, FontData};

use crate::app_state::{BridgeConfig, Shared};

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

        let mut sending = self.shared.sending_enabled();
        if ui.checkbox(&mut sending, "送信を有効化").changed() {
            self.shared.set_sending_enabled(sending);
        }

        ui.add_space(8.0);
        if ui.button("緊急停止（送信停止）").clicked() {
            self.shared.set_sending_enabled(false);
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

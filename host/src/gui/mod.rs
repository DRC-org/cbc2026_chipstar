//! 操縦、調整、診断。操作はローカルAPIと同じ受付を通す。
use crate::{
    application::app_state::Shared, interface::control_api::Request, machine::MachineProfile,
};
use eframe::egui::{self, Color32, FontData, RichText};
use std::{sync::Arc, time::Duration};

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Operate,
    Tune,
    Diagnose,
}
pub struct BridgeApp {
    shared: Arc<Shared>,
    screen: Screen,
    edit: MachineProfile,
    source: String,
    base: String,
    message: String,
    connection: crate::application::app_state::Connection,
}
impl BridgeApp {
    pub fn new(shared: Arc<Shared>) -> Self {
        let edit = shared.config().machine;
        let source = toml::to_string_pretty(&edit).unwrap_or_default();
        let config = shared.config();
        let connection = crate::application::app_state::Connection {
            serial_device: config.serial_device,
            baud_rate: config.baud_rate,
        };
        Self {
            connection,
            shared,
            screen: Screen::Operate,
            edit,
            base: source.clone(),
            source,
            message: String::new(),
        }
    }
    fn request(&mut self, request: Request) {
        let reply = self.shared.submit(request, true);
        self.message = if reply.ok && !reply.data.is_empty() {
            reply.data
        } else {
            reply.message
        };
    }
    fn operation(&mut self, action: &str) {
        self.request(Request::new(action));
    }
    fn reload(&mut self) {
        self.edit = self.shared.config().machine;
        self.source = toml::to_string_pretty(&self.edit).unwrap_or_default();
        self.base = self.source.clone();
    }
}
impl eframe::App for BridgeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.ctx().request_repaint_after(Duration::from_millis(50));
        let status = self.shared.status_snapshot();
        let color = if status.ai_active {
            Color32::from_rgb(220, 48, 48)
        } else {
            ui.visuals().window_fill
        };
        egui::Frame::new()
            .stroke(egui::Stroke::new(
                if status.ai_active { 5.0 } else { 0.0 },
                color,
            ))
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                ui.horizontal(|ui| {
                    ui.heading("キャチロボクワガタ");
                    if status.simulated {
                        ui.colored_label(Color32::YELLOW, "模擬接続");
                    }
                    if status.ai_active {
                        ui.colored_label(color, "AI操作中");
                    }
                });
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.screen, Screen::Operate, "操縦");
                    ui.selectable_value(&mut self.screen, Screen::Tune, "調整");
                    ui.selectable_value(&mut self.screen, Screen::Diagnose, "診断");
                    ui.separator();
                    if ui
                        .add_sized(
                            [160.0, 42.0],
                            egui::Button::new(RichText::new("停止・保持").size(21.0))
                                .fill(Color32::from_rgb(150, 35, 35)),
                        )
                        .clicked()
                    {
                        self.operation("stop");
                    }
                    if ui
                        .add_enabled(
                            !status.ai_active && !status.running,
                            egui::Button::new("運転再開").min_size(egui::vec2(100.0, 42.0)),
                        )
                        .clicked()
                    {
                        self.operation("run");
                    }
                });
                if !status.error.is_empty() {
                    ui.colored_label(Color32::LIGHT_RED, &status.error);
                }
                if !self.message.is_empty() {
                    ui.label(&self.message);
                }
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| match self.screen {
                    Screen::Operate => self.operate(ui),
                    Screen::Tune => {
                        ui.add_enabled_ui(!status.ai_active, |ui| self.tune(ui));
                    }
                    Screen::Diagnose => self.diagnose(ui),
                });
            });
    }
    fn on_exit(&mut self) {
        self.shared.request_stop();
    }
}
fn badge(ui: &mut egui::Ui, text: &str, good: bool) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.colored_label(
            if good {
                Color32::LIGHT_GREEN
            } else {
                Color32::YELLOW
            },
            text,
        );
    });
}
pub fn install_japanese_font(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "noto_sans_jp".into(),
        FontData::from_static(include_bytes!("../../assets/NotoSansJP-Regular.ttf")).into(),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "noto_sans_jp".into());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "noto_sans_jp".into());
    ctx.set_fonts(fonts);
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(Color32::from_rgb(220, 224, 230));
    ctx.set_visuals(visuals);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(16.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(16.0));
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    });
}

mod diagnose;
mod operate;
mod tune;

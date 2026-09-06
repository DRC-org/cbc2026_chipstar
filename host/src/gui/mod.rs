//! 操縦、調整、診断。ボタンとキー操作は共通の受付を通す。
use crate::{
    application::{
        app_state::{Shared, Status},
        command::Request,
    },
    machine::MachineProfile,
};
use eframe::egui::{self, Color32, FontData, RichText};
use shortcuts::Action;
use std::{sync::Arc, time::Duration};

const BG: Color32 = Color32::from_rgb(13, 19, 28);
const SURFACE: Color32 = Color32::from_rgb(22, 31, 43);
const BORDER: Color32 = Color32::from_rgb(43, 57, 74);
const MUTED: Color32 = Color32::from_rgb(148, 166, 186);
const ACCENT: Color32 = Color32::from_rgb(91, 219, 186);
const WARNING: Color32 = Color32::from_rgb(240, 193, 107);
const DANGER: Color32 = Color32::from_rgb(246, 113, 117);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Screen {
    Operate,
    Tune,
    Diagnose,
    Documents,
}

pub struct BridgeApp {
    shared: Arc<Shared>,
    screen: Screen,
    edit: MachineProfile,
    source: String,
    base: String,
    message: String,
    message_error: bool,
    help_open: bool,
    stop_requested: bool,
    log_filter: String,
    documents: documents::Documents,
    profile_file: String,
    profile_file_base: String,
    requested_jog: Option<(String, f32)>,
    active_jog: Option<String>,
    vim: shortcuts::Vim,
    navigation: Option<Action>,
    connection: crate::application::app_state::Connection,
}
impl BridgeApp {
    pub fn new(shared: Arc<Shared>) -> Self {
        let config = shared.config();
        let edit = config.machine;
        let source = toml::to_string_pretty(&edit).unwrap_or_default();
        Self {
            connection: crate::application::app_state::Connection {
                serial_device: config.serial_device,
                baud_rate: config.baud_rate,
                simulate: Some(config.simulate),
            },
            shared,
            screen: Screen::Operate,
            edit,
            base: source.clone(),
            source,
            message: String::new(),
            message_error: false,
            help_open: false,
            stop_requested: false,
            log_filter: String::new(),
            documents: documents::Documents::new(),
            profile_file: config.profile_path.display().to_string(),
            profile_file_base: config.profile_path.display().to_string(),
            requested_jog: None,
            active_jog: None,
            vim: shortcuts::Vim::default(),
            navigation: None,
        }
    }
    fn request(&mut self, request: Request) {
        let reply = self.shared.submit(request, true);
        self.message_error = !reply.ok;
        self.message = if reply.ok && !reply.data.is_empty() {
            reply.data
        } else if reply.ok {
            "操作を受け付けました".into()
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
    fn draft_matches_applied(&self) -> bool {
        MachineProfile::parse(&self.source)
            .is_ok_and(|profile| profile == self.shared.config().machine)
    }
    fn can_apply(&self, status: &Status) -> bool {
        !status.ai_active
            && !status.running
            && toml::to_string_pretty(&self.shared.config().machine).unwrap_or_default()
                == self.base
    }
    fn can_save(&self, status: &Status) -> bool {
        !status.ai_active && !status.running && self.draft_matches_applied()
    }
    fn can_run(status: &Status) -> bool {
        !status.ai_active && !status.running && status.connected && status.configured
    }
    fn dispatch(&mut self, action: Action) {
        let status = self.shared.status_snapshot();
        match action {
            Action::Stop => {
                self.stop_requested = true;
                self.operation("stop");
            }
            Action::Run if !self.stop_requested && Self::can_run(&status) => self.operation("run"),
            Action::Screen(screen) => self.screen = screen,
            Action::Tab(direction) => {
                let screens = [
                    Screen::Operate,
                    Screen::Tune,
                    Screen::Diagnose,
                    Screen::Documents,
                ];
                let index = screens
                    .iter()
                    .position(|screen| *screen == self.screen)
                    .unwrap_or(0);
                self.screen = screens[(index as i32 + direction).rem_euclid(4) as usize];
            }
            Action::Scroll(_) | Action::Edge(_) => self.navigation = Some(action),
            Action::Help => self.help_open = !self.help_open,
            Action::Apply if self.screen == Screen::Tune && self.can_apply(&status) => {
                match MachineProfile::parse(&self.source) {
                    Ok(profile) => {
                        self.request(Request {
                            text: Some(self.source.clone()),
                            ..Request::new("apply")
                        });
                        if self.shared.config().machine == profile {
                            self.reload();
                        }
                    }
                    Err(error) => {
                        self.message = error.to_string();
                        self.message_error = true;
                    }
                }
            }
            Action::Save if self.screen == Screen::Tune && self.can_save(&status) => {
                self.request(Request {
                    text: Some(self.profile_file.clone()),
                    ..Request::new("save")
                });
            }
            _ => {}
        }
    }
    fn header(&mut self, ui: &mut egui::Ui, status: &Status) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("キャチロボクワガタ").size(22.0).strong());
                ui.horizontal(|ui| {
                    ui.label(RichText::new("CONTROL STATION").size(10.0).color(MUTED));
                    if status.simulated {
                        chip(ui, "模擬接続", WARNING);
                    }
                    if status.ai_active {
                        chip(ui, "AI操作中", DANGER);
                    }
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_sized(
                        [170.0, 48.0],
                        egui::Button::new(RichText::new("停止・保持  Esc").strong())
                            .fill(Color32::from_rgb(115, 39, 48)),
                    )
                    .clicked()
                {
                    self.dispatch(Action::Stop);
                }
                if ui
                    .add_enabled(
                        Self::can_run(status),
                        egui::Button::new("運転再開")
                            .min_size(egui::vec2(120.0, 48.0))
                            .fill(Color32::from_rgb(27, 80, 74)),
                    )
                    .on_hover_text("Ctrl+Enter / Options\n原点と入力中立を確認して再開")
                    .clicked()
                {
                    self.dispatch(Action::Run);
                }
            });
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            for (screen, label) in [
                (Screen::Operate, "F1   操縦"),
                (Screen::Tune, "F2   調整"),
                (Screen::Diagnose, "F3   診断"),
                (Screen::Documents, "F4   文書"),
            ] {
                let selected = self.screen == screen;
                if ui
                    .add_sized(
                        [110.0, 42.0],
                        egui::Button::new(RichText::new(label).color(if selected {
                            ACCENT
                        } else {
                            MUTED
                        }))
                        .fill(if selected { SURFACE } else { BG })
                        .stroke(if selected {
                            egui::Stroke::new(1.0, BORDER)
                        } else {
                            egui::Stroke::new(1.0, BG)
                        }),
                    )
                    .clicked()
                {
                    self.screen = screen;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_sized([140.0, 42.0], egui::Button::new("キー操作  F12"))
                    .clicked()
                {
                    self.dispatch(Action::Help);
                }
                if status.ai_active
                    && ui
                        .add_sized([170.0, 42.0], egui::Button::new("通常操縦へ戻す"))
                        .on_hover_text(
                            "停止・保持してAIの操作権を解除します。運転は自動再開しません。",
                        )
                        .clicked()
                {
                    self.operation("takeover");
                }
            });
        });
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);
    }
    fn help(&mut self, ctx: &egui::Context) {
        egui::Window::new("キーボード操作")
            .open(&mut self.help_open)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                egui::Grid::new("shortcuts")
                    .spacing([28.0, 12.0])
                    .show(ui, |ui| {
                        for (key, action) in [
                            ("Esc / Space", "停止・保持"),
                            ("Ctrl+Enter", "運転再開"),
                            ("F1 / F2 / F3 / F4", "操縦 / 調整 / 診断 / 文書"),
                            ("h / l", "前 / 次のタブ"),
                            ("j / k", "下 / 上へスクロール"),
                            ("Ctrl+d / Ctrl+u", "下 / 上へ半ページ移動"),
                            ("gg / Shift+g", "ページの先頭 / 末尾"),
                            ("Ctrl+Shift+Enter", "一時適用（調整画面）"),
                            ("Ctrl+S", "適用中の設定を保存（調整画面）"),
                            ("F12", "この案内を開閉"),
                        ] {
                            keycap(ui, key);
                            ui.label(action);
                            ui.end_row();
                        }
                    });
                ui.add_space(12.0);
                ui.label(RichText::new("文字・数値の入力中はEscのみ有効です。").color(MUTED));
                ui.label(
                    RichText::new("保存する前に、編集中の設定を一時適用してください。")
                        .color(MUTED),
                );
            });
    }
}
impl eframe::App for BridgeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.ctx().request_repaint_after(Duration::from_millis(50));
        self.stop_requested = false;
        self.requested_jog = None;
        let current_path = self.shared.config().profile_path.display().to_string();
        if self.profile_file == self.profile_file_base {
            self.profile_file = current_path.clone();
        }
        self.profile_file_base = current_path;
        let editing = ui.ctx().text_edit_focused();
        let action = ui.input(|input| {
            if input.focused {
                if let Some(action) = shortcuts::resolve(&input.events, editing) {
                    self.vim.clear();
                    Some(action)
                } else {
                    self.vim
                        .resolve(&input.events, editing || self.help_open, input.time)
                }
            } else {
                self.vim.clear();
                None
            }
        });
        if let Some(action) = action {
            ui.input_mut(|input| {
                input
                    .events
                    .retain(|event| !matches!(event, egui::Event::Key { pressed: true, .. }))
            });
            self.dispatch(action);
        }
        let status = self.shared.status_snapshot();
        egui::Frame::new()
            .fill(BG)
            .stroke(egui::Stroke::new(
                if status.ai_active { 4.0 } else { 0.0 },
                DANGER,
            ))
            .inner_margin(20.0)
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                self.header(ui, &status);
                if !status.error.is_empty() {
                    ui.colored_label(DANGER, &status.error);
                    ui.add_space(6.0);
                }
                egui::ScrollArea::vertical()
                    .id_salt(format!("page-{:?}", self.screen))
                    .max_height((ui.available_height() - 34.0).max(120.0))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let top = ui.cursor().min;
                        if let Some(Action::Scroll(amount)) = self.navigation {
                            let amount = if amount.is_infinite() {
                                amount.signum() * ui.clip_rect().height() * 0.5
                            } else {
                                amount
                            };
                            ui.scroll_with_delta(egui::vec2(0.0, -amount));
                        }
                        match self.screen {
                            Screen::Operate => self.operate(ui),
                            Screen::Tune => {
                                ui.add_enabled_ui(!status.ai_active, |ui| self.tune(ui));
                            }
                            Screen::Diagnose => self.diagnose(ui),
                            Screen::Documents => self.documents.show(ui),
                        }
                        if let Some(Action::Edge(start)) = self.navigation.take() {
                            let point = if start { top } else { ui.min_rect().max };
                            ui.scroll_to_rect(
                                egui::Rect::from_min_size(point, egui::Vec2::ZERO),
                                Some(if start {
                                    egui::Align::TOP
                                } else {
                                    egui::Align::BOTTOM
                                }),
                            );
                        }
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(if status.connected {
                            "● 接続済み"
                        } else {
                            "○ 接続待ち"
                        })
                        .size(12.0)
                        .color(if status.connected {
                            ACCENT
                        } else {
                            MUTED
                        }),
                    );
                    ui.label(
                        RichText::new(format!(
                            "応答 {} ms  ·  {}",
                            status.telemetry_age_ms,
                            if status.configured {
                                "設定一致"
                            } else {
                                "設定確認中"
                            }
                        ))
                        .size(12.0)
                        .color(MUTED),
                    );
                    if !self.message.is_empty() {
                        ui.separator();
                        if ui.small_button("閉じる").clicked() {
                            self.message.clear();
                        }
                        ui.add(
                            egui::Label::new(
                                RichText::new(&self.message)
                                    .size(12.0)
                                    .color(if self.message_error { DANGER } else { ACCENT }),
                            )
                            .truncate(),
                        )
                        .on_hover_text(&self.message);
                    }
                });
            });
        self.update_screen_input(ui.ctx());
        self.help(ui.ctx());
    }
    fn on_exit(&mut self) {
        self.shared.request_stop();
    }
}
fn panel() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .corner_radius(10)
        .inner_margin(14.0)
}
fn chip(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.12))
        .corner_radius(5)
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(12.0).color(color));
        });
}
fn keycap(ui: &mut egui::Ui, text: &str) {
    egui::Frame::new()
        .fill(BG)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(7, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(12.0).color(MUTED));
        });
}
fn section(ui: &mut egui::Ui, title: &str, description: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(title).size(20.0).strong());
        if !description.is_empty() {
            ui.label(RichText::new(description).size(13.0).color(MUTED));
        }
    });
    ui.add_space(8.0);
}
pub fn install_japanese_font(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "noto_sans_jp".into(),
        FontData::from_static(include_bytes!("../../assets/NotoSansJP-Regular.ttf")).into(),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "noto_sans_jp".into());
    }
    ctx.set_fonts(fonts);
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(Color32::from_rgb(230, 238, 246));
    visuals.panel_fill = BG;
    visuals.window_fill = SURFACE;
    visuals.extreme_bg_color = BG;
    visuals.selection.bg_fill = Color32::from_rgb(34, 89, 84);
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, BORDER);
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(32, 44, 59);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(45, 65, 82);
    ctx.set_visuals(visuals);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 8.0);
        style.spacing.interact_size.y = 30.0;
    });
}
mod diagnose;
mod operate;
mod shortcuts;
mod tune;

mod manual;

mod documents;

mod parameter_help;

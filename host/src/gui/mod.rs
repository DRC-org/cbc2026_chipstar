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
    tune_view: tune::TuneView,
    tune_axis: String,
    tune_ee_axis: String,
    diagnosis_view: diagnose::DiagnosisView,
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
    page_offsets: [f32; 4],
    page_heights: [f32; 4],
    command_open: bool,
    emergency_edit_guard: bool,
    previous_emergency: bool,
    command_focus: bool,
    command_text: String,
    tests: individual::TestPanel,
    sts_ui: sts::StsPanel,
    pid_plot: pid_plot::Panel,
    homing_confirmed: bool,
    homing_timeout: f32,
    ee_values: std::collections::BTreeMap<String, f32>,
    connection: crate::application::app_state::Connection,
}
impl BridgeApp {
    pub fn new(shared: Arc<Shared>) -> Self {
        let config = shared.config();
        let edit = config.machine;
        let tune_axis = edit
            .axes
            .first()
            .map(|axis| axis.name.clone())
            .unwrap_or_default();
        let source = toml::to_string_pretty(&edit).unwrap_or_default();
        Self {
            connection: crate::application::app_state::Connection {
                serial_device: config.serial_device,
                baud_rate: config.baud_rate,
                simulate: Some(config.simulate),
            },
            shared,
            screen: Screen::Operate,
            tune_view: tune::TuneView::Axes,
            tune_axis,
            tune_ee_axis: "ee_rotation".into(),
            diagnosis_view: diagnose::DiagnosisView::Tests,
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
            page_offsets: [0.0; 4],
            page_heights: [0.0; 4],
            command_open: false,
            emergency_edit_guard: false,
            previous_emergency: false,
            command_focus: false,
            command_text: String::new(),
            tests: individual::TestPanel::default(),
            sts_ui: sts::StsPanel::default(),
            pid_plot: pid_plot::Panel::default(),
            homing_confirmed: false,
            homing_timeout: 180.0,
            ee_values: std::collections::BTreeMap::new(),
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
        !status.emergency
            && status.homing.is_none()
            && !status.sts.active
            && !status.sts.busy
            && !status.test_active
            && !status.ai_active
            && !status.running
            && toml::to_string_pretty(&self.shared.config().machine).unwrap_or_default()
                == self.base
    }
    fn can_save(&self, status: &Status) -> bool {
        !status.emergency
            && status.homing.is_none()
            && !status.sts.active
            && !status.sts.busy
            && !status.test_active
            && !status.ai_active
            && !status.running
            && self.draft_matches_applied()
    }
    fn can_run(status: &Status) -> bool {
        !status.emergency
            && status.homing.is_none()
            && !status.sts.active
            && !status.sts.busy
            && !status.test_mode
            && !status.ai_active
            && !status.running
            && status.connected
            && status.configured
    }
    fn end_test_on_tab_change(&mut self) {
        let status = self.shared.status_snapshot();
        if status.sts.active || status.sts.busy {
            self.dispatch(Action::Stop);
        }
        if self.shared.status_snapshot().test_mode {
            self.stop_requested = true;
            self.tests.requested = false;
            self.request(Request {
                flag: Some(false),
                ..Request::new("test_mode")
            });
        }
    }
    fn switch_screen(&mut self, screen: Screen) {
        if self.screen != screen {
            self.end_test_on_tab_change();
            self.screen = screen;
        }
    }
    fn dispatch(&mut self, action: Action) {
        let status = self.shared.status_snapshot();
        match action {
            Action::Emergency(engage) => {
                if !engage {
                    self.emergency_edit_guard = false;
                }
                self.stop_requested = true;
                self.command_open = false;
                self.operation(if engage { "estop" } else { "estop_reset" });
            }
            Action::Escape => {
                self.emergency_edit_guard = false;
                self.help_open = false;
                self.command_open = false;
                self.command_text.clear();
            }
            Action::Command => {
                self.command_open = true;
                self.command_focus = true;
                self.command_text.clear();
            }

            Action::Stop => {
                self.stop_requested = true;
                self.operation("stop");
            }
            Action::Run if !self.stop_requested && Self::can_run(&status) => self.operation("run"),
            Action::Recover => self.operation("recover"),
            Action::Screen(screen) => self.switch_screen(screen),
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
                self.switch_screen(screens[(index as i32 + direction).rem_euclid(4) as usize]);
            }
            Action::Scroll(_) | Action::Page(_) | Action::Edge(_) => self.navigation = Some(action),
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
            Action::Apply | Action::Save => {
                self.message = "調整画面で停止し、編集・適用状態を確認してください".into();
                self.message_error = true;
            }
            Action::Run => {
                self.message = "接続・設定・緊停・個別テストの状態を確認してください".into();
                self.message_error = true;
            }
        }
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
                            ("Space", "ソフト緊停 / 解除（再開は別操作）"),
                            ("s / :stop", "停止・保持（個別テストは出力解除）"),
                            (":run", "運転再開"),
                            ("1 / 2 / 3 / 4", "操縦 / 調整 / 診断 / 文書"),
                            ("h / l ・ gT / gt", "前 / 次のタブ"),
                            ("j / k", "下 / 上へスクロール"),
                            ("Ctrl+d / Ctrl+u", "下 / 上へ半ページ移動"),
                            ("gg / G", "ページの先頭 / 末尾"),
                            (":apply / :w", "調整画面で適用 / 適用済み設定を保存"),
                            ("Esc", "編集終了 / コマンド取消"),
                            ("?", "この案内を開閉"),
                        ] {
                            keycap(ui, key);
                            ui.label(action);
                            ui.end_row();
                        }
                    });
                ui.add_space(12.0);
                ui.label(
                    RichText::new("編集中の文字入力を優先します。出力中のSpaceは緊停です。")
                        .color(MUTED),
                );
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
        self.tests.requested = false;
        let current_path = self.shared.config().profile_path.display().to_string();
        if self.profile_file == self.profile_file_base {
            self.profile_file = current_path.clone();
        }
        self.profile_file_base = current_path;
        let editing = ui.ctx().text_edit_focused() || self.command_open;
        let shortcut_status = self.shared.status_snapshot();
        if shortcut_status.emergency && !self.previous_emergency && editing {
            self.emergency_edit_guard = true;
        }
        self.previous_emergency = shortcut_status.emergency;
        if !editing && ui.input(|input| input.pointer.any_pressed()) {
            self.emergency_edit_guard = false;
        }
        let action = ui.input(|input| {
            if input.focused {
                if let Some(action) = shortcuts::resolve(
                    &input.events,
                    editing || (shortcut_status.emergency && self.emergency_edit_guard),
                    shortcut_status.outputs_active,
                    shortcut_status.emergency,
                ) {
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
            if action == Action::Emergency(true) && editing {
                self.emergency_edit_guard = true;
            }
            if action == Action::Escape {
                ui.ctx().memory_mut(|memory| {
                    if let Some(id) = memory.focused() {
                        memory.surrender_focus(id);
                    }
                });
            }
            ui.input_mut(|input| {
                input.events.retain(|event| {
                    !matches!(event, egui::Event::Key { pressed: true, .. })
                        && !matches!(event, egui::Event::Text(_))
                });
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
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                self.header(ui, &status);
                self.global_state(ui, &status);
                self.page_navigation(ui);
                self.command_line(ui);
                if !status.error.is_empty() || self.message_error || (status.connected && !status.configured) {
                    ui.horizontal_wrapped(|ui| {
                        if !status.error.is_empty() {
                            ui.colored_label(DANGER, &status.error);
                        }
                        if ui.add_enabled(
                            status.connected && !status.emergency && !status.ai_active
                                && !status.running && !status.outputs_active,
                            egui::Button::new("設定を再送して確認"),
                        ).on_hover_text("出力停止のまま設定を再送・照合します。緊停やモータ異常は解除しません")
                            .clicked() {
                            self.operation("recover");
                        }
                    });
                    ui.add_space(6.0);
                }
                let page = self.screen as usize;
                let height = (ui.available_height() - 34.0).max(120.0);
                let mut scroll = egui::ScrollArea::vertical()
                    .id_salt(format!("page-{:?}", self.screen))
                    .max_height(height)
                    .auto_shrink([false, false]);
                if let Some(action) = self.navigation.take() {
                    let offset = match action {
                        Action::Scroll(delta) => self.page_offsets[page] + delta,
                        Action::Page(fraction) => self.page_offsets[page] + fraction * height,
                        Action::Edge(true) => 0.0,
                        Action::Edge(false) => self.page_heights[page],
                        _ => self.page_offsets[page],
                    };
                    scroll = scroll.vertical_scroll_offset(offset.max(0.0));
                }
                let output = scroll.show(ui, |ui| match self.screen {
                    Screen::Operate => self.operate(ui),
                    Screen::Tune => {
                        ui.add_enabled_ui(!status.ai_active, |ui| self.tune(ui));
                    }
                    Screen::Diagnose => self.diagnose(ui),
                    Screen::Documents => self.documents.show(ui),
                });
                self.page_offsets[page] = output.state.offset.y;
                self.page_heights[page] = output.content_size.y;
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
        self.update_test_input(ui.ctx());
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
    ui.label(RichText::new(title).size(20.0).strong());
    if !description.is_empty() {
        ui.label(RichText::new(description).size(13.0).color(MUTED));
    }
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
mod ee;
mod operate;
mod pid;
mod pid_plot;
mod shortcuts;
mod sts;
mod tune;

mod controls;
mod individual;
mod manual;

mod documents;
mod gamepad;

mod parameter_help;

mod shell;

#[cfg(test)]
mod workflow_tests {
    use super::*;
    use crate::{
        application::{app_state::BridgeConfig, sts, worker},
        diagnostics::individual::Kind,
    };
    use std::{
        thread,
        time::{Duration, Instant},
    };

    struct Harness {
        app: BridgeApp,
        shared: Arc<Shared>,
        worker: Option<thread::JoinHandle<()>>,
    }

    impl Harness {
        fn new() -> Self {
            let shared = Arc::new(Shared::new(BridgeConfig {
                serial_device: "unused".into(),
                baud_rate: 115200,
                rate_hz: 100.0,
                machine: MachineProfile::embedded().unwrap(),
                profile_path: "/dev/null".into(),
                simulate: true,
            }));
            let worker_shared = shared.clone();
            let worker = thread::spawn(move || worker::run(worker_shared));
            let start = Instant::now();
            while !shared.status_snapshot().configured {
                assert!(start.elapsed() < Duration::from_secs(5));
                thread::sleep(Duration::from_millis(10));
            }
            let app = BridgeApp::new(shared.clone());
            Self {
                app,
                shared,
                worker: Some(worker),
            }
        }

        fn wait(&self, predicate: impl Fn(&Status) -> bool) -> Status {
            let start = Instant::now();
            loop {
                let status = self.shared.status_snapshot();
                if predicate(&status) {
                    return status;
                }
                assert!(start.elapsed() < Duration::from_secs(5), "state timeout");
                thread::sleep(Duration::from_millis(10));
            }
        }

        fn capture_origins(&mut self) {
            for axis in ["r", "theta", "z"] {
                self.app.request(Request {
                    axis: Some(axis.into()),
                    ..Request::new("origin")
                });
                assert!(!self.app.message_error, "{}", self.app.message);
            }
            self.wait(|status| status.origins.iter().all(|origin| origin.captured));
        }

        fn run(&mut self) {
            self.app.stop_requested = false;
            self.app.dispatch(Action::Run);
            assert!(!self.app.message_error, "{}", self.app.message);
            self.wait(|status| status.running);
        }
    }

    impl Drop for Harness {
        fn drop(&mut self) {
            self.shared.request_stop();
            if let Some(worker) = self.worker.take() {
                worker.join().unwrap();
            }
        }
    }

    #[test]
    fn gui_emergency_reset_keeps_outputs_stopped_until_explicit_run() {
        let mut harness = Harness::new();
        harness.capture_origins();
        harness.run();

        harness.app.dispatch(Action::Emergency(true));
        let emergency = harness.wait(|status| status.emergency && !status.outputs_active);
        assert!(!emergency.running);
        assert!(emergency.origins.iter().all(|origin| origin.captured));

        harness.app.dispatch(Action::Emergency(false));
        let reset = harness.wait(|status| !status.emergency);
        assert!(!reset.running && !reset.outputs_active);
        assert!(reset.origins.iter().all(|origin| origin.captured));
        thread::sleep(Duration::from_millis(100));
        assert!(!harness.shared.status_snapshot().running);

        harness.run();
    }

    #[test]
    fn gui_navigation_keeps_normal_run_but_ends_both_individual_test_modes() {
        let mut harness = Harness::new();
        harness.capture_origins();
        harness.run();
        harness.app.switch_screen(Screen::Documents);
        assert!(harness.shared.status_snapshot().running);
        harness.app.dispatch(Action::Stop);
        harness.wait(|status| !status.running);

        for (kind, value) in [(Kind::Velocity, 0.1), (Kind::Position, 0.0)] {
            harness.app.select_cctl_test(1);
            harness.app.request(Request {
                axis: Some("cctl:1".into()),
                text: Some(kind.key().into()),
                ..Request::new("test_select")
            });
            harness.app.request(Request {
                value: Some(value),
                flag: (kind == Kind::Position).then_some(true),
                ..Request::new("test_output")
            });
            harness.wait(|status| status.test_active);

            if kind == Kind::Velocity {
                harness
                    .app
                    .switch_diagnosis_view(diagnose::DiagnosisView::Connection);
            } else {
                harness.app.switch_screen(Screen::Documents);
            }
            let stopped = harness.wait(|status| !status.test_mode && !status.test_active);
            assert!(!stopped.outputs_active);
        }
    }

    #[test]
    fn gui_diagnosis_navigation_stops_sts_output() {
        let mut harness = Harness::new();
        harness.app.screen = Screen::Diagnose;
        harness.app.diagnosis_view = diagnose::DiagnosisView::Sts;
        let operation = sts::Operation::Move {
            targets: vec![sts::Target::default()],
        };
        harness.app.request(Request {
            text: Some(toml::to_string(&operation).unwrap()),
            ..Request::new("sts")
        });
        harness.wait(|status| status.sts.active);

        harness
            .app
            .switch_diagnosis_view(diagnose::DiagnosisView::Connection);
        let stopped = harness.wait(|status| !status.sts.active && !status.sts.busy);
        assert!(!stopped.outputs_active);
    }

    #[test]
    fn gui_emergency_stops_individual_output_and_tab_exit_keeps_latch() {
        let mut harness = Harness::new();
        harness.capture_origins();
        harness.app.select_cctl_test(1);
        harness.app.request(Request {
            axis: Some("cctl:1".into()),
            text: Some(Kind::Position.key().into()),
            ..Request::new("test_select")
        });
        harness.app.request(Request {
            value: Some(0.0),
            flag: Some(true),
            ..Request::new("test_output")
        });
        harness.wait(|status| status.test_active);

        harness.app.dispatch(Action::Emergency(true));
        let emergency = harness.wait(|status| status.emergency && !status.test_active);
        assert!(!emergency.outputs_active);
        assert!(emergency.test_mode);
        harness.app.switch_screen(Screen::Documents);
        let exited = harness.wait(|status| !status.test_mode);
        assert!(exited.emergency);
        assert!(!exited.outputs_active && !exited.running);
    }

    #[test]
    fn gui_emergency_stops_sts_and_reset_does_not_restart_it() {
        let mut harness = Harness::new();
        let operation = sts::Operation::Move {
            targets: vec![sts::Target::default()],
        };
        harness.app.request(Request {
            text: Some(toml::to_string(&operation).unwrap()),
            ..Request::new("sts")
        });
        harness.wait(|status| status.sts.active);

        harness.app.dispatch(Action::Emergency(true));
        harness.wait(|status| status.emergency && !status.sts.active && !status.outputs_active);
        harness.app.dispatch(Action::Emergency(false));
        let reset = harness.wait(|status| !status.emergency);
        assert!(!reset.sts.active && !reset.sts.busy);
        assert!(!reset.outputs_active && !reset.running);
    }
}

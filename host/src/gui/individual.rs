use super::*;
use crate::diagnostics::individual::{Kind, Target};

pub(super) struct TestPanel {
    board: &'static str,
    id: u8,
    kind: Kind,
    value: f32,
    pub requested: bool,
    held: bool,
}
impl Default for TestPanel {
    fn default() -> Self {
        Self {
            board: "cctl",
            id: 0,
            kind: Kind::Velocity,
            value: 0.0,
            requested: false,
            held: false,
        }
    }
}
impl BridgeApp {
    pub(super) fn select_cctl_test(&mut self, slot: u8) {
        self.end_test_on_tab_change();
        self.screen = Screen::Diagnose;
        self.diagnosis_view = diagnose::DiagnosisView::Tests;
        self.tests.board = "cctl";
        self.tests.id = slot;
        self.tests.kind = Kind::Velocity;
        self.tests.value = 0.0;
        self.request(Request {
            flag: Some(true),
            ..Request::new("test_mode")
        });
        self.request(Request {
            axis: Some(format!("cctl:{slot}")),
            text: Some("velocity".into()),
            ..Request::new("test_select")
        });
    }

    pub(super) fn select_ee_test(&mut self, target: Target, value: f32) {
        self.end_test_on_tab_change();
        self.screen = Screen::Diagnose;
        self.diagnosis_view = diagnose::DiagnosisView::Tests;
        self.tests.board = target.board();
        self.tests.id = match target {
            Target::Pwm(id) | Target::Sts(id) => id,
            _ => return,
        };
        self.tests.kind = Kind::Position;
        self.tests.value = value;
        self.request(Request {
            flag: Some(true),
            ..Request::new("test_mode")
        });
        self.request(Request {
            axis: Some(target.key()),
            text: Some("position".into()),
            ..Request::new("test_select")
        });
    }
    pub(super) fn individual_test(&mut self, ui: &mut egui::Ui, status: &Status) {
        ui.horizontal_wrapped(|ui| {
            ui.label("EE単体テスト");
            for axis in crate::machine::ee::axes(&self.shared.config().machine) {
                if ui
                    .add_enabled(
                        !status.ai_active && !status.emergency,
                        egui::Button::new(axis.label),
                    )
                    .clicked()
                {
                    self.select_ee_test(axis.target, axis.initial);
                }
            }
        });
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("個別テスト").strong());
                if ui.add_enabled(!status.emergency && !status.ai_active,
                    egui::Button::new(if status.test_mode { "個別テストを終了" } else { "通常操縦を停止してテストに入る" })).clicked() {
                    self.request(Request { flag: Some(!status.test_mode), ..Request::new("test_mode") });
                }
            });
            ui.label(RichText::new("センサ・接点は下の受信状態で確認できます。出力は1対象ずつ操作します。").size(12.0).color(MUTED));
            if !status.test_mode { return; }
            ui.add_enabled_ui(!status.emergency && !status.ai_active, |ui| {
                let mut changed = false;
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("test-board").selected_text(self.tests.board).show_ui(ui, |ui| {
                        for (key, label) in [("cctl", "cctl モータ"), ("pwm", "PWMサーボ"), ("sts", "STS3215"), ("dc", "DCモータ")] {
                            if ui.selectable_value(&mut self.tests.board, key, label).changed() {
                                self.tests.id = if key == "sts" { 1 } else { 0 };
                                self.tests.kind = if key == "dc" { Kind::Duty } else if key == "cctl" { Kind::Velocity } else { Kind::Position };
                                self.tests.value = if key == "pwm" { 1500.0 } else if key == "sts" { 2048.0 } else { 0.0 };
                                changed = true;
                            }
                        }
                    });
                    ui.label("チャネル / ID");
                    let range = match self.tests.board { "cctl" => 0..=2, "pwm" => 0..=3, "sts" => 1..=253, _ => 0..=0 };
                    changed |= ui.add(egui::DragValue::new(&mut self.tests.id).range(range)).changed();
                    if self.tests.board == "cctl" {
                        changed |= ui.selectable_value(&mut self.tests.kind, Kind::Velocity, "速度").changed();
                        changed |= ui.selectable_value(&mut self.tests.kind, Kind::Position, "位置").changed();
                    }
                });
                let key = format!("{}:{}", self.tests.board, self.tests.id);
                let target = Target::parse(&key).expect("GUI target range");
                if let Ok((min, max, unit)) = target.limits(self.tests.kind, &self.shared.config().machine) {
                    if changed || status.test_target.is_empty() {
                        self.tests.value = self.tests.value.clamp(min, max);
                        self.request(Request { axis: Some(key.clone()), text: Some(self.tests.kind.key().into()), ..Request::new("test_select") });
                    }
                    ui.label(RichText::new(format!("指令範囲 {min:.3} .. {max:.3} {unit}")).size(12.0).color(MUTED));
                    if matches!(target, Target::Cctl(_)) {
                        ui.label(RichText::new("モータ側の座標・単位で指定します。位置テストには機体座標の可動域制限を適用しません。").size(12.0).color(WARNING));
                    }
                    ui.horizontal_wrapped(|ui| {
                        ui.label("指令値");
                        ui.add(egui::DragValue::new(&mut self.tests.value).speed(if matches!(target, Target::Cctl(_)) { 0.01 } else { 1.0 }).range(min..=max).suffix(format!(" {unit}")));
                        let can_output = status.test_ready && status.connected && status.configured && !self.stop_requested && status.test_target == key && status.test_kind == self.tests.kind.key();
                        if self.tests.kind.momentary() {
                            let response = ui.add_enabled(can_output, egui::Button::new("押している間だけ出力").min_size(egui::vec2(200.0, 40.0)));
                            self.tests.requested = can_output && response.is_pointer_button_down_on();
                        } else if ui.add_enabled(can_output, egui::Button::new("指定位置へ移動・保持")).clicked() {
                            self.request(Request { value: Some(self.tests.value), flag: Some(true), ..Request::new("test_output") });
                        }
                        if ui.button("出力解除").clicked() { self.dispatch(Action::Stop); }
                    });
                    ui.label(RichText::new(if self.tests.kind.momentary() {
                        "ボタンを離すか入力更新が150ms途切れると出力解除します。"
                    } else {
                        "タブを切り替えると位置保持を解除し、個別テストを終了します。"
                    }).size(12.0).color(MUTED));
                } else {
                    ui.colored_label(WARNING, "この対象の軸設定を読み込んでください。");
                }
            });
        });
    }
    pub(super) fn update_test_input(&mut self, ctx: &egui::Context) {
        let status = self.shared.status_snapshot();
        let held = self.tests.requested
            && ctx.input(|input| input.focused)
            && !self.stop_requested
            && !status.emergency
            && status.test_mode
            && !status.ai_active;
        if held {
            let reply = self.shared.submit(
                Request {
                    value: Some(self.tests.value),
                    ..Request::new("test_output")
                },
                true,
            );
            if !reply.ok {
                self.message = reply.message;
                self.message_error = true;
            }
        }
        if !held && self.tests.held && status.test_active {
            self.operation("stop");
        }
        // 停止やエラーで出力が切れても、物理的に離すまでは再試行を許さない。
        self.tests.held |= held;
        if self.tests.held && !ctx.input(|input| input.pointer.primary_down()) {
            self.operation("test_off");
            self.tests.held = false;
        }
    }
}

//! 全画面に共通する運転操作、状態、ページ選択。
use super::*;

impl BridgeApp {
    pub(super) fn header(&mut self, ui: &mut egui::Ui, status: &Status) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("キャチロボクワガタ").size(20.0).strong());
            if status.simulated {
                chip(ui, "模擬接続", WARNING);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_sized(
                        [170.0, 38.0],
                        egui::Button::new(if status.emergency {
                            "緊停解除  Space"
                        } else {
                            "ソフト緊停  Space"
                        })
                        .fill(Color32::from_rgb(115, 39, 48)),
                    )
                    .clicked()
                {
                    self.dispatch(Action::Emergency(!status.emergency));
                }
                if ui
                    .add_sized(
                        [150.0, 38.0],
                        egui::Button::new(if status.test_mode {
                            "テスト出力解除  s"
                        } else {
                            "停止・保持  s"
                        }),
                    )
                    .clicked()
                {
                    self.dispatch(Action::Stop);
                }
                if ui
                    .add_enabled(
                        Self::can_run(status),
                        egui::Button::new("運転再開")
                            .min_size(egui::vec2(100.0, 38.0))
                            .fill(Color32::from_rgb(27, 80, 74)),
                    )
                    .on_hover_text(":run / Options\n原点と入力中立を確認して再開")
                    .clicked()
                {
                    self.dispatch(Action::Run);
                }
            });
        });
        ui.add_space(4.0);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), 40.0),
            egui::Layout::left_to_right(egui::Align::Min),
            |ui| {
                for (screen, label) in [
                    (Screen::Operate, "1  操縦"),
                    (Screen::Tune, "2  調整"),
                    (Screen::Diagnose, "3  診断"),
                    (Screen::Documents, "4  文書"),
                ] {
                    let selected = self.screen == screen;
                    if ui
                        .add_sized(
                            [100.0, 40.0],
                            egui::Button::new(RichText::new(label).color(if selected {
                                ACCENT
                            } else {
                                MUTED
                            }))
                            .fill(if selected { SURFACE } else { BG })
                            .stroke(egui::Stroke::new(1.0, if selected { BORDER } else { BG })),
                        )
                        .clicked()
                    {
                        self.switch_screen(screen);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("キー操作  ?").clicked() {
                        self.dispatch(Action::Help);
                    }
                    if status.ai_active {
                        chip(ui, "AI操作中", DANGER);
                        if ui
                            .button("通常操縦へ戻す")
                            .on_hover_text(
                                "停止・保持してAIの操作権を解除。運転は自動再開しません。",
                            )
                            .clicked()
                        {
                            self.operation("takeover");
                        }
                    }
                });
            },
        );
    }

    pub(super) fn global_state(&mut self, ui: &mut egui::Ui, status: &Status) {
        egui::Frame::new().fill(if status.emergency {
            Color32::from_rgb(115, 39, 48)
        } else { SURFACE }).corner_radius(6).inner_margin(8.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&status.operating_state).strong()
                    .color(if status.running { ACCENT } else { Color32::WHITE }));
                if status.test_mode {
                    chip(ui, "個別テスト", WARNING);
                    ui.label(format!("{} · {}",
                        if status.test_target.is_empty() { "対象未選択" } else { &status.test_target },
                        if status.test_active { "出力要求中" } else { "出力停止" }));
                } else if !status.emergency {
                    ui.add(egui::Label::new(RichText::new(&status.reason).size(13.0).color(MUTED))
                        .truncate()).on_hover_text(&status.reason);
                }
                if status.origin_adjustment {
                    chip(ui, "原点調整 · 可動域制限解除", WARNING);
                }
            });
            if status.emergency {
                ui.label(if !status.connected {
                    "接続断のため出力状態を確認できません。緊停状態を維持しています。"
                } else if self.emergency_edit_guard {
                    "全駆動出力を停止。Escで編集を終了してからSpace、または解除ボタンで解除できます。"
                } else {
                    "全駆動出力を停止しています。解除しても運転は再開しません。"
                });
            }
        });
        ui.add_space(8.0);
    }

    pub(super) fn page_navigation(&mut self, ui: &mut egui::Ui) {
        if !matches!(self.screen, Screen::Tune | Screen::Diagnose) {
            return;
        }
        let mut changed = false;
        ui.horizontal(|ui| match self.screen {
            Screen::Tune => {
                for (view, label) in [
                    (tune::TuneView::Axes, "軸・原点"),
                    (tune::TuneView::Pid, "PID調整"),
                    (tune::TuneView::Ee, "EE設定"),
                    (tune::TuneView::Parameters, "基板パラメータ"),
                    (tune::TuneView::File, "設定ファイル"),
                ] {
                    changed |= ui
                        .selectable_value(&mut self.tune_view, view, label)
                        .changed();
                }
            }
            Screen::Diagnose => {
                for (view, label) in [
                    (diagnose::DiagnosisView::Tests, "個別テスト"),
                    (diagnose::DiagnosisView::Sts, "STS管理"),
                    (diagnose::DiagnosisView::Connection, "接続・保守"),
                    (diagnose::DiagnosisView::Log, "通信ログ"),
                ] {
                    changed |= ui
                        .selectable_value(&mut self.diagnosis_view, view, label)
                        .changed();
                }
            }
            _ => {}
        });
        if changed {
            self.end_test_on_tab_change();
            self.navigation = Some(Action::Edge(true));
        }
        if self.screen == Screen::Tune {
            self.tune_actions(ui);
        }
    }
}

//! 全画面に共通する運転操作、状態、ページ選択。
use super::*;

impl BridgeApp {
    pub(super) fn header(&mut self, ui: &mut egui::Ui, status: &Status) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("キャチロボクワガタ").size(18.0).strong());
            chip(
                ui,
                &status.operating_state,
                if status.emergency {
                    DANGER
                } else if status.running {
                    ACCENT
                } else {
                    WARNING
                },
            );
            if status.simulated {
                chip(ui, "模擬接続", WARNING);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_sized(
                        [150.0, 34.0],
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
                        [130.0, 34.0],
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
                            .min_size(egui::vec2(92.0, 34.0))
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
            egui::vec2(ui.available_width(), 32.0),
            egui::Layout::left_to_right(egui::Align::Center),
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
                            [82.0, 30.0],
                            egui::Button::new(RichText::new(label).color(if selected {
                                ACCENT
                            } else {
                                MUTED
                            }))
                            .fill(if selected { SURFACE } else { BG })
                            .stroke(egui::Stroke::new(1.0, if selected { ACCENT } else { BG })),
                        )
                        .clicked()
                    {
                        self.switch_screen(screen);
                    }
                }
                ui.separator();
                ui.label(
                    RichText::new(if status.connected {
                        format!(
                            "接続済み · {} ms · {}",
                            status.telemetry_age_ms,
                            if status.configured {
                                "設定一致"
                            } else {
                                "設定確認中"
                            }
                        )
                    } else {
                        "機体との接続待ち".into()
                    })
                    .size(12.0)
                    .color(if status.connected { ACCENT } else { MUTED }),
                );
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
        if let Some(seconds) = status.homing_confirmation {
            ui.colored_label(
                WARNING,
                format!(
                    "Create長押し {:.1}/1.0秒：EEの向きと移動経路を確認済みなら保持してください",
                    seconds.min(1.0)
                ),
            );
        }
    }

    pub(super) fn global_state(&mut self, ui: &mut egui::Ui, status: &Status) {
        let detail = if status.emergency {
            Some(if !status.connected {
                "機体と通信できません。ソフト緊停を維持しています。"
            } else if self.emergency_edit_guard {
                "全出力を停止しました。Escで入力を終了してから緊停を解除できます。"
            } else {
                "全出力を停止しました。解除しても運転は再開しません。"
            })
        } else if let Some(homing) = status.homing.as_deref() {
            Some(homing)
        } else if status.test_mode {
            Some(if status.test_active {
                "個別テストで出力しています。画面を移動すると出力を解除します。"
            } else {
                "個別テスト中です。対象を選び、出力操作を行ってください。"
            })
        } else if status.origin_adjustment {
            Some("原点調整中：低速固定で、機体座標の可動域制限を解除しています。")
        } else if !status.running && !status.reason.is_empty() && status.reason != status.error {
            Some(status.reason.as_str())
        } else {
            None
        };
        let Some(detail) = detail else { return };
        egui::Frame::new()
            .fill(if status.emergency {
                Color32::from_rgb(115, 39, 48)
            } else {
                SURFACE
            })
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(8, 5))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    if status.test_mode {
                        chip(ui, "個別テスト", WARNING);
                    }
                    if status.origin_adjustment {
                        chip(ui, "原点調整", WARNING);
                    }
                    ui.add(
                        egui::Label::new(RichText::new(detail).size(13.0).color(
                            if status.emergency {
                                Color32::WHITE
                            } else {
                                MUTED
                            },
                        ))
                        .truncate(),
                    )
                    .on_hover_text(detail);
                });
            });
        ui.add_space(5.0);
    }

    pub(super) fn page_navigation(&mut self, ui: &mut egui::Ui) {
        if self.screen != Screen::Tune {
            return;
        }
        let mut changed = false;
        ui.horizontal(|ui| {
            for (view, label) in [
                (tune::TuneView::Axes, "アーム"),
                (tune::TuneView::Ee, "EE"),
                (tune::TuneView::Parameters, "基板"),
                (tune::TuneView::File, "設定ファイル"),
            ] {
                changed |= ui
                    .selectable_value(&mut self.tune_view, view, label)
                    .changed();
            }
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

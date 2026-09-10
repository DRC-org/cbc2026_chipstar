use super::*;

impl BridgeApp {
    pub(super) fn operate(&mut self, ui: &mut egui::Ui) {
        let status = self.shared.status_snapshot();
        if !self.preparation_panel(ui, &status) {
            return;
        }
        ui.add_space(8.0);
        self.dashboard(ui, &status);
    }

    pub(super) fn debug_dashboard(&mut self, ui: &mut egui::Ui) {
        let status = self.shared.status_snapshot();
        section(
            ui,
            "デバッグ",
            "軸の状態を見ながら、各機構の動作を確認します。",
        );
        if status.preparation.locked() {
            ui.colored_label(
                WARNING,
                "開始待ち中は表示のみ確認できます。操作する場合は準備画面に戻ってください。",
            );
        }
        ui.add_enabled_ui(!status.preparation.locked(), |ui| {
            self.dashboard(ui, &status);
            ui.collapsing("自動ホーミング", |ui| {
                self.homing_controls(ui, &status)
            });
        });
    }

    fn dashboard(&mut self, ui: &mut egui::Ui, status: &Status) {
        let config = self.shared.config();
        ui.horizontal(|ui| {
            ui.label(RichText::new("機体を操縦").size(20.0).strong());
            if status.slow {
                chip(
                    ui,
                    &format!("低速 {:.0}%", config.machine.slow_speed_percent),
                    ACCENT,
                );
            }
            ui.label(
                RichText::new(format!(
                    "原点確認  {} / {}",
                    status.origins.iter().filter(|axis| axis.captured).count(),
                    status.origins.len()
                ))
                .size(12.0)
                .color(MUTED),
            );
            if ui.small_button("アーム設定を開く").clicked() {
                self.tune_view = tune::TuneView::Axes;
                self.switch_screen(Screen::Tune);
            }
        });
        let mut test_slot = None;
        ui.columns(3, |columns| {
            for (i, axis) in status.origins.iter().enumerate() {
                let ui = &mut columns[i % 3];
                let slot = config
                    .machine
                    .axes
                    .iter()
                    .find(|profile| profile.name == axis.name)
                    .map(|profile| profile.slot);
                panel().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_min_height(104.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(if axis.name == "theta" {
                                "θ"
                            } else {
                                &axis.name
                            })
                            .size(24.0)
                            .color(ACCENT),
                        );
                        ui.label(
                            RichText::new(match axis.name.as_str() {
                                "r" => "アーム伸縮",
                                "theta" => "アーム旋回",
                                "z" => "昇降",
                                _ => "",
                            })
                            .size(12.0)
                            .color(MUTED),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if slot.is_some()
                                && ui
                                    .small_button("単体テスト")
                                    .on_hover_text("この軸を選択した状態で診断画面を開きます")
                                    .clicked()
                            {
                                test_slot = slot;
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(if status.connected {
                                format!("{:.1}", axis.position)
                            } else {
                                "—".into()
                            })
                            .size(36.0)
                            .strong(),
                        );
                        ui.label(RichText::new(&axis.unit).color(MUTED));
                    });
                    let color = if axis.captured { MUTED } else { WARNING };
                    ui.label(
                        RichText::new(if axis.captured {
                            "原点 採用済み"
                        } else if axis.lost {
                            "原点喪失 · 再確認が必要"
                        } else {
                            "原点未採用 · 暫定座標"
                        })
                        .size(12.0)
                        .color(color),
                    );
                    if axis.captured && status.connected {
                        if let Some(profile) = config
                            .machine
                            .axes
                            .iter()
                            .find(|profile| profile.name == axis.name)
                        {
                            let fraction = ((axis.position - profile.minimum)
                                / (profile.maximum - profile.minimum))
                                .clamp(0.0, 1.0);
                            ui.add(
                                egui::ProgressBar::new(fraction)
                                    .fill(ACCENT.gamma_multiply(0.65))
                                    .desired_height(4.0),
                            )
                            .on_hover_text(format!(
                                "設定可動域：{} 〜 {} {}",
                                profile.minimum, profile.maximum, axis.unit
                            ));
                        }
                    } else {
                        ui.add_space(4.0);
                    }
                    if axis.at_limit == Some(true) {
                        ui.colored_label(WARNING, "端点スイッチ作動中");
                    }
                });
            }
        });
        if let Some(slot) = test_slot {
            self.select_cctl_test(slot);
        }
        ui.add_space(8.0);
        self.operate_sequence(ui, status);
        ui.add_space(8.0);
        ui.add_enabled_ui(!status.sequence.active, |ui| {
            if ui.available_width() < 1000.0 {
                self.manual_controls(ui, status);
                ui.add_space(8.0);
                self.operate_ee(ui, status);
            } else {
                ui.columns(2, |columns| {
                    self.manual_controls(&mut columns[0], status);
                    self.operate_ee(&mut columns[1], status);
                });
            }
        });
        ui.add_space(8.0);
        self.operate_bonus(ui, status);
    }

    pub(super) fn homing_controls(&mut self, ui: &mut egui::Ui, status: &Status) {
        let Some(court) = status.court else {
            ui.colored_label(WARNING, "先に赤コートか青コートを選んでください。");
            return;
        };
        let config = self.shared.config();
        let rise = config
            .machine
            .axes
            .iter()
            .find(|a| a.name == "z")
            .map(|a| a.homing_retreat_mm())
            .unwrap_or(0.0);
        let retreat = config
            .machine
            .axes
            .iter()
            .find(|a| a.name == "r")
            .map(|a| a.homing_retreat_mm())
            .unwrap_or(0.0);
        ui.label(format!(
            "{}：正面をθ=0°として、z上昇後にθを{:+.0}°へ旋回します。",
            court.label(),
            court.homing_theta()
        ));
        ui.label(
            RichText::new(format!(
                "z下端で原点設定 → zを{rise:.0} mm上昇 → θ旋回 → r前端で原点設定 → rを{retreat:.0} mm後退・完了"
            ))
            .size(13.0)
            .color(MUTED),
        );
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            for (name, label) in [("theta", "θ"), ("z", "z"), ("r", "r")] {
                let captured = status
                    .origins
                    .iter()
                    .any(|axis| axis.name == name && axis.captured);
                chip(
                    ui,
                    &format!(
                        "{label}原点：{}",
                        if captured {
                            "設定済み"
                        } else {
                            "未設定"
                        }
                    ),
                    if captured { ACCENT } else { WARNING },
                );
            }
        });
        ui.add_space(8.0);
        if let Some(label) = &status.homing {
            ui.colored_label(ACCENT, label);
            if ui.button("ホーミングを中断する").clicked() {
                self.dispatch(Action::Stop);
            }
        } else {
            if ui
                .add_enabled(
                    status.homing_ready,
                    egui::Button::new("自動ホーミングを開始する"),
                )
                .clicked()
            {
                self.request(Request {
                    flag: Some(true),
                    value: Some(self.homing_timeout),
                    ..Request::new("home")
                });
            }
        }
        ui.add_space(6.0);
        ui.label(RichText::new("停止中にCreateを1秒長押ししても同じ動作を開始できます。rを後退して終了し、θ・zの保持を続けます。").size(12.0).color(MUTED));
    }
}

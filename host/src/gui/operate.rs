use super::*;

impl BridgeApp {
    pub(super) fn operate(&mut self, ui: &mut egui::Ui) {
        let status = self.shared.status_snapshot();
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
        if ui.available_width() < 1000.0 {
            self.manual_controls(ui, &status);
            ui.add_space(8.0);
            self.operate_ee(ui, &status);
        } else {
            ui.columns(2, |columns| {
                self.manual_controls(&mut columns[0], &status);
                self.operate_ee(&mut columns[1], &status);
            });
        }
    }

    pub(super) fn homing_controls(&mut self, ui: &mut egui::Ui, status: &Status) {
        ui.separator();
        let theta = self.shared.config().machine.axes.iter()
            .find(|axis| axis.name == "theta").cloned();
        if let Some(theta) = theta {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("θ原点（手動）").strong());
                let captured = status.origins.iter()
                    .any(|axis| axis.name == "theta" && axis.captured);
                chip(ui, if captured { "採用済み" } else { "未採用" },
                    if captured { ACCENT } else { WARNING });
                if ui.add_enabled(status.homing_ready,
                    egui::Button::new("θの現在位置を原点に採用"))
                    .on_hover_text(format!(
                        "現在のθ位置に {} {} を割り当てます。モータは移動しません。停止・保持中も採用できます。",
                        theta.origin_position, theta.unit))
                    .clicked() {
                    self.request(Request {
                        axis: Some("theta".into()),
                        ..Request::new("origin")
                    });
                }
                ui.label(format!("採用座標：{} {}", theta.origin_position, theta.unit));
            });
            ui.label(RichText::new(
                "EEをシューティングボックスの反対側に向けて採用してください。r・zホーミングの前後どちらでも操作できます。")
                .size(12.0).color(MUTED));
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("r・z原点").strong());
            if let Some(label) = &status.homing {
                chip(ui, label, ACCENT);
                if ui.button("中断").clicked() {
                    self.dispatch(Action::Stop);
                }
                return;
            }
            ui.checkbox(
                &mut self.homing_confirmed,
                "姿勢・経路を確認済み",
            )
            .on_hover_text(
                "EEがシューティングボックスの反対側を向き、z下降・設定量の上昇・r前進・設定量の後退の全経路に干渉がないことを確認してください",
            );
            let can_start = self.homing_confirmed && status.homing_ready;
            if ui
                .add_enabled(can_start, egui::Button::new("自動設定を開始"))
                .on_hover_text("zを下端へ移動した後、rを前端へ移動して原点座標を設定します")
                .clicked()
            {
                self.homing_confirmed = false;
                self.request(Request {
                    flag: Some(true),
                    value: Some(self.homing_timeout),
                    ..Request::new("home")
                });
            }
        });
        ui.label(
            RichText::new("z下端 → zを設定量上昇 → r前端 → rを設定量後退。DualSenseでは停止中にCreateを1秒長押し。")
                .size(12.0)
                .color(MUTED),
        );
    }
}

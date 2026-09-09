use super::*;

impl BridgeApp {
    pub(super) fn operate(&mut self, ui: &mut egui::Ui) {
        let status = self.shared.status_snapshot();
        if !self.preparation_panel(ui, &status) {
            return;
        }
        ui.add_space(8.0);
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
        self.operate_sequence(ui, &status);
        ui.add_space(8.0);
        ui.add_enabled_ui(!status.sequence.active, |ui| {
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
        });
    }

    pub(super) fn homing_controls(&mut self, ui: &mut egui::Ui, status: &Status) {
        let theta = self
            .shared
            .config()
            .machine
            .axes
            .iter()
            .find(|axis| axis.name == "theta")
            .cloned();
        if let Some(theta) = theta {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("旋回（θ）の原点").strong());
                let captured = status
                    .origins
                    .iter()
                    .any(|axis| axis.name == "theta" && axis.captured);
                chip(
                    ui,
                    if captured {
                        "設定済み"
                    } else {
                        "未設定"
                    },
                    if captured { ACCENT } else { WARNING },
                );
            });
            ui.label(
                RichText::new(format!(
                    "現在の向きを {} {} として記録します。この操作では機体は動きません。",
                    theta.origin_position, theta.unit
                ))
                .size(12.0)
                .color(MUTED),
            );
            if ui
                .add_enabled(
                    status.homing_ready,
                    egui::Button::new("現在の向きをθの原点にする"),
                )
                .clicked()
            {
                self.request(Request {
                    axis: Some("theta".into()),
                    ..Request::new("origin")
                });
            }
        }
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("伸縮（r）・昇降（z）の原点").strong());
            let captured = ["r", "z"].iter().all(|name| {
                status
                    .origins
                    .iter()
                    .any(|axis| axis.name == *name && axis.captured)
            });
            chip(
                ui,
                if captured {
                    "設定済み"
                } else {
                    "未設定"
                },
                if captured { ACCENT } else { WARNING },
            );
        });
        ui.label(RichText::new("zを下端まで下げてから上昇し、rを前端まで伸ばしてから戻します。移動経路に干渉がないことを確認してください。").size(12.0).color(MUTED));
        if let Some(label) = &status.homing {
            ui.colored_label(ACCENT, label);
            if ui.button("原点設定を中断する").clicked() {
                self.dispatch(Action::Stop);
            }
        } else {
            ui.checkbox(
                &mut self.homing_confirmed,
                "先端の向きと移動経路を確認しました",
            );
            if ui
                .add_enabled(
                    self.homing_confirmed && status.homing_ready,
                    egui::Button::new("r・zの原点設定を開始する"),
                )
                .clicked()
            {
                self.homing_confirmed = false;
                self.request(Request {
                    flag: Some(true),
                    value: Some(self.homing_timeout),
                    ..Request::new("home")
                });
            }
        }
        ui.add_space(6.0);
        ui.label(RichText::new("パッドで行う場合：停止中にCreateを1秒長押しすると、θの原点記録に続いてr・zの原点設定を開始します。").size(12.0).color(MUTED));
    }
}

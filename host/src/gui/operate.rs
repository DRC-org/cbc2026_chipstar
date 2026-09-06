use super::*;

impl BridgeApp {
    pub(super) fn operate(&mut self, ui: &mut egui::Ui) {
        let status = self.shared.status_snapshot();
        let config = self.shared.config();
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(if status.running {
                        "運転中"
                    } else {
                        "停止中"
                    })
                    .size(32.0)
                    .strong()
                    .color(if status.running {
                        ACCENT
                    } else {
                        Color32::WHITE
                    }),
                );
                if status.slow {
                    chip(ui, "低速 20%", ACCENT);
                }
                if status.origin_adjustment {
                    chip(ui, "原点調整", WARNING);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    chip(
                        ui,
                        if status.configured {
                            "設定一致"
                        } else {
                            "設定確認中"
                        },
                        if status.configured { ACCENT } else { WARNING },
                    );
                });
            });
            ui.add_space(6.0);
            ui.label(RichText::new(&status.reason).size(16.0).color(MUTED));
            if status.origin_adjustment {
                ui.add_space(6.0);
                ui.colored_label(WARNING, "機体座標の可動域制限を解除しています");
            }
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("軸の位置").size(17.0).strong());
            ui.label(
                RichText::new(format!(
                    "原点確認  {} / {}",
                    status.origins.iter().filter(|axis| axis.captured).count(),
                    status.origins.len()
                ))
                .size(12.0)
                .color(MUTED),
            );
        });
        ui.columns(3, |columns| {
            for (i, axis) in status.origins.iter().enumerate() {
                let ui = &mut columns[i % 3];
                panel().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_min_height(116.0);
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
                        ui.colored_label(WARNING, "リミット到達");
                    }
                });
            }
        });
        ui.add_space(12.0);
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("操縦ガイド").strong());
                ui.label(
                    RichText::new(if status.gamepad.is_empty() {
                        "DualSense 未接続"
                    } else {
                        &status.gamepad
                    })
                    .size(12.0)
                    .color(MUTED),
                );
            });
            ui.add_space(6.0);
            ui.columns(3, |columns| {
                for (ui, (title, key, detail)) in columns.iter_mut().zip([
                    ("移動", "左スティック / 右上下", "r・θ / z"),
                    ("低速", "L1 を保持", "通常速度の20%"),
                    ("再開 / 停止", "Options / PS", "PC：Ctrl+Enter / Esc"),
                ]) {
                    ui.label(RichText::new(title).size(12.0).color(MUTED));
                    keycap(ui, key);
                    ui.label(RichText::new(detail).size(12.0).color(MUTED));
                }
            });
        });
        if status.simulated {
            ui.add_space(12.0);
            ui.add_enabled_ui(!status.ai_active, |ui| {
                ui.collapsing("模擬スティック", |ui| {
                    for axis in config.machine.axes {
                        let mut value = axis.input_axis.map(|n| status.axes[n]).unwrap_or(0.0);
                        ui.horizontal(|ui| {
                            ui.label(&axis.name);
                            if ui.add(egui::Slider::new(&mut value, -1.0..=1.0)).changed() {
                                self.request(Request {
                                    axis: Some(axis.name.clone()),
                                    value: Some(value),
                                    ..Request::new("input")
                                });
                            }
                            if ui.button("中立").clicked() {
                                self.request(Request {
                                    axis: Some(axis.name),
                                    value: Some(0.0),
                                    ..Request::new("input")
                                });
                            }
                        });
                    }
                });
            });
        }
    }
}

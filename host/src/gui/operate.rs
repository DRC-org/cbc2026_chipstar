use super::*;

impl BridgeApp {
    pub(super) fn operate(&mut self, ui: &mut egui::Ui) {
        let status = self.shared.status_snapshot();
        let config = self.shared.config();
        ui.horizontal(|ui| {
            ui.label(RichText::new("アーム").size(20.0).strong());
            if status.slow {
                chip(ui, "低速 20%", ACCENT);
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
        self.manual_controls(ui, &status);
        self.operate_ee(ui, &status);
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
            ui.horizontal_wrapped(|ui| {
                keycap(ui, "左スティック / 右上下");
                ui.label(RichText::new("r・θ / z").size(12.0).color(MUTED));
                keycap(ui, "右左右 / 十字上下 / 十字左右");
                ui.label(
                    RichText::new("EE回転 / 畳み / 把持3本")
                        .size(12.0)
                        .color(MUTED),
                );
                keycap(ui, "L1");
                ui.label(RichText::new("低速20%").size(12.0).color(MUTED));
                keycap(ui, "Create 1秒");
                ui.label(
                    RichText::new("停止中・姿勢確認済みでホーミング")
                        .size(12.0)
                        .color(MUTED),
                );
                keycap(ui, "Options / PS");
                ui.label(RichText::new("再開 / 停止").size(12.0).color(MUTED));
            });
        });
    }
}

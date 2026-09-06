use super::*;

impl BridgeApp {
    pub(super) fn operate(&mut self, ui: &mut egui::Ui) {
        let status = self.shared.status_snapshot();
        ui.add_space(12.0);
        ui.label(
            RichText::new(if status.running {
                "操縦可能"
            } else {
                "停止中"
            })
            .size(34.0)
            .color(if status.running {
                Color32::LIGHT_GREEN
            } else {
                Color32::WHITE
            }),
        );
        ui.label(RichText::new(&status.reason).size(19.0));
        ui.add_space(18.0);
        ui.horizontal_wrapped(|ui| {
            badge(
                ui,
                if status.connected {
                    "機体 接続済み"
                } else {
                    "機体 未接続"
                },
                status.connected,
            );
            badge(
                ui,
                if status.configured {
                    "設定 一致"
                } else {
                    "設定 未確認"
                },
                status.configured,
            );
            if status.slow {
                badge(ui, "低速", true);
            }
            if status.origin_adjustment {
                ui.colored_label(Color32::YELLOW, "原点調整中・機体座標の可動域制限解除");
            }
        });
        ui.add_space(20.0);
        ui.columns(3, |columns| {
            for (i, axis) in status.origins.iter().enumerate() {
                let ui = &mut columns[i % 3];
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_min_height(130.0);
                    ui.label(
                        RichText::new(if axis.name == "theta" {
                            "θ"
                        } else {
                            &axis.name
                        })
                        .size(24.0),
                    );
                    ui.label(
                        RichText::new(format!("{:.1} {}", axis.position, axis.unit)).size(30.0),
                    );
                    ui.label(if axis.captured {
                        "原点 採用済み"
                    } else if axis.lost {
                        "原点 喪失・再確認が必要"
                    } else {
                        "原点 未採用・暫定座標"
                    });
                    if axis.at_limit == Some(true) {
                        ui.colored_label(Color32::YELLOW, "リミット到達");
                    }
                });
            }
        });
        ui.add_space(24.0);
        ui.label("左スティック：r・θ　右スティック上下：z　L1：低速");
        ui.label("Options：運転再開　PS：停止・保持");
        ui.label(if status.gamepad.is_empty() {
            "DualSense 未接続"
        } else {
            &status.gamepad
        });
        if status.simulated {
            ui.add_space(16.0);
            ui.collapsing("模擬スティック（実機出力なし）", |ui| {
                for axis in self.shared.config().machine.axes {
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
        }
    }
}

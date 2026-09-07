use super::*;

impl BridgeApp {
    pub(super) fn diagnose(&mut self, ui: &mut egui::Ui) {
        let status = self.shared.status_snapshot();
        let can_change =
            !status.emergency && !status.test_active && !status.ai_active && !status.running;
        section(ui, "接続と診断", "接続状態と通信履歴を確認できます。");
        ui.columns(2, |columns| {
            panel().show(&mut columns[0], |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("接続先").strong());
                ui.add_enabled_ui(can_change, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("接続方式");
                        ui.selectable_value(&mut self.connection.simulate, Some(false), "実機");
                        ui.selectable_value(&mut self.connection.simulate, Some(true), "模擬接続");
                    });

                    ui.horizontal(|ui| {
                        ui.label("ポート");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.connection.serial_device)
                                .desired_width(300.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("通信速度");
                        ui.add(egui::DragValue::new(&mut self.connection.baud_rate));
                    });
                    if ui.button("接続設定を適用").clicked() {
                        self.request(Request {
                            text: Some(toml::to_string(&self.connection).unwrap()),
                            ..Request::new("connection")
                        });
                    }
                });
                ui.label(
                    RichText::new("接続先の変更は停止中に行えます")
                        .size(12.0)
                        .color(MUTED),
                );
            });
            panel().show(&mut columns[1], |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("機体の状態").strong());
                egui::Grid::new("health")
                    .spacing([24.0, 10.0])
                    .show(ui, |ui| {
                        for (label, value) in [
                            ("基板モード", status.board_mode.clone()),
                            ("応答経過", format!("{} ms", status.telemetry_age_ms)),
                            ("送信数", format!("{} 行", status.tx_count)),
                        ] {
                            ui.label(RichText::new(label).color(MUTED));
                            ui.label(value);
                            ui.end_row();
                        }
                    });
                ui.add_enabled_ui(!status.ai_active, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .button("SAFE")
                            .on_hover_text("出力を切って設定可能な状態へ移行")
                            .clicked()
                        {
                            self.operation("safe");
                        }
                        if ui
                            .button(RichText::new("全出力停止").color(DANGER))
                            .on_hover_text("位置保持を解除。位置追跡が継続していれば原点は維持")
                            .clicked()
                        {
                            self.operation("cut");
                        }
                    });
                });
            });
        });
        ui.add_space(12.0);
        self.individual_test(ui, &status);
        ui.add_space(12.0);
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::CollapsingHeader::new("保守・周辺基板")
                .default_open(true)
                .show(ui, |ui| {
                    for (board, state) in &status.peripherals {
                        ui.label(format!("{board} · 最終受信値：{state}"));
                    }
                    egui::CollapsingHeader::new("モータ再初期化")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.label(
                    RichText::new(
                        "再通電後に制御モードを設定し直します。完了後は原点を再確認してください。",
                    )
                    .color(MUTED),
                );
                            ui.add_enabled_ui(can_change, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    for axis in self.shared.config().machine.axes {
                                        if ui.button(format!("{} を再初期化", axis.name)).clicked()
                                        {
                                            self.request(Request {
                                                axis: Some(axis.name),
                                                ..Request::new("reinit")
                                            });
                                        }
                                    }
                                });
                            });
                        });
                    if status.simulated {
                        egui::CollapsingHeader::new("模擬接続のテスト")
                            .default_open(true)
                            .show(ui, |ui| {
                                ui.add_enabled_ui(!status.ai_active, |ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        for (label, fault) in [
                                            ("通信断", "disconnect"),
                                            ("接続復帰", "reconnect"),
                                            ("次の指令を拒否", "reject"),
                                        ] {
                                            if ui.button(label).clicked() {
                                                self.request(Request {
                                                    text: Some(fault.into()),
                                                    ..Request::new("fault")
                                                });
                                            }
                                        }
                                    });
                                });
                            });
                    }
                });
        });
        ui.add_space(12.0);
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("通信ログ").strong());
                ui.label(RichText::new("直近300行").size(12.0).color(MUTED));
                ui.add(
                    egui::TextEdit::singleline(&mut self.log_filter)
                        .hint_text("絞り込み：ERR / PARAM / JOG …")
                        .desired_width(280.0),
                );
                if !self.log_filter.is_empty() && ui.small_button("クリア").clicked() {
                    self.log_filter.clear();
                }
            });
            ui.add_space(4.0);
            let filter = self.log_filter.to_lowercase();
            egui::ScrollArea::vertical()
                .id_salt("communication-log")
                .max_height(260.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for line in status
                        .logs
                        .iter()
                        .filter(|line| line.to_lowercase().contains(&filter))
                    {
                        let color = if line.starts_with("ERR") || line.contains(" ERR ") {
                            DANGER
                        } else if line.starts_with("TX") {
                            MUTED
                        } else {
                            Color32::from_rgb(209, 223, 235)
                        };
                        ui.label(RichText::new(line).monospace().size(12.0).color(color));
                    }
                });
        });
    }
}

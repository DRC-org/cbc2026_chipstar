use super::*;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum DiagnosisView {
    Tests,
    Sts,
    Connection,
    Log,
}

impl BridgeApp {
    pub(super) fn switch_diagnosis_view(&mut self, view: DiagnosisView) {
        if self.diagnosis_view != view {
            self.end_test_on_tab_change();
            self.diagnosis_view = view;
            self.navigation = Some(Action::Edge(true));
        }
    }

    pub(super) fn diagnose(&mut self, ui: &mut egui::Ui) {
        let status = self.shared.status_snapshot();
        let can_change =
            !status.emergency && !status.test_active && !status.ai_active && !status.running;
        section(
            ui,
            "機体の状態を確認",
            "接続状態を確認し、必要な機構だけを個別に動かします。",
        );
        let mut open_error_log = false;
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                chip(
                    ui,
                    if status.connected {
                        "cctl 受信中"
                    } else {
                        "cctl 未接続"
                    },
                    if status.connected { ACCENT } else { DANGER },
                );
                chip(
                    ui,
                    if status.configured {
                        "設定一致"
                    } else {
                        "設定確認中"
                    },
                    if status.configured { ACCENT } else { WARNING },
                );
                let mut boards = std::collections::BTreeSet::new();
                for board in status.peripherals.keys() {
                    if let Some(label) = peripheral_group(board) {
                        boards.insert(label);
                    }
                }
                for board in boards {
                    chip(ui, &format!("{board} 受信中"), ACCENT);
                }
                if status.peripherals.is_empty() {
                    ui.label(
                        RichText::new("周辺基板からの応答なし")
                            .size(12.0)
                            .color(MUTED),
                    );
                }
                if !status.error.is_empty() && ui.button("異常ログを開く").clicked() {
                    open_error_log = true;
                }
            });
        });
        if open_error_log {
            self.log_filter = "ERR".into();
            self.switch_diagnosis_view(DiagnosisView::Log);
        }
        ui.add_space(8.0);
        let mut view = self.diagnosis_view;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(165.0);
                ui.label(RichText::new("確認する項目").strong());
                for (item, label) in [
                    (DiagnosisView::Tests, "個別に動作確認"),
                    (DiagnosisView::Sts, "STS3215の設定"),
                    (DiagnosisView::Connection, "接続と再初期化"),
                    (DiagnosisView::Log, "通信ログ"),
                ] {
                    ui.selectable_value(&mut view, item, label);
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                match view {
            DiagnosisView::Sts => self.sts_panel(ui),
            DiagnosisView::Tests => {
                self.individual_test(ui, &status);
                ui.add_space(12.0);
                panel().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    section(
                        ui,
                        "基板・センサの現在値",
                        "表示を確認するだけではモータ出力を開始しません。",
                    );
                    if status.peripherals.is_empty() {
                        ui.label(RichText::new("周辺基板からの受信を待っています").color(MUTED));
                    }
                    egui::Grid::new("peripheral-values")
                        .num_columns(2)
                        .spacing([20.0, 8.0])
                        .show(ui, |ui| {
                            for (board, state) in &status.peripherals {
                                ui.label(RichText::new(peripheral_name(board)).strong());
                                ui.label(state);
                                ui.end_row();
                            }
                        });
                });
            }
            DiagnosisView::Connection => {
                section(
                    ui,
                    "接続と再初期化",
                    "hostの接続先変更と、再通電したモータの初期化を行います。",
                );
                ui.columns(2, |columns| {
                    panel().show(&mut columns[0], |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new("hostが接続する機体").strong());
                        ui.add_enabled_ui(can_change, |ui| {
                            ui.horizontal(|ui| {
                                ui.label("接続方式");
                                ui.selectable_value(
                                    &mut self.connection.simulate,
                                    Some(false),
                                    "実機",
                                );
                                ui.selectable_value(
                                    &mut self.connection.simulate,
                                    Some(true),
                                    "模擬接続",
                                );
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
                            RichText::new("接続先を変更すると、原点の再確認が必要になります。")
                                .size(12.0)
                                .color(MUTED),
                        );
                    });
                    panel().show(&mut columns[1], |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new("cctlの通信状態").strong());
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
                                    .button("設定待機へ移行")
                                    .on_hover_text("cctlへSAFEを送り、モータ出力を切って設定可能な状態へ移行します")
                                    .clicked()
                                {
                                    self.operation("safe");
                                }
                                if ui
                                    .button(RichText::new("全出力停止").color(DANGER))
                                    .on_hover_text(
                                        "位置保持を解除。位置追跡が継続していれば原点は維持",
                                    )
                                    .clicked()
                                {
                                    self.operation("cut");
                                }
                            });
                        });
                    });
                });
                ui.add_space(12.0);
                panel().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::CollapsingHeader::new("周辺基板とモータの再初期化")
                        .default_open(true)
                        .show(ui, |ui| {
                            for (board, state) in &status.peripherals {
                                ui.label(format!(
                                    "{} · 最終受信値：{state}",
                                    peripheral_name(board)
                                ));
                            }
                            egui::CollapsingHeader::new("再通電したモータを初期化")
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
                                                if ui
                                                    .button(format!("{} を再初期化", axis.name))
                                                    .clicked()
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
            }
            DiagnosisView::Log => {
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
                        .max_height((ui.available_height() - 12.0).max(260.0))
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
            });
        });
        self.switch_diagnosis_view(view);
    }
}

fn peripheral_group(board: &str) -> Option<&'static str> {
    match board {
        "cctl 接点" => None,
        "pwm" => Some("PWMサーボ基板"),
        "dc" | "DCMD" | "dcmd 接点" => Some("DCモータ基板"),
        "sts" | "serial_svmd 接点" => Some("STS3215基板"),
        name if name.starts_with("STS3215 ID ") => Some("STS3215"),
        name if name.starts_with("C620 ") => Some("C620"),
        _ => Some("周辺基板"),
    }
}

fn peripheral_name(board: &str) -> String {
    match board {
        "pwm" => "PWMサーボ基板".into(),
        "dc" => "DCモータ基板".into(),
        "sts" => "STS3215基板".into(),
        "serial_svmd 接点" => "STS3215基板の接点".into(),
        "dcmd 接点" => "DCモータ基板の接点".into(),
        "cctl 接点" => "cctlのリミットスイッチ".into(),
        other => other.into(),
    }
}

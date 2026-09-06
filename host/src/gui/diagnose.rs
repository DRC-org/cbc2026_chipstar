use super::*;

impl BridgeApp {
    pub(super) fn diagnose(&mut self, ui: &mut egui::Ui) {
        let status = self.shared.status_snapshot();
        ui.heading("接続と診断");
        ui.horizontal(|ui| {
            ui.label("接続先");
            ui.text_edit_singleline(&mut self.connection.serial_device);
            ui.add(egui::DragValue::new(&mut self.connection.baud_rate));
            if ui.button("再接続").clicked() {
                self.request(Request {
                    text: Some(toml::to_string(&self.connection).unwrap()),
                    ..Request::new("connection")
                });
            }
        });
        for (board, state) in &status.peripherals {
            ui.label(format!("{board}（最終受信値）: {state}"));
        }
        ui.collapsing("モータ再初期化", |ui| {
            ui.label("モータを再通電した後に制御モードを設定し直します。完了後は原点の再確認が必要です。");
            for axis in self.shared.config().machine.axes {
                if ui.button(format!("{}を再初期化", axis.name)).clicked() { self.request(Request { axis: Some(axis.name), ..Request::new("reinit") }); }
            }
        });
        ui.label(format!("基板状態：{}", status.board_mode));
        ui.label(format!(
            "応答から {} ms / 送信 {} 行",
            status.telemetry_age_ms, status.tx_count
        ));
        ui.horizontal(|ui| {
            if ui.button("SAFE（出力を切る）").clicked() {
                self.operation("safe");
            }
            if ui.button("全出力停止").clicked() {
                self.operation("cut");
            }
        });
        if status.simulated {
            ui.horizontal(|ui| {
                for (label, fault) in [
                    ("通信断を模擬", "disconnect"),
                    ("再接続", "reconnect"),
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
        }
        ui.collapsing("配線ガイド", |ui| {
            ui.label(include_str!("../../../docs/wiring.md"));
        });
        ui.separator();
        ui.label("通信ログ（直近300行）");
        egui::ScrollArea::vertical()
            .max_height(430.0)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in &status.logs {
                    ui.monospace(line);
                }
            });
    }
}

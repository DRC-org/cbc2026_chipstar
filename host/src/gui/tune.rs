use super::*;

impl BridgeApp {
    pub(super) fn tune(&mut self, ui: &mut egui::Ui) {
        let status = self.shared.status_snapshot();
        ui.heading("原点と可動域");
        ui.label(
            "原点採用は現在位置に設定上の原点座標を割り当てます。モータのゼロ位置は変更しません。",
        );
        let mut adjustment = status.origin_adjustment;
        if ui
            .checkbox(
                &mut adjustment,
                "原点調整モード（低速固定・機体座標の可動域制限を解除）",
            )
            .changed()
        {
            self.request(Request {
                flag: Some(adjustment),
                ..Request::new("adjustment")
            });
        }
        for axis in &status.origins {
            ui.horizontal(|ui| {
                ui.label(format!("{}  {:.1} {}", axis.name, axis.position, axis.unit));
                if ui.button("現在位置を原点に採用").clicked() {
                    self.request(Request {
                        axis: Some(axis.name.clone()),
                        ..Request::new("origin")
                    });
                }
            });
        }
        ui.separator();
        ui.heading("機体設定");
        ui.label(format!(
            "保存先：{}",
            self.shared.config().profile_path.display()
        ));
        ui.label(format!(
            "{}（{}/{}項目）",
            status.configuration, status.parameters_confirmed, status.parameters_expected
        ));
        let current = toml::to_string_pretty(&self.shared.config().machine).unwrap_or_default();
        if current != self.base {
            ui.colored_label(
                Color32::YELLOW,
                "別の操作で設定が更新されました。現在の設定を読み直してください。",
            );
        }
        let mut edited = false;
        for axis in &mut self.edit.axes {
            ui.push_id(&axis.name, |ui| {
                ui.collapsing(format!("{} [{}]", axis.name, axis.unit), |ui| {
                    egui::Grid::new("axis").num_columns(2).show(ui, |ui| {
                        for (label, value) in [
                            ("通常速度 / 秒", &mut axis.speed_per_second),
                            ("最小位置", &mut axis.minimum),
                            ("最大位置", &mut axis.maximum),
                            ("原点採用時の座標", &mut axis.origin_position),
                            ("ネイティブ単位 / 機体単位", &mut axis.native_per_unit),
                        ] {
                            ui.label(label);
                            edited |= ui.add(egui::DragValue::new(value).speed(0.1)).changed();
                            ui.end_row();
                        }
                    });
                    ui.label(
                        "可動域の値は実機で校正してください。θの安全な全周回転高さはありません。",
                    );
                });
            });
        }
        if edited {
            self.source = toml::to_string_pretty(&self.edit).unwrap_or_default();
        }
        ui.collapsing("基板の調整値", |ui| {
            egui::Grid::new("parameters").striped(true).show(ui, |ui| {
                for (name, value) in &mut self.edit.parameters {
                    ui.label(name);
                    edited |= ui.add(egui::DragValue::new(value).speed(0.01)).changed();
                    ui.end_row();
                }
            });
        });
        if edited {
            self.source = toml::to_string_pretty(&self.edit).unwrap_or_default();
        }
        ui.collapsing("全設定を編集（接点・サーボを含む）", |ui| {
            if ui
                .add(
                    egui::TextEdit::multiline(&mut self.source)
                        .font(egui::TextStyle::Monospace)
                        .desired_rows(20)
                        .desired_width(f32::INFINITY),
                )
                .changed()
                && let Ok(profile) = MachineProfile::parse(&self.source)
            {
                self.edit = profile;
            }
        });
        ui.horizontal(|ui| {
            if ui.button("現在の設定を読み直す").clicked() {
                self.reload();
            }
            if ui
                .add_enabled(
                    current == self.base && !status.running,
                    egui::Button::new("一時適用"),
                )
                .clicked()
            {
                match MachineProfile::parse(&self.source) {
                    Ok(profile) => {
                        self.request(Request {
                            text: Some(self.source.clone()),
                            ..Request::new("apply")
                        });
                        if self.shared.config().machine == profile {
                            self.reload();
                        }
                    }
                    Err(error) => self.message = error.to_string(),
                }
            }
            if ui
                .add_enabled(!status.running, egui::Button::new("適用中の設定を保存"))
                .clicked()
            {
                self.operation("save");
            }
        });
        ui.label(if status.saved {
            "PCへの保存済み"
        } else {
            "PCへの保存は未確認。一時適用だけでは次回起動に引き継がれません。"
        });
    }
}

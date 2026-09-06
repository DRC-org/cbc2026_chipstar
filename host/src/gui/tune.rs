use super::*;

impl BridgeApp {
    pub(super) fn tune(&mut self, ui: &mut egui::Ui) {
        let status = self.shared.status_snapshot();
        section(
            ui,
            "原点の確認",
            "現在の実測位置に、設定上の原点座標を割り当てます。",
        );
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut adjustment = status.origin_adjustment;
            if ui
                .add_enabled(
                    !status.running,
                    egui::Checkbox::new(&mut adjustment, "原点調整モード"),
                )
                .on_hover_text("低速固定・機体座標の可動域制限を解除")
                .changed()
            {
                self.request(Request {
                    flag: Some(adjustment),
                    ..Request::new("adjustment")
                });
            }
            ui.label(
                RichText::new(if adjustment {
                    "低速固定 · 機体座標の可動域制限を解除しています"
                } else {
                    "原点まで移動する際に有効にしてください"
                })
                .size(12.0)
                .color(if adjustment { WARNING } else { MUTED }),
            );
            ui.add_space(10.0);
            ui.columns(3, |columns| {
                for (index, axis) in status.origins.iter().enumerate() {
                    let ui = &mut columns[index % 3];
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&axis.name).strong().color(ACCENT));
                        ui.label(format!("{:.1} {}", axis.position, axis.unit));
                    });
                    if ui
                        .add_enabled(
                            status.connected && !status.running,
                            egui::Button::new("現在位置を原点に採用"),
                        )
                        .clicked()
                    {
                        self.request(Request {
                            axis: Some(axis.name.clone()),
                            ..Request::new("origin")
                        });
                    }
                    ui.label(
                        RichText::new(if axis.captured {
                            "採用済み"
                        } else if axis.lost {
                            "原点喪失"
                        } else {
                            "未採用"
                        })
                        .size(12.0)
                        .color(if axis.captured {
                            MUTED
                        } else {
                            WARNING
                        }),
                    );
                }
            });
        });
        ui.add_space(18.0);
        section(ui, "機体設定", "編集 → 一時適用 → 保存の順で反映します。");
        let current = toml::to_string_pretty(&self.shared.config().machine).unwrap_or_default();
        let draft_matches = self.draft_matches_applied();
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                chip(ui, if draft_matches { "適用内容と一致" } else { "未適用の編集あり" }, if draft_matches { MUTED } else { WARNING });
                chip(ui, if status.saved { "適用内容は保存済み" } else { "適用内容は未保存" }, if status.saved { ACCENT } else { WARNING });
            });
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(self.can_apply(&status), egui::Button::new("一時適用  Ctrl+Shift+Enter").fill(Color32::from_rgb(27, 80, 74))).clicked() { self.dispatch(Action::Apply); }
                if ui.add_enabled(self.can_save(&status), egui::Button::new("保存  Ctrl+S"))
                    .on_hover_text("適用中の設定をファイルへ保存。未適用の編集がある場合は先に一時適用してください。").clicked() { self.dispatch(Action::Save); }
                if ui.button("適用中の内容に戻す").on_hover_text("未適用の編集を破棄して、hostで適用中の設定を読み直します").clicked() { self.reload(); }
            });
            ui.label(RichText::new(format!("{}  ·  {}/{}項目", status.configuration, status.parameters_confirmed, status.parameters_expected)).size(12.0).color(MUTED));
            ui.label(RichText::new(format!("保存先  {}", self.shared.config().profile_path.display())).size(12.0).color(MUTED));
            if current != self.base { ui.colored_label(WARNING, "外部から設定が更新されています。適用中の内容を読み直してください。"); }
        });
        ui.add_space(12.0);
        let mut edited = false;
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            for axis in &mut self.edit.axes {
                ui.push_id(&axis.name, |ui| {
                    egui::CollapsingHeader::new(format!(
                        "{}  ·  速度と可動域 [{}]",
                        axis.name, axis.unit
                    ))
                    .show(ui, |ui| {
                        egui::Grid::new("axis")
                            .num_columns(2)
                            .spacing([30.0, 10.0])
                            .show(ui, |ui| {
                                for (label, value) in [
                                    ("通常速度 / 秒", &mut axis.speed_per_second),
                                    ("最小位置", &mut axis.minimum),
                                    ("最大位置", &mut axis.maximum),
                                    ("原点採用時の座標", &mut axis.origin_position),
                                    ("ネイティブ単位 / 機体単位", &mut axis.native_per_unit),
                                ] {
                                    ui.label(label);
                                    edited |=
                                        ui.add(egui::DragValue::new(value).speed(0.1)).changed();
                                    ui.end_row();
                                }
                            });
                    });
                });
            }
            ui.label(
                RichText::new(
                    "可動域は実機で校正してください。θの安全な全周回転高さはありません。",
                )
                .size(12.0)
                .color(MUTED),
            );
        });
        ui.add_space(12.0);
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.collapsing("基板の調整値", |ui| {
                egui::Grid::new("parameters")
                    .striped(true)
                    .spacing([30.0, 8.0])
                    .show(ui, |ui| {
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
            ui.collapsing("全設定を編集 · TOML", |ui| {
                if ui
                    .add(
                        egui::TextEdit::multiline(&mut self.source)
                            .font(egui::TextStyle::Monospace)
                            .desired_rows(16)
                            .desired_width(f32::INFINITY),
                    )
                    .changed()
                    && let Ok(profile) = MachineProfile::parse(&self.source)
                {
                    self.edit = profile;
                }
                if let Err(error) = MachineProfile::parse(&self.source) {
                    ui.colored_label(DANGER, error.to_string());
                }
            });
        });
    }
}

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

            ui.horizontal(|ui| {
                ui.label("読込元・保存先");
                ui.add(egui::TextEdit::singleline(&mut self.profile_file).desired_width(470.0));
                if ui.button("ファイルから読込").on_hover_text("編集中の内容をファイルの内容に置き換えます。一時適用するまで機体設定は変わりません。").clicked() {
                    match crate::transport::profile_store::load(std::path::Path::new(&self.profile_file)) {
                        Ok(profile) => {
                            self.edit = profile;
                            self.source = toml::to_string_pretty(&self.edit).unwrap_or_default();
                            self.message = "設定ファイルを読み込みました。一時適用で反映します".into();
                            self.message_error = false;
                        }
                        Err(error) => { self.message = error.to_string(); self.message_error = true; }
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                chip(ui, if draft_matches { "適用内容と一致" } else { "未適用の編集あり" }, if draft_matches { MUTED } else { WARNING });
                chip(ui, if status.saved { "適用内容は保存済み" } else { "適用内容は未保存" }, if status.saved { ACCENT } else { WARNING });
            });
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(self.can_apply(&status), egui::Button::new("一時適用  Ctrl+Shift+Enter").fill(Color32::from_rgb(27, 80, 74))).clicked() { self.dispatch(Action::Apply); }
                if ui.add_enabled(self.can_save(&status), egui::Button::new("保存  Ctrl+S"))
                    .on_hover_text("指定したパスに適用中の設定を保存。未適用の編集がある場合は先に一時適用してください。").clicked() { self.dispatch(Action::Save); }
                if ui.button("適用中の内容に戻す").on_hover_text("未適用の編集を破棄して、hostで適用中の設定を読み直します").clicked() { self.reload(); }
            });
            ui.label(RichText::new(format!("{}  ·  {}/{}項目", status.configuration, status.parameters_confirmed, status.parameters_expected)).size(12.0).color(MUTED));
            ui.label(RichText::new(format!("適用中の設定ファイル  {}", self.shared.config().profile_path.display())).size(12.0).color(MUTED));
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
                    .default_open(true)
                    .show(ui, |ui| {
                        egui::Grid::new("axis")
                            .num_columns(3)
                            .spacing([30.0, 10.0])
                            .show(ui, |ui| {
                                for (label, value, description) in [
                                ("通常速度 / 秒", &mut axis.speed_per_second, "スティック最大入力時の機体速度。低速・画面操作ではこの20%になります。"),
                                ("最小位置", &mut axis.minimum, "原点採用後に有効になる機体座標の下限。基板側の絶対可動域とは別です。"),
                                ("最大位置", &mut axis.maximum, "原点採用後に有効になる機体座標の上限。境界付近では速度を抑えます。"),
                                ("原点採用時の座標", &mut axis.origin_position, "現在位置を原点に採用したとき、その位置に割り当てる機体座標です。"),
                                ("ネイティブ単位 / 機体単位", &mut axis.native_per_unit, "機体の1 mmまたは1 degをモータ側の単位へ変換する倍率。ギア比などから決めます。"),
                            ] {
                                ui.label(label).on_hover_text(description);
                                edited |= ui.add(egui::DragValue::new(value).speed(0.1)).on_hover_text(description).changed();
                                ui.label(RichText::new(description).size(12.0).color(MUTED));
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
            egui::CollapsingHeader::new("基板の調整値")
                .default_open(true)
                .show(ui, |ui| {
                    for (board, parameters) in [
                        ("cctl", &mut self.edit.parameters),
                        ("PWMサーボ基板", &mut self.edit.svmd_parameters),
                        ("DCモータ基板", &mut self.edit.dcmd_parameters),
                        ("STS3215基板", &mut self.edit.serial_svmd_parameters),
                    ] {
                        if parameters.is_empty() {
                            continue;
                        }
                        ui.label(RichText::new(board).strong().color(ACCENT));
                        egui::Grid::new(("parameters", board))
                            .num_columns(3)
                            .striped(true)
                            .spacing([16.0, 10.0])
                            .max_col_width(540.0)
                            .show(ui, |ui| {
                                for (name, value) in parameters {
                                    let (unit, description) = parameter_help::help(name)
                                        .unwrap_or(("", "この項目の説明は未登録です"));
                                    ui.label(name).on_hover_text(description);
                                    edited |= ui
                                        .add(egui::DragValue::new(value).speed(0.01).suffix(
                                            if unit.is_empty() {
                                                String::new()
                                            } else {
                                                format!(" {unit}")
                                            },
                                        ))
                                        .on_hover_text(description)
                                        .changed();
                                    ui.label(RichText::new(description).size(12.0).color(MUTED));
                                    ui.end_row();
                                }
                            });
                    }
                });
            if edited {
                self.source = toml::to_string_pretty(&self.edit).unwrap_or_default();
            }
            egui::CollapsingHeader::new("全設定を編集 · TOML")
                .default_open(true)
                .show(ui, |ui| {
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

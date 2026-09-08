use super::*;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum TuneView {
    Axes,
    Ee,
    Parameters,
    File,
}

impl BridgeApp {
    pub(super) fn tune_actions(&mut self, ui: &mut egui::Ui) {
        let status = self.shared.status_snapshot();
        ui.add_enabled_ui(!status.ai_active, |ui| {
            let current = toml::to_string_pretty(&self.shared.config().machine).unwrap_or_default();
            let matches = self.draft_matches_applied();
            egui::Frame::new()
                .fill(SURFACE)
                .stroke(egui::Stroke::new(1.0, BORDER))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            self.can_apply(&status),
                            egui::Button::new("適用  :apply").fill(Color32::from_rgb(27, 80, 74)),
                        )
                        .clicked()
                    {
                        self.dispatch(Action::Apply);
                    }
                    if ui
                        .add_enabled(self.can_save(&status), egui::Button::new("保存  :w"))
                        .on_hover_text(format!(
                            "保存先：{}\n未適用の編集は先に適用してください。",
                            self.profile_file
                        ))
                        .clicked()
                    {
                        self.dispatch(Action::Save);
                    }
                    if ui
                        .button("編集を戻す")
                        .on_hover_text("未適用の編集を破棄して、適用中の内容に戻します")
                        .clicked()
                    {
                        self.reload();
                    }
                    chip(
                        ui,
                        if matches {
                            "編集内容を適用済み"
                        } else {
                            "未適用の変更あり"
                        },
                        if matches { MUTED } else { WARNING },
                    );
                    chip(
                        ui,
                        if status.saved {
                            "ファイル保存済み"
                        } else {
                            "ファイル未保存"
                        },
                        if status.saved { ACCENT } else { WARNING },
                    );
                    ui.label(
                        RichText::new(format!(
                            "基板への反映 {}/{}",
                            status.parameters_confirmed, status.parameters_expected
                        ))
                        .size(12.0)
                        .color(MUTED),
                    )
                    .on_hover_text(&status.configuration);
                });
                if current != self.base {
                    ui.colored_label(
                        WARNING,
                        "別の操作で設定が変わりました。「編集を戻す」で現在の設定を読み込んでください。",
                    );
                }
            });
        });
        ui.add_space(8.0);
    }

    pub(super) fn tune(&mut self, ui: &mut egui::Ui) {
        let edited = match self.tune_view {
            TuneView::Axes => self.tune_axes(ui),
            TuneView::Ee => self.tune_ee(ui),
            TuneView::Parameters => self.tune_parameters(ui),
            TuneView::File => {
                self.tune_file(ui);
                false
            }
        };
        if edited {
            self.source = toml::to_string_pretty(&self.edit).unwrap_or_default();
        }
    }

    fn tune_axes(&mut self, ui: &mut egui::Ui) -> bool {
        let status = self.shared.status_snapshot();
        let axes: Vec<_> = self
            .edit
            .axes
            .iter()
            .map(|axis| (axis.name.clone(), axis.slot))
            .collect();
        if !axes.iter().any(|(name, _)| name == &self.tune_axis) {
            self.tune_axis = axes
                .first()
                .map(|(name, _)| name.clone())
                .unwrap_or_default();
        }
        let mut edited = false;
        section(
            ui,
            "アームを調整",
            "対象軸の原点、速度、可動範囲、制御応答をまとめて確認します。",
        );
        ui.horizontal_wrapped(|ui| {
            ui.label("調整する軸");
            for (name, _) in &axes {
                let label = if name == "theta" { "θ" } else { name };
                ui.selectable_value(&mut self.tune_axis, name.clone(), label);
            }
            ui.separator();
            let description =
                "L1保持・画面操作・原点調整・個別速度テストで使う、通常最高速度に対する割合です。";
            ui.label("低速操作率").on_hover_text(description);
            edited |= ui
                .add(
                    egui::DragValue::new(&mut self.edit.slow_speed_percent)
                        .speed(1.0)
                        .range(1.0..=100.0)
                        .suffix(" %"),
                )
                .on_hover_text(description)
                .changed();
            if let Some(origin) = status
                .origins
                .iter()
                .find(|origin| origin.name == self.tune_axis)
            {
                ui.separator();
                ui.label(format!("現在位置 {:.2} {}", origin.position, origin.unit));
                chip(
                    ui,
                    if origin.captured {
                        "原点設定済み"
                    } else if origin.lost {
                        "原点を再設定してください"
                    } else {
                        "原点未設定"
                    },
                    if origin.captured { ACCENT } else { WARNING },
                );
            }
        });
        ui.add_space(8.0);
        let selected = axes
            .iter()
            .find(|(name, _)| name == &self.tune_axis)
            .cloned();
        let mut capture_origin = false;
        let mut adjustment_change = None;
        if let Some((name, slot)) = selected {
            ui.columns(2, |columns| {
                panel().show(&mut columns[0], |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("原点と移動範囲").strong());
                    if let Some(origin) = status.origins.iter().find(|axis| axis.name == name) {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(format!("実測位置 {:.2} {}", origin.position, origin.unit));
                            if ui
                                .add_enabled(
                                    status.connected && !status.running,
                                    egui::Button::new("この位置を原点として設定"),
                                )
                                .on_hover_text("現在の実測位置へ、下の「原点位置」の座標を割り当てます")
                                .clicked()
                            {
                                capture_origin = true;
                            }
                        });
                    }
                    let mut adjustment = status.origin_adjustment;
                    if ui
                        .add_enabled(
                            !status.running,
                            egui::Checkbox::new(&mut adjustment, "原点位置まで手動で移動する"),
                        )
                        .on_hover_text("低速固定にし、機体座標による移動範囲の制限を一時的に解除します")
                        .changed()
                    {
                        adjustment_change = Some(adjustment);
                    }
                    if adjustment {
                        ui.colored_label(
                            WARNING,
                            "低速固定中。機体座標による移動範囲の制限は無効です。",
                        );
                    }
                    if let Some(axis) = self.edit.axes.iter_mut().find(|axis| axis.name == name) {
                        edited |= axis_settings(ui, axis);
                    }
                    if name == "theta" {
                        ui.colored_label(
                            WARNING,
                            "θはどのz高さでも干渉する可能性があります。旋回前に実機を確認してください。",
                        );
                    }
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        ui.label("r・z自動ホーミングの制限時間");
                        ui.add(
                            egui::DragValue::new(&mut self.homing_timeout)
                                .range(10.0..=1800.0)
                                .suffix(" 秒/軸"),
                        );
                    });
                    ui.label(
                        RichText::new("開始操作は操縦画面にあります。")
                            .size(12.0)
                            .color(MUTED),
                    );
                });
                edited |= self.tune_pid_axis(&mut columns[1], &name, slot);
            });
            if capture_origin {
                self.request(Request {
                    axis: Some(name.clone()),
                    ..Request::new("origin")
                });
            }
            if let Some(adjustment) = adjustment_change {
                self.request(Request {
                    flag: Some(adjustment),
                    ..Request::new("adjustment")
                });
            }
            self.select_pid_axis(&name);
            ui.add_space(8.0);
            self.pid_response(ui);
        } else {
            ui.label("アーム軸が設定されていません。");
        }
        edited
    }

    fn tune_parameters(&mut self, ui: &mut egui::Ui) -> bool {
        let mut edited = false;
        section(
            ui,
            "基板へ送る制限値と通信設定を調整",
            "変更する値の説明を確認し、画面上部の「適用」で機体へ反映します。",
        );
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::CollapsingHeader::new("基板別の設定値")
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
                            .max_col_width((ui.available_width() - 360.0).max(180.0))
                            .show(ui, |ui| {
                                for (name, value) in parameters {
                                    if board == "cctl"
                                        && matches!(
                                            name.as_str(),
                                            "el05_limit_spd"
                                                | "m3508_max_rpm"
                                                | "m3508_slot2_max_rpm"
                                        )
                                    {
                                        continue;
                                    }
                                    let (unit, description) = parameter_help::help(name)
                                        .unwrap_or(("", "この項目の説明は未登録です"));
                                    let label = parameter_help::label(name).unwrap_or(name);
                                    ui.label(label)
                                        .on_hover_text(format!("設定キー：{name}\n{description}"));
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
        });
        edited
    }

    fn tune_file(&mut self, ui: &mut egui::Ui) {
        section(
            ui,
            "設定ファイルを直接確認・編集",
            "通常の調整項目にない値を含め、機体設定全体を扱います。",
        );
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label("読込元・保存先");
                ui.add(egui::TextEdit::singleline(&mut self.profile_file).desired_width(470.0));
                let hint = "編集中の内容をファイルから読み直します。一時適用するまで機体設定は変わりません。";
                if ui.button("ファイルから読込").on_hover_text(hint).clicked() {
                    match crate::transport::profile_store::load(std::path::Path::new(&self.profile_file)) {
                        Ok(profile) => {
                            self.edit = profile;
                            self.source = toml::to_string_pretty(&self.edit).unwrap_or_default();
                            self.message = "設定ファイルを読み込みました。一時適用で反映します".into();
                            self.message_error = false;
                        }
                        Err(error) => {
                            self.message = error.to_string();
                            self.message_error = true;
                        }
                    }
                }
            });
            let path = self.shared.config().profile_path.display().to_string();
            ui.label(RichText::new(format!("適用中の設定ファイル  {path}")).size(12.0).color(MUTED));
            egui::CollapsingHeader::new("機体設定の全項目（TOML形式）")
                .default_open(true)
                .show(ui, |ui| {
                    if ui.add(
                        egui::TextEdit::multiline(&mut self.source)
                            .font(egui::TextStyle::Monospace)
                            .desired_rows(16)
                            .desired_width(f32::INFINITY),
                    ).changed() && let Ok(profile) = MachineProfile::parse(&self.source) {
                        self.edit = profile;
                    }
                    if let Err(error) = MachineProfile::parse(&self.source) {
                        ui.colored_label(DANGER, error.to_string());
                    }
                });
        });
    }
}

fn axis_settings(ui: &mut egui::Ui, axis: &mut crate::machine::AxisProfile) -> bool {
    let mut edited = false;
    let unit = axis.unit.clone();
    ui.add_space(8.0);
    egui::Grid::new(("axis-settings", &axis.name))
        .num_columns(3)
        .spacing([12.0, 8.0])
        .striped(true)
        .show(ui, |ui| {
            for (label, value, suffix, description) in [
                (
                    "最高速度",
                    &mut axis.speed_per_second,
                    unit.as_str(),
                    "スティック最大入力時の機体速度です。低速時は上部の「低速操作率」を掛けます。",
                ),
                (
                    "加減速時間",
                    &mut axis.jog_ramp_seconds,
                    "s",
                    "停止から最高速度までの時間です。長いほど反転・中立時の変化が穏やかになります。0は即時変更。緊停や接点作動時は即時停止します。",
                ),
                (
                    "移動範囲の下限",
                    &mut axis.minimum,
                    unit.as_str(),
                    "原点設定後に通常操縦で移動できる最小座標です。",
                ),
                (
                    "移動範囲の上限",
                    &mut axis.maximum,
                    unit.as_str(),
                    "原点設定後に通常操縦で移動できる最大座標です。",
                ),
                (
                    "原点位置",
                    &mut axis.origin_position,
                    unit.as_str(),
                    "「この位置を原点として設定」を押した位置に割り当てる座標です。",
                ),
                (
                    "モータ換算係数",
                    &mut axis.native_per_unit,
                    "motor/unit",
                    "機体座標1単位を、モータ側の位置単位へ変換する倍率です。",
                ),
            ] {
                ui.label(label).on_hover_text(description);
                edited |= ui
                    .add(
                        egui::DragValue::new(value)
                            .speed(0.1)
                            .suffix(format!(" {suffix}")),
                    )
                    .on_hover_text(description)
                    .changed();
                ui.label("ⓘ").on_hover_text(description);
                ui.end_row();
            }
            if axis.limit.is_some() {
                let description =
                    "r・z自動ホーミングで使う速度です。軸の最高速度に対する1〜100%で指定します。";
                if matches!(axis.name.as_str(), "r" | "z") {
                    let mut distance = axis.homing_retreat_mm();
                    let help = "原点採用後にリミットから離れる距離です。rは後退、zは上昇します。0 mmで戻し移動を省略します。";
                    ui.label("ホーミング戻し量").on_hover_text(help);
                    if ui.add(egui::DragValue::new(&mut distance)
                        .speed(1.0).range(0.0..=f32::MAX).suffix(" mm"))
                        .on_hover_text(help).changed() {
                        axis.homing_retreat_mm = Some(distance);
                        edited = true;
                    }
                    ui.label("ⓘ").on_hover_text(help);
                    ui.end_row();
                }
                ui.label("ホーミング速度率").on_hover_text(description);
                edited |= ui
                    .add(
                        egui::DragValue::new(&mut axis.homing_speed_percent)
                            .speed(1.0)
                            .range(1.0..=100.0)
                            .suffix(" %"),
                    )
                    .on_hover_text(description)
                    .changed();
                ui.label("ⓘ").on_hover_text(description);
                ui.end_row();
            }
        });
    edited
}

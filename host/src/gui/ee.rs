use super::*;
use crate::machine::{PwmServoProfile, SerialServoProfile, SerialSvmdProfile, ee};
impl BridgeApp {
    pub(super) fn operate_ee(&mut self, ui: &mut egui::Ui, status: &Status) {
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("EEを操作").strong());
                if ui.small_button("設定を開く").clicked() {
                    self.tune_view = tune::TuneView::Ee;
                    self.switch_screen(Screen::Tune);
                }
            });
            let axes = ee::axes(&self.shared.config().machine);
            let can = status.running
                && !status.test_mode
                && !status.sts.active
                && !status.sts.busy
                && !status.ai_active
                && !status.emergency;
            let mut command = std::collections::BTreeMap::new();
            egui::Grid::new("operate-ee")
                .num_columns(5)
                .spacing([8.0, 6.0])
                .striped(true)
                .show(ui, |ui| {
                    for (name, label) in ee::ROLES {
                        ui.label(label);
                        if let Some(axis) = axes.iter().find(|a| a.name == name) {
                            if name == "ee_rotation" {
                                // 先端回転はフィールド基準0°/180°のトグル。
                                let field = self.ee_values.entry(name.into()).or_insert(0.0);
                                if ui
                                    .add_enabled(can && axis.enabled, egui::Button::new("0°へ"))
                                    .clicked()
                                {
                                    *field = 0.0;
                                    command.insert(name.to_string(), 0.0);
                                }
                                if ui
                                    .add_enabled(can && axis.enabled, egui::Button::new("180°へ"))
                                    .clicked()
                                {
                                    *field = 180.0;
                                    command.insert(name.to_string(), 180.0);
                                }
                                ui.label("");
                            } else {
                                let value =
                                    self.ee_values.entry(name.into()).or_insert(axis.initial);
                                *value = value.clamp(axis.min, axis.max);
                                ui.add(
                                    egui::DragValue::new(value)
                                        .range(axis.min..=axis.max)
                                        .speed(1.0)
                                        .suffix(format!(" {}", axis.unit()))
                                        .min_decimals(0),
                                );
                                let selected = *value;
                                if ui
                                    .add_enabled(can && axis.enabled, egui::Button::new("移動"))
                                    .on_hover_text("指定位置へ移動し、その位置を保持します")
                                    .clicked()
                                {
                                    command.insert(name.to_string(), selected);
                                }
                                if ui
                                    .add_enabled(
                                        !status.ai_active && !status.emergency,
                                        egui::Button::new("テスト"),
                                    )
                                    .clicked()
                                {
                                    self.select_ee_test(axis.target, selected);
                                }
                            }
                            if let Some(target) = status.ee_targets.get(name) {
                                ui.label(format!("指令 {target:.0}"));
                            } else if !axis.enabled {
                                ui.colored_label(WARNING, "出力未許可");
                            } else {
                                ui.label("");
                            }
                        } else {
                            ui.label("—");
                            ui.label("未割当");
                            ui.label("");
                            ui.label("");
                        }
                        ui.end_row();
                    }
                });
            let grips: Vec<_> = axes
                .iter()
                .filter(|a| a.name.starts_with("ee_grip_"))
                .collect();
            if ui
                .add_enabled(
                    can && grips.len() == 3 && grips.iter().all(|a| a.enabled),
                    egui::Button::new("把持3本をまとめて移動"),
                )
                .on_hover_text("把持1〜3を、各行に表示された指令値へ移動します")
                .clicked()
            {
                for axis in grips {
                    command.insert(axis.name.clone(), self.ee_values[&axis.name]);
                }
            }
            ui.label(
                RichText::new("PWMサーボには位置センサがないため、表示値は指令値です。")
                    .size(12.0)
                    .color(MUTED),
            );
            if !command.is_empty() {
                #[derive(serde::Serialize)]
                struct Input {
                    targets: std::collections::BTreeMap<String, f32>,
                }
                self.request(Request {
                    text: Some(toml::to_string(&Input { targets: command }).unwrap()),
                    ..Request::new("ee")
                });
            }
        });
    }
    fn ee_calibration(&mut self, ui: &mut egui::Ui) {
        use crate::application::sts::Operation;
        let status = self.shared.status_snapshot();
        let applied = self
            .shared
            .config()
            .machine
            .serial_svmd
            .and_then(|b| b.servos.into_iter().find(|s| s.name == "ee_rotation"));
        let mut operation = None;
        panel().show(ui, |ui| {
            ui.label(RichText::new("フィールド方向の2点較正").strong());
            ui.label("θの原点を採用して停止した後、較正を開始します。対象STSだけを脱力します。手でEEを0°、次に90°または180°へ向けて、それぞれ取り込んでください。");
            ui.label("取込中はθとEEを静止させてください。サーボの0/4095境界をまたがない姿勢で行います。180°まで回せない減速機構では、2点目に90°を使えます。");
            if status.sts.teach_id.is_none() {
                if ui.add_enabled(applied.is_some() && status.connected && status.configured && !status.running && !status.emergency && !status.ai_active && !status.test_mode && !status.sts.active,
                    egui::Button::new("較正開始・EE回転を脱力")).clicked() {
                    operation = Some(Operation::Teach { id: applied.as_ref().unwrap().id });
                }
            } else {
                if let Some(sample) = status.sts.samples.back() {
                    ui.label(format!("STS ID {}：実測 {} count", sample.id, sample.position));
                }
                ui.horizontal(|ui| {
                    for field_deg in [0, 90, 180] {
                        if ui.button(format!("この向きを{field_deg}°として取込")).clicked() {
                            operation = Some(Operation::Capture { field_deg });
                        }
                    }
                    if ui.button("較正終了（脱力を維持）").clicked() { operation = Some(Operation::EndTeach); }
                });
            }
            for p in [status.sts.teach_zero, status.sts.teach_half].into_iter().flatten() {
                ui.label(format!("{}°：{} count、θ={:.2}°", p.field_deg, p.count, p.theta_deg));
            }
            if let (Some(a), Some(b)) = (status.sts.teach_zero, status.sts.teach_half) {
                match ee::calibrate(a, b) {
                    Ok((offset, ratio)) => {
                        ui.label(format!("計算結果：オフセット {offset:.2} count、換算係数 {ratio:.5} count/deg"));
                        if (ratio * 180.0).abs() > 4095.0 {
                            ui.colored_label(WARNING, "この減速比では同じθで0°・180°の両方へ移動できません。現行の通常操作はサーボ単回転範囲です。");
                        }
                        if ui.add_enabled(status.sts.teach_id.is_none() && self.edit.serial_svmd.as_ref().is_some_and(|b| b.servos.iter().any(|s| s.name == "ee_rotation" && Some(s.id) == status.sts.teach_result_id)), egui::Button::new("計算結果を編集値へ反映")).clicked()
                            && let Some(s) = self.edit.serial_svmd.as_mut().and_then(|b| b.servos.iter_mut().find(|s| s.name == "ee_rotation")) {
                            s.zero0_count = offset;
                            s.counts_per_deg = ratio;
                        }
                    }
                    Err(e) => { ui.colored_label(WARNING, e.to_string()); }
                }
            }
            ui.label("較正終了 → 編集値へ反映 → 設定を適用・保存、の順で確定します。再組付けやサーボ内部の原点変更時は再較正してください。");
            ui.label(&status.sts.message);
        });
        if let Some(op) = operation {
            self.request(Request {
                text: Some(toml::to_string(&op).unwrap()),
                ..Request::new("sts")
            });
        }
    }
    pub(super) fn tune_ee(&mut self, ui: &mut egui::Ui) -> bool {
        let before = toml::to_string(&self.edit).unwrap_or_default();
        section(
            ui,
            "EEを調整",
            "サーボの接続先、移動範囲、DualSenseでの動かし方を設定します。",
        );
        ui.colored_label(
            WARNING,
            "初期値は安全な校正値ではありません。単体テストで方向と端点を確認してから通常出力を許可してください。",
        );
        let previous_axis = self.tune_ee_axis.clone();
        ui.horizontal_wrapped(|ui| {
            ui.label("調整する機構");
            for (name, label) in ee::ROLES {
                let assigned = ee::axes(&self.edit).iter().any(|axis| axis.name == name);
                let text = if assigned {
                    label.to_owned()
                } else {
                    format!("{label}（未割当）")
                };
                ui.selectable_value(&mut self.tune_ee_axis, name.into(), text);
            }
        });
        if previous_axis != self.tune_ee_axis {
            self.end_test_on_tab_change();
        }
        ui.add_space(6.0);
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            for (name, label) in ee::ROLES
                .into_iter()
                .filter(|(name, _)| *name == self.tune_ee_axis.as_str())
            {
                let exists = self.edit.pwm_servos.iter().any(|s| s.name == name)
                    || self
                        .edit
                        .serial_svmd
                        .as_ref()
                        .is_some_and(|board| board.servos.iter().any(|s| s.name == name));
                ui.horizontal(|ui| {
                    ui.label(RichText::new(label).strong().color(ACCENT));
                    if let Some(axis) = ee::axes(&self.edit).iter().find(|axis| axis.name == name) {
                        chip(
                            ui,
                            if axis.enabled {
                                "通常操作で使用"
                            } else {
                                "単体テストのみ"
                            },
                            if axis.enabled { ACCENT } else { WARNING },
                        );
                    }
                });
                if !exists {
                    if name == "ee_rotation" {
                        if ui.button("STS割当を追加（出力無効）").clicked() {
                            let board = self
                                .edit
                                .serial_svmd
                                .get_or_insert(SerialSvmdProfile { servos: vec![] });
                            let id = (1..=253)
                                .find(|id| board.servos.iter().all(|s| s.id != *id))
                                .unwrap_or(1);
                            board.servos.push(SerialServoProfile {
                                name: name.into(),
                                id,
                                input_axis: None,
                                input_sign: 1.0,
                                speed_position_per_second: 100.0,
                                acceleration: 10,
                                zero0_count: 2048.0,
                                counts_per_deg: -11.377778,
                                enabled: false,
                            });
                        }
                    } else {
                        let channel =
                            (0..4).find(|ch| self.edit.pwm_servos.iter().all(|s| s.channel != *ch));
                        if ui
                            .add_enabled(
                                channel.is_some(),
                                egui::Button::new("空いているPWM出力へ割り当て"),
                            )
                            .clicked()
                        {
                            self.edit.pwm_servos.push(PwmServoProfile {
                                name: name.into(),
                                channel: channel.unwrap(),
                                input_axis: None,
                                input_sign: 1.0,
                                speed_us_per_second: 100.0,
                                acceleration_us_per_second2: 2500.0,
                                minimum_us: 1400,
                                maximum_us: 1600,
                                initial_us: 1500,
                                enabled: false,
                            });
                        }
                    }
                    continue;
                }
                let mut remove = false;
                if let Some(s) = self.edit.pwm_servos.iter_mut().find(|s| s.name == name) {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("接続先：PWM");
                        ui.label("ch");
                        ui.add(egui::DragValue::new(&mut s.channel).range(0..=3));
                        pulse_fields(
                            ui,
                            &mut s.minimum_us,
                            &mut s.maximum_us,
                            &mut s.initial_us,
                            500..=2500,
                            "µs",
                        );
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.selectable_value(&mut s.input_sign, 1.0, "標準方向");
                        ui.selectable_value(&mut s.input_sign, -1.0, "端点を反転");
                        ui.label("最高速度");
                        ui.add(
                            egui::DragValue::new(&mut s.speed_us_per_second)
                                .range(1.0..=5000.0)
                                .suffix(" µs/s"),
                        );
                        ui.label("加減速度");
                        ui.add(
                            egui::DragValue::new(&mut s.acceleration_us_per_second2)
                                .range(1.0..=20000.0)
                                .suffix(" µs/s²"),
                        );
                    });
                    ui.label(
                        RichText::new("十字キーを押すと移動下限または上限へ移動し、離しても目標位置まで動きます。")
                            .size(12.0)
                            .color(MUTED),
                    );
                    ui.checkbox(&mut s.enabled, "通常操作でこのサーボへ出力する");
                }
                if let Some(s) = self
                    .edit
                    .serial_svmd
                    .as_mut()
                    .and_then(|board| board.servos.iter_mut().find(|s| s.name == name))
                {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("接続先：STS3215");
                        ui.label("ID");
                        ui.add(egui::DragValue::new(&mut s.id));
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label("取付オフセット");
                        ui.add(egui::DragValue::new(&mut s.zero0_count).suffix(" count"))
                            .on_hover_text("θ=0°でEEがフィールド0°を向くカウント。下の2点較正で求めます。");
                        ui.label("回転換算係数");
                        ui.add(egui::DragValue::new(&mut s.counts_per_deg).speed(0.01).suffix(" count/deg"))
                            .on_hover_text("減速比と回転方向を含み、フィールド角とθ補正に共通で使います。");
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label("サーボ内部の加速度");
                        ui.add(egui::DragValue::new(&mut s.acceleration));
                        ui.label("最高速度");
                        ui.add(
                            egui::DragValue::new(&mut s.speed_position_per_second)
                                .suffix(" count/s"),
                        );
                    });
                    ui.label(
                        RichText::new("△ボタンで0°/180°を切り替え、θの回転は回転換算係数で打ち消します。")
                            .size(12.0)
                            .color(MUTED),
                    );
                    ui.checkbox(&mut s.enabled, "通常操作でこのサーボへ出力する");
                }
                if ui.small_button("この割当を削除").clicked() {
                    remove = true;
                }
                if remove {
                    self.edit.pwm_servos.retain(|s| s.name != name);
                    if let Some(board) = &mut self.edit.serial_svmd {
                        board.servos.retain(|s| s.name != name);
                        if board.servos.is_empty() {
                            self.edit.serial_svmd = None;
                        }
                    }
                }
            }
        });
        ui.label(
            RichText::new("EE全体回転はSTS3215で、フィールド基準0°/180°を△で切り替えます。θの回転は回転換算係数で打ち消します。")
                .size(12.0)
                .color(MUTED),
        );
        if self.tune_ee_axis == "ee_rotation" {
            self.ee_calibration(ui);
        }
        before != toml::to_string(&self.edit).unwrap_or_default()
    }
}
fn pulse_fields(
    ui: &mut egui::Ui,
    min: &mut u16,
    max: &mut u16,
    initial: &mut u16,
    range: std::ops::RangeInclusive<u16>,
    unit: &str,
) {
    for (label, value, description) in [
        ("移動下限", min, "通常操作で送信できる最小値です"),
        ("移動上限", max, "通常操作で送信できる最大値です"),
        (
            "操作開始位置",
            initial,
            "停止後、DualSenseで最初に動かすときの指令値です",
        ),
    ] {
        ui.label(label);
        ui.add(
            egui::DragValue::new(value)
                .range(range.clone())
                .suffix(format!(" {unit}")),
        )
        .on_hover_text(description);
    }
}

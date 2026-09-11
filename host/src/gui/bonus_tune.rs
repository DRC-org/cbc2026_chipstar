use super::*;
use crate::{diagnostics::individual::Target, machine::bonus::HandoffLimit};

pub(super) fn encoder_status(ui: &mut egui::Ui, status: &Status) {
    ui.horizontal_wrapped(|ui| {
        match (status.bonus.encoder_count, status.bonus.encoder_age_ms) {
            (Some(count), Some(age)) if status.connected && age < 300 => {
                ui.label(format!("エンコーダ {count} count"));
                if let Some(position) = status.bonus.position_mm {
                    ui.label(format!("受け渡し位置から {position:+.2} mm"));
                }
            }
            _ => {
                ui.colored_label(WARNING, "エンコーダ応答待ち");
            }
        }
        if let Some(duty) = status.bonus.selector_output {
            ui.label(format!("基板の出力 {:+.1} %", f32::from(duty) / 10.0));
        }
        if let Some(reached) = status.bonus.handoff_limit {
            ui.label(if reached {
                "受け渡し側リミット：作動"
            } else {
                "受け渡し側リミット：開放"
            });
        }
    });
}

fn can_capture(status: &Status) -> bool {
    status.connected
        && status.configured
        && !status.running
        && !status.test_active
        && !status.bonus.active
        && !status.sts.active
        && !status.sts.busy
        && !status.ai_active
        && !status.emergency
        && status.bonus.encoder_age_ms.is_some_and(|age| age < 300)
        && status.bonus.encoder_count.is_some()
}

fn box_offset(status: &Status) -> Option<i32> {
    can_capture(status)
        .then_some(status.bonus.position_counts)
        .flatten()
}

fn duty_field(ui: &mut egui::Ui, label: &str, value: &mut u16, minimum: f32) {
    ui.label(label);
    let mut percent = f32::from(*value) / 10.0;
    if ui
        .add(
            egui::DragValue::new(&mut percent)
                .range(minimum..=100.0)
                .speed(0.1)
                .suffix(" %"),
        )
        .changed()
    {
        *value = (percent * 10.0).round() as u16;
    }
}

impl BridgeApp {
    pub(super) fn tune_bonus(&mut self, ui: &mut egui::Ui) -> bool {
        let before = self.edit.clone();
        let status = self.shared.status_snapshot();
        section(
            ui,
            "ボーナスを調整",
            "単体テストで位置を合わせ、受け渡し基準・ボックス位置・サーボ端点を登録します。変更後に「適用」「保存」を押してください。",
        );
        let Some(mut bonus) = self.edit.bonus.clone() else {
            ui.label("この機体にはボーナスが登録されていません。");
            if ui.button("ボーナス設定を追加").clicked() {
                let defaults = MachineProfile::embedded().expect("embedded profile");
                let mut bonus = defaults.bonus.unwrap();
                if let Some(motor) = self.edit.dc_motors.first() {
                    bonus.selector_motor = motor.name.clone();
                } else {
                    self.edit.dc_motors = defaults.dc_motors;
                }
                let board = self
                    .edit
                    .serial_svmd
                    .get_or_insert(crate::machine::SerialSvmdProfile { servos: vec![] });
                for mut servo in defaults
                    .serial_svmd
                    .unwrap()
                    .servos
                    .into_iter()
                    .filter(|s| s.name == bonus.lid_servo || s.name == bonus.align_servo)
                {
                    if board.servos.iter().any(|s| s.name == servo.name) {
                        continue;
                    }
                    if let Some(id) = (1..=253).find(|id| board.servos.iter().all(|s| s.id != *id))
                    {
                        servo.id = id;
                        servo.enabled = false;
                        board.servos.push(servo);
                    }
                }
                bonus.enabled = false;
                self.edit.bonus = Some(bonus);
            }
            return self.edit != before;
        };
        let can_test = status.connected
            && status.configured
            && !status.running
            && !status.emergency
            && !status.ai_active
            && !status.sts.active
            && !status.sts.busy;
        let mut test = None;
        let mut capture = false;
        ui.checkbox(&mut bonus.enabled, "通常操作でボーナスを使用する");
        ui.label("無効のままでも単体テストと位置の登録ができます。");

        panel().show(ui, |ui| {
            ui.heading("選択軸・DCモータ");
            encoder_status(ui, &status);
            ui.horizontal_wrapped(|ui| {
                ui.label("使用するDCモータ");
                egui::ComboBox::from_id_salt("bonus-motor").selected_text(&bonus.selector_motor).show_ui(ui, |ui| {
                    for motor in &self.edit.dc_motors {
                        ui.selectable_value(&mut bonus.selector_motor, motor.name.clone(), &motor.name);
                    }
                });
                if let Some(motor) = self.edit.dc_motors.iter_mut().find(|motor| motor.name == bonus.selector_motor) {
                    ui.label(format!("出力ch {}", motor.channel));
                    ui.label("カウント増加側の駆動方向");
                    ui.selectable_value(&mut motor.input_sign, 1.0, "正転");
                    ui.selectable_value(&mut motor.input_sign, -1.0, "逆転");
                }
            });
            ui.horizontal_wrapped(|ui| {
                duty_field(ui, "移動出力", &mut bonus.selector_duty, 0.1);
                duty_field(ui, "減速時の出力", &mut bonus.selector_slow_duty, 0.1);
            });
            ui.label("移動出力を単体テストと基板の上限にも反映します。単体テストの正出力でカウントが減る場合は、駆動方向を「逆転」に設定してください。");
            ui.horizontal_wrapped(|ui| {
                ui.label("AMT102-VのDIP分解能");
                egui::ComboBox::from_id_salt("bonus-encoder-ppr")
                    .selected_text(format!("{} PPR", bonus.encoder_ppr))
                    .show_ui(ui, |ui| {
                        for ppr in crate::machine::bonus::AMT102_PPR_VALUES {
                            ui.selectable_value(&mut bonus.encoder_ppr, ppr, format!("{ppr} PPR"));
                        }
                    });
                ui.label("ラック module");
                ui.add(egui::DragValue::new(&mut bonus.pinion_module_mm).range(0.1..=10.0).speed(0.1));
                ui.label("ピニオン歯数");
                ui.add(egui::DragValue::new(&mut bonus.pinion_teeth).range(1..=500));
            });
            ui.label(format!(
                "DCMD {} count/回転、移動 {:.3} mm/回転、{:.2} count/mm",
                bonus.encoder_counts_per_revolution(),
                bonus.travel_mm_per_revolution(),
                bonus.encoder_counts_per_revolution() as f32 / bonus.travel_mm_per_revolution()
            ));
            ui.horizontal_wrapped(|ui| {
                ui.label("減速を始める残り距離");
                let mut slow_zone_mm = bonus.counts_to_mm(bonus.selector_slow_zone_counts).abs();
                if ui.add(egui::DragValue::new(&mut slow_zone_mm).range(0.01..=10000.0).suffix(" mm")).changed()
                    && let Some(counts) = bonus.mm_to_counts(slow_zone_mm)
                {
                    bonus.selector_slow_zone_counts = counts.abs().max(1);
                }
                ui.label("停止位置の許容幅");
                let mut tolerance_mm = bonus.counts_to_mm(bonus.selector_tolerance_counts).abs();
                if ui.add(egui::DragValue::new(&mut tolerance_mm).range(0.001..=1000.0).suffix(" mm")).changed()
                    && let Some(counts) = bonus.mm_to_counts(tolerance_mm)
                {
                    bonus.selector_tolerance_counts = counts.abs().max(1);
                }
            });
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(can_test, egui::Button::new("DCモータを単体テスト")).clicked() {
                    test = Some((Target::Dc, 0.0));
                }
                if ui.add_enabled(can_capture(&status), egui::Button::new("現在位置を受け渡し基準に登録")).clicked() { capture = true; }
            });
            ui.label("単体テストは適用済みの設定を使います。位置を合わせて出力を解除し、この画面へ戻って登録してください。");
        });

        panel().show(ui, |ui| {
            ui.heading("ボックスの位置");
            ui.label("受け渡し基準からの距離をmmで設定します。各ボックスへ移動して「現在位置を取込」を押すこともできます。");
            if !status.bonus.handoff_captured { ui.label("先に受け渡し基準を登録してください。"); }
            let mut remove = None;
            let count = bonus.boxes.len();
            let mm_per_count = bonus.travel_mm_per_revolution()
                / bonus.encoder_counts_per_revolution() as f32;
            for (index, destination) in bonus.boxes.iter_mut().enumerate() {
                ui.push_id(index, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut destination.name).desired_width(130.0));
                        let mut offset_mm = destination.offset_counts as f32 * mm_per_count;
                        if ui.add(egui::DragValue::new(&mut offset_mm).speed(0.1).suffix(" mm")).changed()
                        {
                            destination.offset_counts = (offset_mm / mm_per_count).round() as i32;
                        }
                        let measured = box_offset(&status);
                        if ui.add_enabled(measured.is_some(), egui::Button::new("現在位置を取込")).clicked() {
                            destination.offset_counts = measured.unwrap();
                        }
                        if ui.add_enabled(count > 1, egui::Button::new("削除")).clicked() { remove = Some(index); }
                    });
                });
            }
            if let Some(index) = remove { bonus.boxes.remove(index); }
            if ui.button("ボックスを追加").clicked() {
                bonus.boxes.push(crate::machine::bonus::BoxProfile { name: format!("ボックス{}", bonus.boxes.len() + 1), offset_counts: 0 });
            }
            ui.label("再通電後は受け渡し基準を登録し直します。保存した各ボックスの相対位置はそのまま使えます。");
        });

        panel().show(ui, |ui| {
            ui.heading("受け渡し側のリミットスイッチ");
            let mut enabled = bonus.handoff_limit.is_some();
            if ui
                .checkbox(&mut enabled, "リミットスイッチを使用する")
                .changed()
            {
                bonus.handoff_limit = enabled.then_some(HandoffLimit {
                    input: 0,
                    direction: -1,
                    normally_closed: true,
                });
            }
            if let Some(limit) = &mut bonus.handoff_limit {
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("bonus-limit-input")
                        .selected_text(format!("SW{}", limit.input + 1))
                        .show_ui(ui, |ui| {
                            for input in 0..3 {
                                ui.selectable_value(
                                    &mut limit.input,
                                    input,
                                    format!("SW{}", input + 1),
                                );
                            }
                        });
                    ui.label("受け渡し側へ向かう方向");
                    ui.selectable_value(&mut limit.direction, -1, "カウント減少側");
                    ui.selectable_value(&mut limit.direction, 1, "カウント増加側");
                    ui.checkbox(&mut limit.normally_closed, "常閉（B接点）");
                });
            }
        });

        panel().show(ui, |ui| {
            ui.heading("蓋開閉・整列サーボ");
            for (key, label, name, ends, measured) in [
                ("lid", "蓋開閉", &mut bonus.lid_servo,
                    [("閉じる位置", &mut bonus.lid_closed_position), ("開ける位置", &mut bonus.lid_open_position)], status.bonus.lid_position),
                ("align", "整列", &mut bonus.align_servo,
                    [("戻る位置", &mut bonus.align_home_position), ("整列位置", &mut bonus.align_position)], status.bonus.align_position),
            ] {
                ui.push_id(key, |ui| {
                    ui.label(RichText::new(label).strong().color(ACCENT));
                    if let Some(board) = &mut self.edit.serial_svmd {
                        egui::ComboBox::from_id_salt("servo-role").selected_text(name.as_str()).show_ui(ui, |ui| {
                            for servo in &board.servos { ui.selectable_value(name, servo.name.clone(), &servo.name); }
                        });
                        if let Some(servo) = board.servos.iter_mut().find(|servo| servo.name == *name) {
                            ui.horizontal_wrapped(|ui| {
                                ui.label("ID"); ui.add(egui::DragValue::new(&mut servo.id).range(1..=253));
                                ui.label("移動速度"); ui.add(egui::DragValue::new(&mut servo.speed_position_per_second).range(0.0..=1000.0).suffix(" count/s"));
                                ui.label("加速度"); ui.add(egui::DragValue::new(&mut servo.acceleration).range(0..=254));
                                ui.checkbox(&mut servo.enabled, "通常操作で出力する");
                            });
                            ui.label("ID欄は接続先の指定です。サーボ本体のID変更は「診断 → STS3215の設定」で行います。");
                            for (label, position) in ends {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(label);
                                    ui.add(egui::DragValue::new(position).range(0..=4095).suffix(" count"));
                                    let applied_id = self.shared.config().machine.serial_svmd.as_ref()
                                        .and_then(|b| b.servos.iter().find(|s| s.name == *name)).map(|s| s.id);
                                    let applied = self.shared.config().machine;
                                    let same_role = applied.bonus.as_ref().is_some_and(|b|
                                        (if key == "lid" { &b.lid_servo } else { &b.align_servo }) == name);
                                    let measured = (same_role && applied_id == Some(servo.id)).then_some(measured).flatten()
                                        .and_then(|value| i16::try_from(value).ok());
                                    if ui.add_enabled(can_capture_servo(&status) && measured.is_some(), egui::Button::new("現在位置を取込")).clicked() {
                                        *position = measured.unwrap();
                                    }
                                    if ui.add_enabled(can_test && applied_id == Some(servo.id), egui::Button::new("単体テストへ")).clicked() {
                                        test = Some((Target::Sts(servo.id), f32::from(*position)));
                                    }
                                });
                            }
                        }
                    }
                    ui.separator();
                });
            }
            ui.horizontal_wrapped(|ui| {
                ui.label("サーボ到達の許容幅"); ui.add(egui::DragValue::new(&mut bonus.servo_tolerance_counts).range(1..=i32::MAX).suffix(" count"));
                ui.label("開閉・整列後の待機"); ui.add(egui::DragValue::new(&mut bonus.dwell_ms).suffix(" ms"));
            });
        });
        panel().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("最大保持本数");
                ui.add(egui::DragValue::new(&mut bonus.capacity).range(1..=255));
                ui.label("1サイクルの制限時間");
                ui.add(
                    egui::DragValue::new(&mut bonus.cycle_timeout_ms)
                        .range(1000..=u64::MAX)
                        .suffix(" ms"),
                );
            });
        });
        self.edit.bonus = Some(bonus);
        let duty = crate::protocol::dcmd::max_duty(&self.edit.effective_dcmd_parameters());
        egui::CollapsingHeader::new("DC基板の加減速・反転待ち・通信設定").show(ui, |ui| {
            super::dc::parameters(ui, &mut self.edit.dcmd_parameters, Some(duty));
        });
        if let Err(error) = self.edit.validate() {
            ui.colored_label(WARNING, error.to_string());
        }
        if capture {
            self.request(Request::new("bonus_capture"));
        }
        if let Some((target, value)) = test {
            if target == Target::Dc {
                self.select_dc_test();
            } else {
                self.select_ee_test(target, value);
            }
        }
        self.edit != before
    }
}

fn can_capture_servo(status: &Status) -> bool {
    status.connected
        && status.configured
        && !status.running
        && !status.test_active
        && !status.bonus.active
        && !status.sts.active
        && !status.sts.busy
        && !status.ai_active
        && !status.emergency
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_capture_requires_fresh_stopped_feedback_and_a_reference() {
        let mut status = Status {
            connected: true,
            configured: true,
            ..Status::default()
        };
        status.bonus.encoder_count = Some(200);
        status.bonus.encoder_age_ms = Some(10);
        assert_eq!(box_offset(&status), None);
        status.bonus.position_counts = Some(-123);
        assert_eq!(box_offset(&status), Some(-123));
        status.test_active = true;
        assert_eq!(box_offset(&status), None);
        status.test_active = false;
        status.bonus.encoder_age_ms = Some(500);
        assert_eq!(box_offset(&status), None);
    }
}

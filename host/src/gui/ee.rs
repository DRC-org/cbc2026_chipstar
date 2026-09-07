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
                            let value = self.ee_values.entry(name.into()).or_insert(axis.initial);
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
    pub(super) fn tune_ee(&mut self, ui: &mut egui::Ui) -> bool {
        let before = toml::to_string(&self.edit).unwrap_or_default();
        section(
            ui,
            "EEの割当・動作範囲",
            "出力を停止して編集 → 適用 → 単体テスト → 通常出力を許可",
        );
        ui.label("初期候補値は機構の安全範囲を保証しません。配線・可動範囲・開始指令を確認して設定してください。開始指令はGUIの初期値・パッド操作開始位置です。起動時には送信しません。");
        for (name, label) in ee::ROLES {
            panel().show(ui,|ui| {
                ui.set_width(ui.available_width());ui.heading(label);
                let exists=self.edit.pwm_servos.iter().any(|s|s.name==name)||self.edit.serial_svmd.as_ref().is_some_and(|b|b.servos.iter().any(|s|s.name==name));
                if !exists {
                    if name=="ee_rotation" {
                        if ui.button("STS割当を追加（出力無効）").clicked() {
                            let board=self.edit.serial_svmd.get_or_insert(SerialSvmdProfile{servos:vec![]});
                            let id=(1..=253).find(|id|board.servos.iter().all(|s|s.id!=*id)).unwrap_or(1);
                            board.servos.push(SerialServoProfile{name:name.into(),id,input_axis:None,input_sign:1.0,speed_position_per_second:100.0,minimum_position:1800,maximum_position:2200,initial_position:2048,move_speed:100,acceleration:10,enabled:false});
                        }
                    } else {
                        let channel=(0..4).find(|ch|self.edit.pwm_servos.iter().all(|s|s.channel!=*ch));
                        if ui.add_enabled(channel.is_some(),egui::Button::new("PWM割当を追加（出力無効）")).clicked() {
                            self.edit.pwm_servos.push(PwmServoProfile{name:name.into(),channel:channel.unwrap(),input_axis:None,input_sign:1.0,speed_us_per_second:100.0,minimum_us:1400,maximum_us:1600,initial_us:1500,enabled:false});
                        }
                    }
                    return;
                }
                let mut remove=false;
                if let Some(s)=self.edit.pwm_servos.iter_mut().find(|s|s.name==name) {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("PWM ch");ui.add(egui::DragValue::new(&mut s.channel).range(0..=3));
                        pulse_fields(ui,&mut s.minimum_us,&mut s.maximum_us,&mut s.initial_us,500..=2500,"µs");
                    });
                    input_fields(ui,&mut s.input_axis,&mut s.input_sign,&mut s.speed_us_per_second,"µs/s");
                    ui.checkbox(&mut s.enabled,"位置指令方式・配線・動作範囲を確認し、通常出力を許可");
                }
                if let Some(s)=self.edit.serial_svmd.as_mut().and_then(|b|b.servos.iter_mut().find(|s|s.name==name)) {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("STS ID");ui.add(egui::DragValue::new(&mut s.id).range(1..=253));
                        pulse_fields(ui,&mut s.minimum_position,&mut s.maximum_position,&mut s.initial_position,0..=4095,"count");
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label("サーボ速度上限");ui.add(egui::DragValue::new(&mut s.move_speed).range(1..=1000));
                        ui.label("加速度");ui.add(egui::DragValue::new(&mut s.acceleration).range(0..=254));
                    });
                    input_fields(ui,&mut s.input_axis,&mut s.input_sign,&mut s.speed_position_per_second,"count/s");
                    ui.checkbox(&mut s.enabled,"ID・動作範囲を確認し、通常出力を許可");
                    ui.label("STSは通常位置モード0で使用します。EE角度への換算とθ連動の姿勢補償は行いません。");
                }
                if ui.button("割当を削除").clicked() {remove=true;}
                if remove {
                    self.edit.pwm_servos.retain(|s|s.name!=name);
                    if let Some(b)=&mut self.edit.serial_svmd {b.servos.retain(|s|s.name!=name);if b.servos.is_empty(){self.edit.serial_svmd=None;}}
                }
            });
            ui.add_space(8.0);
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
    for (label, value) in [("下限", min), ("上限", max), ("開始指令", initial)] {
        ui.label(label);
        ui.add(
            egui::DragValue::new(value)
                .range(range.clone())
                .suffix(format!(" {unit}")),
        );
    }
}
fn input_fields(
    ui: &mut egui::Ui,
    axis: &mut Option<usize>,
    sign: &mut f32,
    speed: &mut f32,
    unit: &str,
) {
    ui.horizontal_wrapped(|ui| {
        egui::ComboBox::from_id_salt(ui.id().with("input"))
            .selected_text(axis.map_or("標準パッド割当".into(), |i| format!("入力軸{i}")))
            .show_ui(ui, |ui| {
                ui.selectable_value(axis, None, "標準パッド割当");
                for (i, label) in [
                    "左X（θと共用）",
                    "左Y（rと共用）",
                    "右X",
                    "右Y（zと共用）",
                    "L2",
                    "R2",
                ]
                .iter()
                .enumerate()
                {
                    ui.selectable_value(axis, Some(i), *label);
                }
            });
        ui.selectable_value(sign, 1.0, "正転");
        ui.selectable_value(sign, -1.0, "反転");
        ui.label("入力最大時の指令変化速度");
        ui.add(
            egui::DragValue::new(speed)
                .range(1.0..=5000.0)
                .suffix(format!(" {unit}")),
        );
    });
    ui.label(RichText::new("標準割当：回転＝右左右、畳み＝十字上下、把持3本＝十字左右。中立を経た最初の操作で開始指令へ移動し、以後は押している間だけ増減します。正転・反転と速度は各軸に適用。アームと同じ入力を選ぶと同時に動きます。").size(12.0).color(MUTED));
}

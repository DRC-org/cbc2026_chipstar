use super::*;
use crate::application::sequence::{self, Action as Motion, Config, Side, Stage, Start, Step};

pub(super) struct Panel {
    edit: Config,
    group: usize,
    edit_group: usize,
    side: Side,
    recipe: Stage,
}
enum Event {
    Apply,
    Save,
    Reload,
    Load,
    Start(Stage),
}
impl Panel {
    pub fn new(config: Config) -> Self {
        Self {
            edit: config,
            group: 0,
            edit_group: 0,
            side: Side::Left,
            recipe: Stage::Prepare,
        }
    }
    fn show(
        &mut self,
        ui: &mut egui::Ui,
        status: &Status,
        machine: &MachineProfile,
        applied: &Config,
        path: &str,
    ) -> Option<Event> {
        let mut event = None;
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("取得シーケンス").size(20.0).strong());
                self.group = self.group.min(applied.groups.len().saturating_sub(1));
                egui::ComboBox::from_id_salt("sequence-group")
                    .selected_text(applied.groups.get(self.group).map_or("未登録", |g| &g.name))
                    .show_ui(ui, |ui| { for (index, group) in applied.groups.iter().enumerate() {
                        ui.selectable_value(&mut self.group, index, &group.name);
                    }});
                ui.label("受渡し（フィールドに向かって）");
                ui.selectable_value(&mut self.side, Side::Left, "左");
                ui.selectable_value(&mut self.side, Side::Right, "右");
            });
            ui.horizontal_wrapped(|ui| {
                let can = !status.sequence.active && !status.emergency && !status.ai_active
                    && status.connected && status.configured && status.homing.is_none()
                    && !status.test_mode && !status.sts.active && !status.sts.busy;
                for stage in Stage::ALL {
                    if ui.add_enabled(can, egui::Button::new(stage.label())).clicked() { event = Some(Event::Start(stage)); }
                }
            });
            if status.sequence.active {
                ui.label(format!("実行中：{} / {}側　{}/{}　{}", status.sequence.group, status.sequence.side,
                    status.sequence.step, status.sequence.total, status.sequence.message));
                ui.add(egui::ProgressBar::new(status.sequence.step as f32 / status.sequence.total.max(1) as f32));
                ui.label("選択の変更は次回の実行に使います。中断は上部の停止ボタンまたは S。");
            } else if !status.sequence.message.is_empty() {
                ui.label(&status.sequence.message);
            }
            egui::CollapsingHeader::new("位置・姿勢・工程を設定").id_salt("sequence-settings").show(ui, |ui| {
                let dirty = &self.edit != applied;
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(!status.sequence.active && !status.ai_active, egui::Button::new("適用")).clicked() { event = Some(Event::Apply); }
                    if ui.add_enabled(!dirty && !status.ai_active, egui::Button::new("保存")).clicked() { event = Some(Event::Save); }
                    if ui.button("適用値に戻す").clicked() { event = Some(Event::Reload); }
                    if ui.add_enabled(!status.sequence.active && !status.ai_active, egui::Button::new("ファイル再読込")).clicked() { event = Some(Event::Load); }
                    ui.label(if dirty { "未適用の編集あり" } else if status.sequence_saved { "保存済み" } else { "未保存" });
                });
                ui.label(RichText::new(path).small().color(MUTED));
                ui.label("位置は r・z が mm、θが度。EEはサーボ指令値です。初期値は機体設定から作成します。各姿勢を登録して適用してください。");
                egui::CollapsingHeader::new("取得グループ").default_open(true).show(ui, |ui| {
                    self.edit_group = self.edit_group.min(self.edit.groups.len().saturating_sub(1));
                    ui.horizontal_wrapped(|ui| {
                        egui::ComboBox::from_id_salt("sequence-edit-group")
                            .selected_text(&self.edit.groups[self.edit_group].name).show_ui(ui, |ui| {
                                for (index, group) in self.edit.groups.iter().enumerate() {
                                    ui.selectable_value(&mut self.edit_group, index, &group.name);
                                }
                            });
                        if ui.button("複製して追加").clicked() {
                            let mut group = self.edit.groups[self.edit_group].clone();
                            group.name = format!("グループ{}", self.edit.groups.len() + 1);
                            self.edit.groups.push(group); self.edit_group = self.edit.groups.len() - 1;
                        }
                        if ui.add_enabled(self.edit.groups.len() > 1, egui::Button::new("削除")).clicked() {
                            self.edit.groups.remove(self.edit_group); self.edit_group = 0;
                        }
                    });
                    let group = &mut self.edit.groups[self.edit_group];
                    ui.horizontal(|ui| { ui.label("名前"); ui.text_edit_singleline(&mut group.name); });
                    ui.horizontal_wrapped(|ui| {
                        number(ui, "r", &mut group.r, " mm"); number(ui, "θ", &mut group.theta, " °");
                        if ui.add_enabled(status.connected, egui::Button::new("現在のr・θを登録")).clicked() {
                            capture(status, "r", &mut group.r); capture(status, "theta", &mut group.theta);
                        }
                    });
                    height(ui, status, "ワーク直上", &mut group.approach_z);
                    height(ui, status, "把持高さ", &mut group.grab_z);
                    servo(ui, status, "取得向き", "ee_rotation", &mut group.rotation, " count");
                });
                egui::CollapsingHeader::new("移動高さ・受渡し位置").default_open(true).show(ui, |ui| {
                    height(ui, status, "移動高さ", &mut self.edit.travel_z);
                    number(ui, "把持後の小上昇量", &mut self.edit.lift_mm, " mm");
                    for (label, dest) in [("左", &mut self.edit.left), ("右", &mut self.edit.right)] {
                        ui.push_id(label, |ui| {
                            ui.separator(); ui.label(format!("{label}側への受渡し"));
                            ui.horizontal_wrapped(|ui| {
                                number(ui, "r", &mut dest.r, " mm"); number(ui, "θ", &mut dest.theta, " °");
                                if ui.add_enabled(status.connected, egui::Button::new("現在のr・θを登録")).clicked() {
                                    capture(status, "r", &mut dest.r); capture(status, "theta", &mut dest.theta);
                                }
                            });
                            height(ui, status, "受渡し高さ", &mut dest.z);
                            servo(ui, status, "受渡し向き", "ee_rotation", &mut dest.rotation, " count");
                        });
                    }
                });
                egui::CollapsingHeader::new("EE姿勢").show(ui, |ui| {
                    servo(ui, status, "展開", "ee_fold", &mut self.edit.unfolded, " µs");
                    servo(ui, status, "たたみ", "ee_fold", &mut self.edit.folded, " µs");
                    for i in 0..3 {
                        let role = format!("ee_grip_{}", i + 1);
                        servo(ui, status, &format!("把持{} 開", i + 1), &role, &mut self.edit.grip_open[i], " µs");
                        servo(ui, status, &format!("把持{} 閉", i + 1), &role, &mut self.edit.grip_closed[i], " µs");
                    }
                    for axis in crate::machine::ee::axes(machine) {
                        ui.label(format!("{}：{}〜{} {}{}", axis.label, axis.min, axis.max, axis.unit(), if axis.enabled { "" } else { "（通常出力未許可）" }));
                    }
                    ui.label("サーボ割当・通常出力許可は「2 調整 → EE」で変更できます。");
                });
                egui::CollapsingHeader::new("速度・到達・待ち時間").show(ui, |ui| {
                    number(ui, "アーム速度率", &mut self.edit.speed_percent, " %");
                    number(ui, "r・z到達許容差", &mut self.edit.tolerance_mm, " mm");
                    number(ui, "θ到達許容差", &mut self.edit.tolerance_deg, " °");
                    number(ui, "工程制限時間", &mut self.edit.timeout_seconds, " s");
                    ui.label("アームは実測の到達を待ちます。EEの待ち時間は各工程で設定します。");
                });
                egui::CollapsingHeader::new("工程の順序・同時動作").show(ui, |ui| {
                    ui.horizontal(|ui| { for stage in [Stage::Prepare, Stage::Pick, Stage::Transfer] {
                        ui.selectable_value(&mut self.recipe, stage, stage.label());
                    }});
                    ui.label("上から順に実行。同じ工程内の動作は同時に開始します。");
                    let steps = self.edit.steps_mut(self.recipe);
                    let count = steps.len();
                    let mut change = None;
                    for (i, step) in steps.iter_mut().enumerate() {
                        ui.push_id(i, |ui| {
                            ui.separator();
                            ui.horizontal_wrapped(|ui| {
                                ui.label(format!("{}", i + 1));
                                ui.add(egui::TextEdit::singleline(&mut step.name).desired_width(180.0));
                                if ui.add_enabled(i > 0, egui::Button::new("↑")).clicked() { change = Some((i, -1)); }
                                if ui.add_enabled(i + 1 < count, egui::Button::new("↓")).clicked() { change = Some((i, 1)); }
                                if ui.small_button("削除").clicked() { change = Some((i, 0)); }
                                number(ui, "開始後の待ち", &mut step.wait_seconds, " s");
                            });
                            let mut remove = None;
                            for (j, action) in step.actions.iter_mut().enumerate() {
                                ui.horizontal(|ui| {
                                    egui::ComboBox::from_id_salt(("sequence-action", i, j)).selected_text(action.label()).show_ui(ui, |ui| {
                                        for option in Motion::ALL { ui.selectable_value(action, option, option.label()); }
                                    });
                                    if ui.small_button("−").clicked() { remove = Some(j); }
                                });
                            }
                            if let Some(j) = remove { step.actions.remove(j); }
                            if ui.small_button("同時動作を追加").clicked() { step.actions.push(Motion::Open); }
                        });
                    }
                    if let Some((index, direction)) = change {
                        match direction { -1 => steps.swap(index, index - 1), 1 => steps.swap(index, index + 1), _ => { steps.remove(index); } }
                    }
                    if ui.button("工程を追加").clicked() { steps.push(Step::new(Motion::TravelHeight)); }
                });
            });
        });
        event
    }
}
fn number(ui: &mut egui::Ui, label: &str, value: &mut f32, unit: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(value).speed(0.5).suffix(unit));
    });
}
fn capture(status: &Status, name: &str, value: &mut f32) {
    if let Some(axis) = status.origins.iter().find(|a| a.name == name) {
        *value = axis.position;
    }
}
fn height(ui: &mut egui::Ui, status: &Status, label: &str, value: &mut f32) {
    ui.horizontal_wrapped(|ui| {
        number(ui, label, value, " mm");
        if ui
            .add_enabled(
                status.connected,
                egui::Button::new(format!("現在zを登録：{label}")),
            )
            .clicked()
        {
            capture(status, "z", value);
        }
    });
}
fn servo(ui: &mut egui::Ui, status: &Status, label: &str, role: &str, value: &mut f32, unit: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.label(label);
        ui.add(
            egui::DragValue::new(value)
                .speed(1.0)
                .max_decimals(0)
                .suffix(unit),
        );
        if ui
            .add_enabled(
                status.ee_targets.contains_key(role),
                egui::Button::new(format!("現在指令を登録：{label}")),
            )
            .clicked()
        {
            *value = status.ee_targets[role].round();
        }
    });
}
impl BridgeApp {
    pub(super) fn operate_sequence(&mut self, ui: &mut egui::Ui, status: &Status) {
        let cfg = self.shared.config();
        let applied = self.shared.sequence_config();
        let path = sequence::config_path(&cfg.profile_path);
        let event = self.sequences.show(
            ui,
            status,
            &cfg.machine,
            &applied,
            &path.display().to_string(),
        );
        let request = match event {
            Some(Event::Apply) => Some(Request {
                text: toml::to_string(&self.sequences.edit).ok(),
                ..Request::new("sequence_apply")
            }),
            Some(Event::Save) => Some(Request::new("sequence_save")),
            Some(Event::Load) => Some(Request::new("sequence_load")),
            Some(Event::Reload) => {
                self.sequences.edit = applied;
                None
            }
            Some(Event::Start(stage)) => {
                self.active_jog = None;
                self.requested_jog = None;
                Some(Request {
                    text: toml::to_string(&Start {
                        stage,
                        group: self.sequences.group,
                        side: self.sequences.side,
                    })
                    .ok(),
                    ..Request::new("sequence_start")
                })
            }
            None => None,
        };
        if let Some(request) = request {
            let reload = request.action == "sequence_load";
            let starting = request.action == "sequence_start";
            self.request(request);
            if starting && !self.message_error {
                self.stop_requested = false;
            }
            if reload && !self.message_error {
                self.sequences.edit = self.shared.sequence_config();
            }
        }
    }
}

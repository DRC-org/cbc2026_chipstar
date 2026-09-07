//! 機体の軸割当から制御ゲインを選び、共通の設定ドラフトを編集する。
use super::*;

fn motor_prefix(slot: u8) -> Option<&'static str> {
    match slot {
        1 => Some("m3508"),
        2 => Some("m3508_slot2"),
        _ => None,
    }
}

impl BridgeApp {
    pub(super) fn tune_pid(&mut self, ui: &mut egui::Ui) -> bool {
        section(
            ui,
            "PIDゲイン調整",
            "P：誤差への反応 · I：残る誤差の補償 · D：誤差変化への反応",
        );
        ui.label(
            "編集 → 停止して適用 → 低速で確認 → 保存。適用ボタンは編集中の全設定を反映します。",
        );
        ui.label(RichText::new("位置と速度のゲインは別の制御ループです。一度に変更する項目を絞り、振動・追従・発熱を確認してください。").size(12.0).color(MUTED));
        let applied = self.shared.config().machine;
        let status = self.shared.status_snapshot();
        self.pid_response(ui);
        let axes: Vec<_> = self
            .edit
            .axes
            .iter()
            .map(|axis| (axis.name.clone(), axis.slot))
            .collect();
        let mut edited = false;
        if axes.is_empty() {
            ui.label("このプロファイルにはCCTLの軸がありません。");
        }
        for (name, slot) in axes {
            ui.add_space(12.0);
            ui.push_id(("pid", slot, &name), |ui| {
                panel().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.heading(&name);
                        chip(ui, &format!("slot {slot}"), MUTED);
                        if let Some(origin) = status.origins.iter().find(|origin| origin.name == name) {
                            ui.label(format!("現在位置 {:.2} {}", origin.position, origin.unit));
                            if !status.connected || status.telemetry_age_ms > 200 {
                                chip(ui, "実測更新なし", WARNING);
                            }
                        }
                    });
                    if let Some(prefix) = motor_prefix(slot) {
                        ui.label(RichText::new("M3508 + C620 · CCTLで位置 → 速度 → 電流を制御").color(MUTED));
                        for (kind, title) in [("pos", "位置ループ：位置誤差 → 目標rpm"), ("vel", "速度ループ：速度誤差 → 電流mA")] {
                            ui.add_space(8.0);
                            ui.label(RichText::new(title).strong().color(ACCENT));
                            egui::Grid::new(kind).num_columns(4).spacing([16.0, 8.0]).max_col_width((ui.available_width() - 400.0).max(140.0)).show(ui, |ui| {
                                for gain in ["kp", "ki", "kd"] {
                                    edited |= parameter_row(ui, &mut self.edit, &applied, &format!("{prefix}_{kind}_{gain}"), &gain.to_uppercase(), true);
                                }
                            });
                        }
                        ui.add_space(8.0);
                        ui.label(RichText::new("出力上限").strong());
                        egui::Grid::new("limits").num_columns(4).spacing([16.0, 8.0]).max_col_width((ui.available_width() - 400.0).max(140.0)).show(ui, |ui| {
                            for (suffix, label) in [("max_rpm", "速度上限"), ("max_current_ma", "電流上限")] {
                                edited |= parameter_row(ui, &mut self.edit, &applied, &format!("{prefix}_{suffix}"), label, false);
                            }
                        });
                    } else if slot == 0 {
                        ui.label(RichText::new("EL05 · モータ内部で位置制御").color(MUTED));
                        ui.label("現在の設定APIでは位置Kpのみ変更できます。位置Ki/Kd・速度ループのゲインは公開されていません。");
                        egui::Grid::new("el05").num_columns(4).spacing([16.0, 8.0]).max_col_width((ui.available_width() - 400.0).max(140.0)).show(ui, |ui| {
                            for (key, label, gain) in [("el05_loc_kp", "位置 Kp", true), ("el05_limit_spd", "速度上限", false), ("el05_limit_cur", "電流上限", false)] {
                                edited |= parameter_row(ui, &mut self.edit, &applied, key, label, gain);
                            }
                        });
                    }
                });
            });
        }
        edited
    }
}

fn parameter_row(
    ui: &mut egui::Ui,
    draft: &mut MachineProfile,
    applied: &MachineProfile,
    key: &str,
    label: &str,
    gain: bool,
) -> bool {
    let (unit, description) = parameter_help::help(key).unwrap_or(("", ""));
    let mut changed = false;
    ui.label(label)
        .on_hover_text(format!("{key}\n{description}"));
    if let Some(value) = draft.parameters.get_mut(key) {
        let step = (value.abs() * 0.01).clamp(0.001, 1.0) as f64;
        let mut editor = egui::DragValue::new(value).speed(step).max_decimals(6);
        if gain {
            editor = editor.range(0.0..=1_000_000.0);
        }
        if !unit.is_empty() {
            editor = editor.suffix(format!(" {unit}"));
        }
        changed |= ui.add(editor).on_hover_text(description).changed();
        ui.horizontal(|ui| {
            if let Some(previous) = applied.parameters.get(key) {
                let different = *previous != *value;
                ui.label(
                    RichText::new(format!("適用値 {previous}")).color(if different {
                        WARNING
                    } else {
                        MUTED
                    }),
                );
                if ui
                    .add_enabled(different, egui::Button::new("戻す"))
                    .on_hover_text("この項目の編集を破棄し、hostの適用値に戻します")
                    .clicked()
                {
                    *value = *previous;
                    changed = true;
                }
            } else {
                ui.label("未適用");
            }
        });
    } else {
        ui.label("プロファイルに未設定");
        ui.label("設定ファイルで追加してください");
    }
    ui.add(egui::Label::new(RichText::new(description).size(12.0).color(MUTED)).wrap());
    ui.end_row();
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn motor_slots_select_independent_existing_gains() {
        let profile = MachineProfile::embedded().unwrap();
        assert_eq!(motor_prefix(0), None);
        for slot in [1, 2] {
            let prefix = motor_prefix(slot).unwrap();
            for kind in ["pos", "vel"] {
                for gain in ["kp", "ki", "kd"] {
                    let key = format!("{prefix}_{kind}_{gain}");
                    assert!(profile.parameters.contains_key(&key), "{key}");
                    assert!(parameter_help::help(&key).is_some());
                }
            }
        }
        assert_ne!(motor_prefix(1), motor_prefix(2));
    }
}

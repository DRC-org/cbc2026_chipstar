use super::*;
use crate::{machine::ParameterMap, protocol::dcmd};

/// 未登録でもFWの既定値を表示し、実際に編集した項目だけを適用対象へ追加する。
pub(super) fn parameters(
    ui: &mut egui::Ui,
    values: &mut ParameterMap,
    drive_duty: Option<f32>,
) -> bool {
    let mut edited = false;
    ui.label(RichText::new("DCモータ基板").strong().color(ACCENT));
    ui.label("未設定の項目には基板の起動時初期値を表示します。変更後に「適用」「保存」を押してください。");
    egui::Grid::new("dc-parameters")
        .num_columns(3)
        .striped(true)
        .spacing([16.0, 10.0])
        .max_col_width((ui.available_width() - 330.0).max(150.0))
        .show(ui, |ui| {
            for (index, name) in dcmd::PARAMETER_NAMES.iter().enumerate() {
                if *name == "max_duty"
                    && let Some(duty) = drive_duty
                {
                    ui.label("DCモータ出力上限");
                    ui.label(format!("{:.1} %", duty / 10.0));
                    ui.label("移動出力から自動反映");
                    ui.end_row();
                    continue;
                }
                let mut value = values
                    .get(*name)
                    .copied()
                    .unwrap_or(dcmd::PARAMETER_DEFAULTS[index]);
                let (min, max) = dcmd::PARAMETER_RANGES[index];
                let (unit, help) = parameter_help::help(name).unwrap_or(("", ""));
                ui.label(parameter_help::label(name).unwrap_or(name));
                let changed = if matches!(*name, "max_duty" | "ramp_step") {
                    let mut percent = value / 10.0;
                    let changed = ui
                        .add(
                            egui::DragValue::new(&mut percent)
                                .range(min / 10.0..=max / 10.0)
                                .speed(0.1)
                                .suffix(" %"),
                        )
                        .changed();
                    value = (percent * 10.0).round();
                    changed
                } else {
                    ui.add(
                        egui::DragValue::new(&mut value)
                            .range(min..=max)
                            .max_decimals(0)
                            .suffix(format!(" {unit}")),
                    )
                    .changed()
                };
                if changed {
                    values.insert((*name).into(), value);
                    edited = true;
                }
                ui.label(RichText::new(help).size(12.0).color(MUTED));
                ui.end_row();
            }
        });
    edited
}

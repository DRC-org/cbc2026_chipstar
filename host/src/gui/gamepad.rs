//! DualSenseの生入力と、機体設定を反映した操作要求の表示。
use super::*;

pub(super) fn monitor(ui: &mut egui::Ui, status: &Status, profile: &MachineProfile) {
    let Some(input) = status.gamepad_input.as_ref() else {
        return;
    };
    let slow = status.origin_adjustment || input.buttons[9] != 0;
    let state = if status.ai_active {
        ("モニタのみ · AI操作中", WARNING)
    } else if status.screen_control {
        ("モニタのみ · 画面操作中", WARNING)
    } else if status.running && !status.emergency && !status.test_mode && status.homing.is_none() {
        ("操縦へ反映中", ACCENT)
    } else {
        ("モニタのみ · 出力停止中", MUTED)
    };
    let pressed: Vec<_> = crate::input::BUTTON_NAMES
        .iter()
        .filter_map(|(index, name)| (input.buttons[*index] != 0).then_some(*name))
        .collect();

    egui::Frame::new()
        .fill(BG)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .corner_radius(8)
        .inner_margin(8.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("DualSense入力").strong());
                chip(ui, state.0, state.1);
                ui.label(RichText::new(&status.gamepad).size(12.0).color(MUTED));
                if pressed.is_empty() {
                    ui.label(RichText::new("ボタンなし").size(12.0).color(MUTED));
                } else {
                    ui.label(RichText::new("押下中").size(12.0).color(MUTED));
                    for name in &pressed {
                        keycap(ui, name);
                    }
                }
            });

            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("操作要求").size(12.0).color(MUTED))
                    .on_hover_text(
                        "向き・低速・デッドゾーンを反映した要求値です。停止条件や可動域によって出力されない場合があります。",
                    );
                for axis in &profile.axes {
                    request_value(
                        ui,
                        if axis.name == "theta" {
                            "θ"
                        } else {
                            &axis.name
                        },
                        axis_request(axis, input, slow, profile.slow_speed_percent),
                        true,
                    );
                }
                for axis in crate::machine::ee::axes(profile) {
                    request_value(
                        ui,
                        axis.label,
                        normalized(axis.pad_value(input))
                            * axis.sign
                            * if slow {
                                profile.slow_speed_percent * 0.01
                            } else {
                                1.0
                            },
                        axis.enabled,
                    );
                }
            });
            ui.columns(3, |columns| {
                stick(
                    &mut columns[0],
                    "左スティック",
                    input.axes[0],
                    input.axes[1],
                    "LX",
                    "LY",
                );
                stick(
                    &mut columns[1],
                    "右スティック",
                    input.axes[2],
                    input.axes[3],
                    "RX",
                    "RY",
                );
                columns[2].vertical(|ui| {
                    ui.label(RichText::new("トリガー").size(12.0).color(MUTED));
                    trigger(ui, "L2", input.axes[4]);
                    trigger(ui, "R2", input.axes[5]);
                });
            });
        });
}

fn normalized(value: f32) -> f32 {
    if value.is_finite() && value.abs() >= 0.1 {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

fn axis_request(
    axis: &crate::machine::AxisProfile,
    input: &crate::input::ControllerState,
    slow: bool,
    slow_speed_percent: f32,
) -> f32 {
    axis.input_axis
        .map(|index| {
            normalized(input.machine_axis(index).unwrap_or(0.0))
                * axis.input_sign
                * if slow { slow_speed_percent * 0.01 } else { 1.0 }
        })
        .unwrap_or(0.0)
}

fn request_value(ui: &mut egui::Ui, label: &str, value: f32, enabled: bool) {
    let color = if !enabled {
        WARNING
    } else if value == 0.0 {
        MUTED
    } else {
        ACCENT
    };
    let suffix = if enabled { "" } else { "・未許可" };
    chip(
        ui,
        &format!("{label} {:+.0}%{suffix}", value * 100.0),
        color,
    );
}

fn stick(ui: &mut egui::Ui, title: &str, raw_x: f32, raw_y: f32, x_name: &str, y_name: &str) {
    let x = if raw_x.is_finite() {
        raw_x.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    let y = if raw_y.is_finite() {
        raw_y.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(title).size(12.0).color(MUTED));
        let (rect, _) = ui.allocate_exact_size(egui::vec2(72.0, 54.0), egui::Sense::hover());
        let center = rect.center();
        let radius = 21.0;
        let painter = ui.painter();
        painter.circle_stroke(center, radius, egui::Stroke::new(1.0, BORDER));
        painter.line_segment(
            [
                center - egui::vec2(radius, 0.0),
                center + egui::vec2(radius, 0.0),
            ],
            egui::Stroke::new(1.0, BORDER),
        );
        painter.line_segment(
            [
                center - egui::vec2(0.0, radius),
                center + egui::vec2(0.0, radius),
            ],
            egui::Stroke::new(1.0, BORDER),
        );
        painter.circle_stroke(center, radius * 0.1, egui::Stroke::new(1.0, WARNING));
        painter.circle_filled(
            center + egui::vec2(x * radius, -y * radius),
            5.0,
            if normalized(x).abs() > 0.0 || normalized(y).abs() > 0.0 {
                ACCENT
            } else {
                MUTED
            },
        );
        ui.label(
            RichText::new(format!("{x_name} {x:+.2}  {y_name} {y:+.2}"))
                .monospace()
                .size(11.0),
        );
    });
}

fn trigger(ui: &mut egui::Ui, name: &str, raw: f32) {
    let value = if raw.is_finite() {
        raw.clamp(0.0, 1.0)
    } else {
        0.0
    };
    ui.add(
        egui::ProgressBar::new(value)
            .text(format!("{name} {value:.2}"))
            .desired_width(ui.available_width()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_request_matches_axis_sign_deadzone_and_slow_mode() {
        let profile = MachineProfile::embedded().unwrap();
        let r = profile.axes.iter().find(|axis| axis.name == "r").unwrap();
        let mut input = crate::input::ControllerState::default();
        let input_axis = r.input_axis.unwrap();
        input.axes[input_axis] = -0.5;
        let normal = -0.5 * r.input_sign;
        assert_eq!(
            axis_request(r, &input, false, profile.slow_speed_percent),
            normal
        );
        assert!((axis_request(r, &input, true, 40.0) - normal * 0.4).abs() < f32::EPSILON);
        input.axes[input_axis] = 0.09;
        assert_eq!(
            axis_request(r, &input, false, profile.slow_speed_percent),
            0.0
        );
    }
}

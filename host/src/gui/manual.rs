use super::*;

impl BridgeApp {
    pub(super) fn manual_controls(&mut self, ui: &mut egui::Ui, status: &Status) {
        let slow_speed_percent = self.shared.config().machine.slow_speed_percent;
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("操縦入力を選択").strong());
                ui.add_enabled_ui(
                    !status.emergency && !status.test_mode && !status.ai_active && !status.running,
                    |ui| {
                        for (screen, label) in [
                            (false, "DualSense".to_owned()),
                            (true, format!("画面操作 · 低速{slow_speed_percent:.0}%")),
                        ] {
                            if ui
                                .selectable_label(status.screen_control == screen, label)
                                .clicked()
                            {
                                self.request(Request {
                                    flag: Some(screen),
                                    ..Request::new("manual_control")
                                });
                            }
                        }
                    },
                );
            });
            if status.screen_control {
                ui.label(
                    RichText::new("下のボタンを押している間だけ動き、離すとその位置で保持します。")
                        .size(12.0)
                        .color(MUTED),
                );
                let can_jog = status.running
                    && !status.emergency
                    && !status.test_mode
                    && !status.ai_active
                    && !self.stop_requested;
                ui.with_layout(
                    egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true),
                    |ui| {
                        for axis in self.shared.config().machine.axes {
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&axis.name).strong().color(ACCENT));
                                    for (value, label) in [(-1.0, "−"), (1.0, "+")] {
                                        let response = ui.add_enabled(
                                            can_jog,
                                            egui::Button::new(label)
                                                .min_size(egui::vec2(65.0, 40.0)),
                                        );
                                        if can_jog && response.is_pointer_button_down_on() {
                                            self.requested_jog = Some((axis.name.clone(), value));
                                        }
                                    }
                                });
                            });
                        }
                    },
                );
            } else {
                ui.label(
                    RichText::new(if status.gamepad.is_empty() {
                        "DualSenseが接続されていません。入力の切替は停止中に行えます。"
                    } else {
                        "左：r・θ　右：z　△：先端回転0°/180°　十字：畳み・把持　L1：低速　Options：再開　PS：停止"
                    })
                    .size(12.0)
                    .color(MUTED),
                );
            }
            gamepad::monitor(ui, status, &self.shared.config().machine);
            self.homing_controls(ui, status);
        });
    }

    pub(super) fn update_screen_input(&mut self, ctx: &egui::Context) {
        let status = self.shared.status_snapshot();
        if !ctx.input(|input| input.focused)
            || status.ai_active
            || !status.running
            || !status.screen_control
            || self.stop_requested
        {
            self.requested_jog = None;
        }
        let next = self.requested_jog.take();
        let next_axis = next.as_ref().map(|(axis, _)| axis.as_str());
        if let Some(previous) = self.active_jog.take()
            && Some(previous.as_str()) != next_axis
            && !status.ai_active
            && status.screen_control
        {
            self.send_screen_input(previous, 0.0);
        }
        if let Some((axis, value)) = next {
            self.send_screen_input(axis.clone(), value);
            self.active_jog = Some(axis);
        }
    }

    fn send_screen_input(&mut self, axis: String, value: f32) {
        let reply = self.shared.submit(
            Request {
                axis: Some(axis),
                value: Some(value),
                ..Request::new("input")
            },
            true,
        );
        if !reply.ok {
            self.message = reply.message;
            self.message_error = true;
        }
    }
}

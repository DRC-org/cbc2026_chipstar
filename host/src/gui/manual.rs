use super::*;

impl BridgeApp {
    pub(super) fn manual_controls(&mut self, ui: &mut egui::Ui, status: &Status) {
        let slow_speed_percent = self.shared.config().machine.slow_speed_percent;
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("アームの操作").strong());
            ui.label(RichText::new("DualSenseと画面操作の切替は停止中に行えます。").size(12.0).color(MUTED));
            ui.horizontal_wrapped(|ui| {
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
            if !status.screen_control {
                ui.horizontal_wrapped(|ui| {
                    ui.label("左スティックの移動");
                    for mode in [crate::machine::xy::PlanarMode::Rtheta, crate::machine::xy::PlanarMode::Xy] {
                        if ui.add_enabled(!status.ai_active && status.planar_mode_blocker.is_empty(),
                            egui::Button::new(mode.label()).selected(status.planar_mode == mode)).clicked() {
                            self.request(Request { text: Some(mode.key().into()), ..Request::new("planar_mode") });
                        }
                    }
                    keycap(ui, "L3");
                    ui.label("中立で押すと切替");
                });
                if status.planar_mode == crate::machine::xy::PlanarMode::Xy {
                    ui.label(RichText::new("上：正面へ　下：手前へ　左右：横移動（θ=0の正面を基準）").size(12.0).color(MUTED));
                    if !status.xy_blocker.is_empty() { ui.colored_label(WARNING, &status.xy_blocker); }
                }
                if !status.planar_mode_blocker.is_empty() {
                    ui.label(RichText::new(&status.planar_mode_blocker).size(12.0).color(MUTED));
                }
            }
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
                        "L2/R2：z　↑/↓：畳み　←：取得時開　→：把持閉　○：受け渡し開　△：先端反転　L1：低速　Options：再開　PS：停止"
                    })
                    .size(12.0)
                    .color(MUTED),
                );
            }
            ui.collapsing("コントローラの入力状態", |ui| {
                gamepad::monitor(ui, status, &self.shared.config().machine);
            });

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

//! 全ページ共通の出力状態、緊停、コロンコマンド入力。
use super::*;
impl BridgeApp {
    pub(super) fn global_state(&mut self, ui: &mut egui::Ui, status: &Status) {
        egui::Frame::new()
            .fill(if status.emergency {
                Color32::from_rgb(115, 39, 48)
            } else {
                SURFACE
            })
            .corner_radius(6)
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&status.operating_state).strong().size(16.0));
                    if status.test_mode {
                        chip(ui, "個別テスト", WARNING);
                        ui.label(format!(
                            "{} · {}",
                            if status.test_target.is_empty() {
                                "対象未選択"
                            } else {
                                &status.test_target
                            },
                            if status.test_active {
                                "出力要求中"
                            } else {
                                "出力停止"
                            }
                        ));
                    }
                    if ui
                        .button(if status.emergency {
                            "緊停解除  Space"
                        } else {
                            "ソフト緊停  Space"
                        })
                        .clicked()
                    {
                        self.dispatch(Action::Emergency(!status.emergency));
                    }
                    if status.test_mode && ui.button("テスト出力解除  s").clicked() {
                        self.dispatch(Action::Stop);
                    }
                });
                if status.emergency && !status.connected {
                    ui.label("接続断のため出力状態を確認できません。緊停状態を維持しています。");
                } else if status.emergency {
                    ui.label(if self.emergency_edit_guard { "全駆動出力を停止しています。Escで編集を終了してからSpace、または解除ボタンで解除できます。" } else { "全駆動出力を停止しています。解除しても運転は再開しません。" });
                }
            });
        ui.add_space(8.0);
    }
    pub(super) fn command_line(&mut self, ui: &mut egui::Ui) {
        if !self.command_open {
            return;
        }
        let mut execute = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new(":").strong().color(ACCENT));
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.command_text)
                    .id_salt("command-input")
                    .hint_text("run / stop / apply / w")
                    .desired_width(300.0),
            );
            if self.command_focus {
                response.request_focus();
                self.command_focus = false;
            }
            execute =
                response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            execute |= ui.button("実行  Enter").clicked();
            ui.label(RichText::new("Escで取消").color(MUTED));
        });
        if execute {
            self.command_open = false;
            if let Some(action) = shortcuts::command(&self.command_text) {
                self.dispatch(action);
            } else {
                self.message = format!("未対応のコマンド: {}", self.command_text);
                self.message_error = true;
            }
            self.command_text.clear();
        } else if let Some(spec) = shortcuts::COMMANDS
            .iter()
            .find(|spec| spec.name == self.command_text.trim())
        {
            ui.label(RichText::new(spec.description).size(12.0).color(MUTED));
        }
    }
}

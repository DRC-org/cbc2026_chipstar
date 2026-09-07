//! コロンコマンド入力。
use super::*;
impl BridgeApp {
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

use super::*;

impl BridgeApp {
    pub(super) fn operate_bonus(&mut self, ui: &mut egui::Ui, status: &Status) {
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("ボーナスハンド").strong());
                if ui.button("設定・動作確認").clicked() {
                    self.tune_view = tune::TuneView::Bonus;
                    self.switch_screen(Screen::Tune);
                }
            });
            super::bonus_tune::encoder_status(ui, status);
            if !status.bonus.configured {
                ui.label("「設定・動作確認」で位置と動作を確認し、通常操作を有効にしてください。");
                return;
            }
            ui.horizontal_wrapped(|ui| {
                chip(ui, if status.bonus.semi_auto { "半自動" } else { "手動" }, ACCENT);
                chip(ui, &format!("{} / {}個", status.bonus.loaded, status.bonus.capacity), ACCENT);
                ui.label(format!("状態: {}", status.bonus.phase));
            });
            ui.horizontal_wrapped(|ui| {
                if ui.selectable_label(!status.bonus.semi_auto, "手動モード").clicked() {
                    self.request(Request { flag: Some(false), ..Request::new("bonus_mode") });
                }
                if ui.selectable_label(status.bonus.semi_auto, "半自動モード").clicked() {
                    self.request(Request { flag: Some(true), ..Request::new("bonus_mode") });
                }
                if ui.add_enabled(!status.bonus.active, egui::Button::new("現在位置を受け渡し位置に登録")).clicked() {
                    self.request(Request::new("bonus_capture"));
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("シュート先");
                for (index, name) in status.bonus.box_names.iter().enumerate() {
                    if ui.selectable_label(status.bonus.selected_box == index, name).clicked() {
                        self.request(Request { value: Some(index as f32), ..Request::new("bonus_select") });
                    }
                }
                if ui.add_enabled(status.bonus.handoff_captured && !status.bonus.active, egui::Button::new("選択ボックスへシュート")).clicked() {
                    self.request(Request { value: Some(status.bonus.selected_box as f32), ..Request::new("bonus_shoot") });
                }
            });
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(!status.bonus.active, egui::Button::new("3個受け取り")).on_hover_text("EEから3個受け取った時に押します。半自動では6個でシュートを開始します。 ").clicked() {
                    self.request(Request { value: Some(3.0), ..Request::new("bonus_receive") });
                }
                if ui.add_enabled(!status.bonus.active, egui::Button::new("本数を0に戻す")).clicked() {
                    self.request(Request::new("bonus_reset"));
                }
            });
            if !status.bonus.semi_auto {
                ui.separator();
                ui.label(RichText::new("手動操作").size(12.0).color(MUTED));
                ui.horizontal_wrapped(|ui| {
                    for (label, action, flag) in [
                        ("整列する", "bonus_align", true), ("整列軸を戻す", "bonus_align", false),
                        ("蓋を開ける", "bonus_lid", true), ("蓋を閉じる", "bonus_lid", false),
                    ] {
                        if ui.add_enabled(!status.bonus.active, egui::Button::new(label)).clicked() {
                            self.request(Request { flag: Some(flag), ..Request::new(action) });
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui.add_enabled(!status.bonus.active, egui::Button::new("選択軸 −")).is_pointer_button_down_on() {
                        self.request(Request { value: Some(-1.0), ..Request::new("bonus_jog") });
                    }
                    if ui.button("選択軸 停止").clicked() {
                        self.request(Request { value: Some(0.0), ..Request::new("bonus_jog") });
                    }
                    if ui.add_enabled(!status.bonus.active, egui::Button::new("選択軸 ＋")).is_pointer_button_down_on() {
                        self.request(Request { value: Some(1.0), ..Request::new("bonus_jog") });
                    }
                });
                ui.label(RichText::new("選択軸は方向ボタンを押している間だけ動きます。受け渡し側リミットを使わない場合は、位置を合わせてから登録してください。").size(12.0).color(MUTED));
            }
            ui.separator();
            ui.label(RichText::new("DualSense: R1を押しながら Options=手動/半自動、Create=受け渡し位置登録、L3/R3=ボックス選択、×=3個受取、△=シュート。手動時は←/→=選択軸、↑/↓=整列、○/□=蓋開/閉。PSは停止。 ").size(12.0).color(MUTED));
        });
    }
}

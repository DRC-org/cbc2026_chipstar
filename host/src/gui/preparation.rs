use super::*;
use crate::application::app_state::PreparationPhase;

impl BridgeApp {
    pub(super) fn preparation_panel(&mut self, ui: &mut egui::Ui, status: &Status) -> bool {
        let phase = status.preparation;
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(match phase {
                PreparationPhase::Setting => "セッティング",
                PreparationPhase::Waiting => "開始待ち · 操作ロック中",
                PreparationPhase::Active => "競技操作",
                PreparationPhase::Recovery => "準備の再確認が必要です",
            }).size(24.0).strong());
            match phase {
                PreparationPhase::Setting => {
                    ui.label("設置・固定 → 接続確認 → 原点設定 → 開始姿勢へ配置 → 停止して確認");
                    ui.collapsing("設置・待機の条件", |ui| {
                        ui.label("エリアへ入る間は物理非常停止。固定・配線と退避を確認し、審判に完了を合図してください。");
                        ui.label("準備が間に合わない場合は、競技開始後にリトライを申告します。");
                    });
                    ui.horizontal_wrapped(|ui| {
                        for (label, ready) in [
                            ("機体接続", status.connected),
                            ("設定照合", status.configured),
                            ("操縦入力", status.screen_control || !status.gamepad.is_empty()),
                            ("全軸原点", !status.origins.is_empty() && status.origins.iter().all(|a| a.captured)),
                        ] {
                            chip(ui, &format!("{} {label}", if ready { "✓" } else { "未" }), if ready { ACCENT } else { WARNING });
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("接続・設定を確認").clicked() {
                            self.diagnosis_view = diagnose::DiagnosisView::Connection;
                            self.switch_screen(Screen::Diagnose);
                        }
                        if ui.add_enabled(Self::can_run(status), egui::Button::new("準備のために操縦を有効化")).clicked() {
                            self.dispatch(Action::Run);
                        }
                        if ui.button("配置を終えて停止・保持").clicked() {
                            self.dispatch(Action::Stop);
                        }
                    });
                    ui.label("停止操作はEE出力を解除します。停止後の実際の姿勢を確認してください。");
                    if !status.preparation_blocker.is_empty() {
                        self.preparation_confirmed = false;
                        ui.colored_label(WARNING, &status.preparation_blocker);
                    }
                    ui.add_enabled_ui(status.preparation_blocker.is_empty(), |ui| {
                        ui.checkbox(&mut self.preparation_confirmed,
                            "開始姿勢・固定・ケーブルを確認し、シューティングエリアとボックスから離れ、全員退避した");
                        if ui.add_enabled(self.preparation_confirmed, egui::Button::new("準備完了 → 開始待ち")).clicked() {
                            self.request(Request { flag: Some(true), ..Request::new("preparation_wait") });
                            self.preparation_confirmed = false;
                        }
                    });

                }
                PreparationPhase::Waiting => {
                    ui.label("設定・原点・姿勢の変更をロックしています。会場の開始合図を待ってください。");
                    ui.label("動力電源を切る操作ではありません。現在の保持状態を継続します。");
                    if !status.preparation_blocker.is_empty() {
                        ui.colored_label(WARNING, &status.preparation_blocker);
                    }
                    if ui.add_enabled(status.preparation_blocker.is_empty(),
                        egui::Button::new("合図確認 · 競技開始").min_size(egui::vec2(260.0, 56.0))).clicked() {
                        self.operation("preparation_start");
                    }
                    ui.label("開始しても自動移動は行いません。パッドのスティック・ボタンは離しておいてください。");
                    if ui.button("審判の許可を受けて準備へ戻る").clicked() {
                        self.operation("preparation_return");
                    }
                }
                PreparationPhase::Recovery => {
                    ui.colored_label(WARNING, "停止・異常により準備完了を取り消しました。解除や再接続だけでは開始できません。");
                    if ui.add_enabled(!status.emergency, egui::Button::new("準備へ戻って再確認")).clicked() {
                        self.operation("preparation_return");
                    }
                }
                PreparationPhase::Active => {
                    if ui.button("停止してセッティングへ戻る").clicked() {
                        self.operation("preparation_return");
                    }
                }
            }
        });
        if phase == PreparationPhase::Setting {
            panel().show(ui, |ui| {
                ui.label(RichText::new("原点を設定").strong());
                self.homing_controls(ui, status);
            });
        }
        !phase.locked()
    }
}

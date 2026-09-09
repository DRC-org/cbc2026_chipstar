use super::*;
use crate::application::app_state::PreparationPhase;

// 各作業は、見出し・説明・内容・操作の順で配置する。
fn task_heading(ui: &mut egui::Ui, title: &str, description: &str) {
    ui.label(RichText::new(title).size(18.0).strong());
    ui.label(RichText::new(description).size(13.0).color(MUTED));
    ui.add_space(8.0);
}

fn state_row(ui: &mut egui::Ui, label: &str, ready: bool, detail: &str) {
    ui.label(label);
    ui.colored_label(if ready { ACCENT } else { WARNING }, detail);
    ui.end_row();
}

impl BridgeApp {
    pub(super) fn preparation_panel(&mut self, ui: &mut egui::Ui, status: &Status) -> bool {
        if status.preparation == PreparationPhase::Setting {
            self.setting_tasks(ui, status);
            return false;
        }
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            match status.preparation {
                PreparationPhase::Waiting => {
                    task_heading(
                        ui,
                        "競技開始を待っています",
                        "会場の開始合図を確認してから、競技開始ボタンを押してください。",
                    );
                    ui.label("操縦と設定変更をロックしています。機体の保持状態はそのままです。");
                    ui.label("開始前に、コントローラのスティックとボタンを離してください。");
                    if !status.preparation_blocker.is_empty() {
                        ui.add_space(8.0);
                        ui.colored_label(
                            WARNING,
                            format!("開始できない理由：{}", status.preparation_blocker),
                        );
                    }
                    ui.add_space(12.0);
                    if ui
                        .add_enabled(
                            status.preparation_blocker.is_empty(),
                            egui::Button::new("競技開始").min_size(egui::vec2(220.0, 48.0)),
                        )
                        .clicked()
                    {
                        self.operation("preparation_start");
                    }
                    ui.separator();
                    ui.label(
                        RichText::new("準備をやり直す場合は、先に審判の許可を得てください。")
                            .color(MUTED),
                    );
                    if ui.button("準備に戻る").clicked() {
                        self.operation("preparation_return");
                    }
                }
                PreparationPhase::Recovery => {
                    task_heading(
                        ui,
                        "準備を確認し直してください",
                        "停止または異常を検出したため、準備完了の状態を解除しました。",
                    );
                    ui.label("機体の状態を確認し、準備画面で開始条件を確認し直してください。");
                    if status.emergency {
                        ui.colored_label(
                            WARNING,
                            "ソフト緊停中です。上部のボタンで解除してから準備に戻ってください。",
                        );
                    }
                    ui.add_space(12.0);
                    if ui
                        .add_enabled(!status.emergency, egui::Button::new("準備に戻る"))
                        .clicked()
                    {
                        self.operation("preparation_return");
                    }
                }
                PreparationPhase::Active => {
                    task_heading(
                        ui,
                        "競技の操作",
                        "下の操作パネル、または選択したコントローラで機体を操縦します。",
                    );
                    if ui.button("停止して準備に戻る").clicked() {
                        self.operation("preparation_return");
                    }
                }
                PreparationPhase::Setting => unreachable!(),
            }
        });
        !status.preparation.locked()
    }

    fn setting_tasks(&mut self, ui: &mut egui::Ui, status: &Status) {
        section(
            ui,
            "競技の準備",
            "上から順に準備し、最後に開始待ちへ進んでください。",
        );
        ui.collapsing("設置時の注意", |ui| {
            ui.label("ロボットエリアに入るときは、物理非常停止を押してください。");
            ui.label("セッティングは審判の合図に従って行ってください。準備が間に合わない場合は、競技開始後にリトライを申告します。");
        });
        ui.add_space(8.0);
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            task_heading(
                ui,
                "1. 接続を確認する",
                "機体との通信、設定の反映、操縦方法を確認します。",
            );
            egui::Grid::new("preparation-connections")
                .spacing([24.0, 6.0])
                .show(ui, |ui| {
                    state_row(
                        ui,
                        "機体",
                        status.connected,
                        if status.connected {
                            "接続済み"
                        } else {
                            "未接続"
                        },
                    );
                    state_row(
                        ui,
                        "機体設定",
                        status.configured,
                        if status.configured {
                            "反映済み"
                        } else {
                            "反映を確認できていません"
                        },
                    );
                    let input_ready = status.screen_control || !status.gamepad.is_empty();
                    state_row(
                        ui,
                        "操縦方法",
                        input_ready,
                        if status.screen_control {
                            "画面のボタンで操作"
                        } else if input_ready {
                            "DualSenseで操作"
                        } else {
                            "DualSenseが未接続です"
                        },
                    );
                });
            ui.add_space(10.0);
            if ui.button("通信設定を開く").clicked() {
                self.diagnosis_view = diagnose::DiagnosisView::Connection;
                self.switch_screen(Screen::Diagnose);
            }
        });
        ui.add_space(8.0);
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            task_heading(ui, "2. 原点を設定する", "機体の位置を測る基準を設定します。先端をシューティングボックスの反対側へ向けてください。");
            self.homing_controls(ui, status);
        });
        ui.add_space(8.0);
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            task_heading(ui, "3. 開始姿勢に合わせる", "操縦を有効にして機体を配置し、配置が終わったら停止してください。");
            ui.horizontal_wrapped(|ui| {
                chip(ui, if status.running { "操縦が有効です" } else { "操縦は停止しています" }, if status.running { ACCENT } else { MUTED });
                if ui.add_enabled(Self::can_run(status), egui::Button::new("操縦を有効にする")).clicked() {
                    self.dispatch(Action::Run);
                }
                if ui.button("停止・保持").clicked() { self.dispatch(Action::Stop); }
            });
            ui.label(RichText::new("通常操縦の停止時はアームを保持し、先端機構（EE）の出力を解除します。停止後の姿勢を確認してください。").color(MUTED));
            ui.add_space(8.0);
            ui.add_enabled_ui(!status.sequence.active, |ui| {
                if ui.available_width() < 1200.0 {
                    self.manual_controls(ui, status);
                    self.operate_ee(ui, status);
                } else {
                    ui.columns(2, |columns| {
                        self.manual_controls(&mut columns[0], status);
                        self.operate_ee(&mut columns[1], status);
                    });
                }
            });
            ui.collapsing("登録した動作を使う", |ui| { self.operate_sequence(ui, status); });
        });
        ui.add_space(8.0);
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            task_heading(
                ui,
                "4. 準備を完了する",
                "機体を停止した状態で、次の項目を目視で確認してください。",
            );
            ui.label("• 機体が固定され、規定のセッティング範囲に収まっている");
            ui.label("• ケーブルが床に触れず、規定の範囲に収まっている");
            ui.label("• 機体がシューティングエリアとボックスのどちらにも触れていない");
            ui.label("• 操縦者とピットクルーが、それぞれの待機場所に移動している");
            ui.add_space(8.0);
            if !status.preparation_blocker.is_empty() {
                self.preparation_confirmed = false;
                ui.colored_label(
                    WARNING,
                    format!("完了できない理由：{}", status.preparation_blocker),
                );
            }
            ui.add_enabled_ui(status.preparation_blocker.is_empty(), |ui| {
                ui.checkbox(&mut self.preparation_confirmed, "すべて確認しました");
                ui.add_space(8.0);
                if ui
                    .add_enabled(
                        self.preparation_confirmed,
                        egui::Button::new("開始待ちに進む"),
                    )
                    .clicked()
                {
                    self.request(Request {
                        flag: Some(true),
                        ..Request::new("preparation_wait")
                    });
                    self.preparation_confirmed = false;
                }
            });
            ui.label(
                RichText::new("開始待ちに進んだら、審判に準備完了を知らせてください。")
                    .color(MUTED),
            );
        });
    }
}

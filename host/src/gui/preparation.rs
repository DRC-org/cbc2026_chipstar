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
        if status.pad_guide {
            self.controller_preparation(ui, status);
            return status.preparation == PreparationPhase::Active;
        }
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
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            task_heading(
                ui,
                "1. コートを選ぶ",
                "使用するコートを選んでください。変更すると原点の再設定が必要です。",
            );
            ui.horizontal(|ui| {
                for (court, value) in [
                    (crate::application::app_state::Court::Red, "red"),
                    (crate::application::app_state::Court::Blue, "blue"),
                ] {
                    if ui
                        .add_enabled(
                            !status.running
                                && status.homing.is_none()
                                && !status.sequence.active
                                && !status.preparation.locked(),
                            egui::Button::new(court.label()).selected(status.court == Some(court)),
                        )
                        .clicked()
                    {
                        self.request(Request {
                            text: Some(value.into()),
                            ..Request::new("preparation_court")
                        });
                        self.homing_confirmed = false;
                    }
                }
            });
        });
        if status.court.is_none() {
            return;
        }
        ui.add_space(8.0);
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            task_heading(
                ui,
                "2. 接続を確認する",
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
            task_heading(
                ui,
                "3. 原点を設定する",
                "機体を真正面に向けてから、自動ホーミングを開始してください。",
            );
            self.homing_controls(ui, status);
        });
        ui.add_space(8.0);
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            task_heading(ui, "4. 開始姿勢に合わせる", "操縦を有効にして機体を配置し、配置が終わったら停止してください。");
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
                "5. 開始待ちにする",
                "開始姿勢に配置して停止したら、開始待ちに進んでください。",
            );
            if !status.preparation_blocker.is_empty() {
                ui.colored_label(WARNING, &status.preparation_blocker);
            }
            if ui
                .add_enabled(
                    status.preparation_blocker.is_empty(),
                    egui::Button::new("開始待ちに進む"),
                )
                .clicked()
            {
                self.operation("preparation_wait");
            }
            ui.label(
                RichText::new("開始待ちの間は操縦入力を無効にし、現在の保持状態を継続します。")
                    .color(MUTED),
            );
        });
    }
}

impl BridgeApp {
    fn controller_preparation(&mut self, ui: &mut egui::Ui, status: &Status) {
        use crate::application::app_state::PreparationStep::*;
        let (title, description, buttons) = if status.emergency {
            (
                "ソフト緊停中",
                "機体の状態を確認してください。解除しても操縦は再開しません。",
                "□を1秒長押しして離す：解除",
            )
        } else if status.preparation == PreparationPhase::Waiting {
            (
                "競技開始を待っています",
                "会場の開始合図を確認してから、明示的に操縦を開始します。",
                "Optionsを1秒長押しして離す：競技開始　／　○：準備に戻る",
            )
        } else if status.preparation == PreparationPhase::Recovery {
            (
                "準備を確認し直してください",
                "停止または異常で開始待ちを解除しました。",
                "○：接続確認に戻る",
            )
        } else if status.running {
            (
                "機体を操縦しています",
                "スティックと従来のEE割当で操縦します。十字キーは機構操作です。",
                "PS：停止・保持　／　△：先端回転　／　十字キー：畳み・把持",
            )
        } else if status.preparation == PreparationPhase::Active {
            (
                "操縦を停止しています",
                "再開するか、準備に戻る操作を選んでください。",
                "Optionsを1秒長押しして離す：操縦再開　／　○：準備に戻る",
            )
        } else {
            match status.preparation_step {
                Court => (
                    "1. コートを選ぶ",
                    "使用するコートを選択して確定してください。",
                    "←：赤　／　→：青　／　×：確定",
                ),
                Connection => (
                    "2. 接続を確認する",
                    "機体との通信と設定の反映を確認します。",
                    "×：原点設定へ　／　○：コート選択に戻る",
                ),
                Home => (
                    "3. 原点を設定する",
                    "機体を真正面に向け、移動経路に干渉がないことを確認してください。",
                    "Createを1秒長押しして離す：自動ホーミング　／　完了後に×：次へ　／　○：戻る",
                ),
                Position => (
                    "4. 開始姿勢に合わせる",
                    "操縦を有効にして配置し、PSで停止・保持してください。停止時はEE出力が解除されます。",
                    "Optionsを1秒長押しして離す：操縦　／　停止後に×：次へ　／　○：戻る",
                ),
                Finish => (
                    "5. 開始待ちにする",
                    "停止後の姿勢を確認して、開始待ちへ進んでください。",
                    "×を1秒長押しして離す：開始待ち　／　○：配置に戻る",
                ),
            }
        };
        section(
            ui,
            "コントローラで準備",
            "表示されている手順とボタンに従って操作してください。",
        );
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            task_heading(ui, title, description);
            if status.preparation == PreparationPhase::Setting && status.preparation_step == Court {
                ui.horizontal(|ui| {
                    chip(ui, "赤コート：θ −90°", if status.guide_blue { MUTED } else { ACCENT });
                    chip(ui, "青コート：θ +90°", if status.guide_blue { ACCENT } else { MUTED });
                });
            }
            if status.preparation_step == Connection {
                ui.label(format!("機体：{}　／　設定：{}", if status.connected { "接続済み" } else { "未接続" }, if status.configured { "反映済み" } else { "確認中" }));
            }
            if status.preparation_step == Home {
                if let Some(court) = status.court {
                    let config = self.shared.config();
                    let distance = |name| config.machine.axes.iter().find(|a| a.name == name)
                        .map(|a| a.homing_retreat_mm()).unwrap_or(0.0);
                    ui.label(format!("{}：真正面をθ=0°として記録します。", court.label()));
                    ui.label(format!("z下端 → zを{:.0} mm上昇 → θ {:+.0}° → r前端で原点設定 → rを{:.0} mm後退・完了", distance("z"), court.homing_theta(), distance("r")));
                }
                if let Some(homing) = &status.homing { ui.colored_label(ACCENT, homing); }
                let ready = status.origins.iter().filter(|o| o.captured).count();
                ui.label(format!("原点設定：{ready} / {} 軸", status.origins.len()));
            }
            ui.add_space(16.0);
            ui.label(RichText::new(if status.homing.is_some() { "PS：ホーミングを中断する" } else { buttons }).strong());
            if status.guide_release {
                ui.colored_label(ACCENT, "ボタンを離すと実行します。");
            }
            ui.label(RichText::new("手順が変わったら、スティックとすべてのボタンを一度離してください。PSの停止は常に受け付けます。").color(MUTED));
            if status.gamepad.is_empty() { ui.colored_label(WARNING, "DualSenseを接続してください。"); }
            if !status.preparation_blocker.is_empty() && (status.preparation_step == Finish || status.preparation == PreparationPhase::Waiting) {
                ui.colored_label(WARNING, &status.preparation_blocker);
            }
        });
        ui.add_space(8.0);
        ui.label(RichText::new("通信や機体設定の変更は「調整」「診断」、従来の操作パネルは「デバッグ」から開けます。").color(MUTED));
    }
}

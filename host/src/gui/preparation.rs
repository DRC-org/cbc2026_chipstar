use super::*;
use crate::application::app_state::{PreparationPhase, PreparationStep};

impl BridgeApp {
    // 操作名をマウス用ボタン、パッドの割当と押し方を別の列に統一する。
    fn preparation_action(
        &mut self,
        ui: &mut egui::Ui,
        label: &str,
        key: &str,
        gesture: &str,
        enabled: bool,
        action: &str,
    ) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    enabled,
                    egui::Button::new(label).min_size(egui::vec2(255.0, 40.0)),
                )
                .clicked()
            {
                match action {
                    "court_red" | "court_blue" => self.request(Request {
                        text: Some(if action == "court_red" { "red" } else { "blue" }.into()),
                        ..Request::new("preparation_court")
                    }),
                    "home" => self.request(Request {
                        flag: Some(true),
                        value: Some(self.homing_timeout),
                        ..Request::new("home")
                    }),
                    "run" => self.dispatch(Action::Run),
                    "stop" => self.dispatch(Action::Stop),
                    "estop_reset" => self.dispatch(Action::Emergency(false)),
                    _ => self.operation(action),
                }
            }
            ui.allocate_ui_with_layout(
                egui::vec2(100.0, 40.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(100.0);
                    chip(ui, key, if enabled { ACCENT } else { MUTED });
                },
            );
            ui.label(RichText::new(gesture).color(MUTED));
        });
        ui.add_space(5.0);
    }

    pub(super) fn preparation_panel(&mut self, ui: &mut egui::Ui, status: &Status) -> bool {
        use PreparationPhase::*;
        use PreparationStep::*;
        let (title, description) = if status.emergency {
            (
                "ソフト緊停中",
                "機体を確認してから解除してください。解除しても操縦は再開しません。",
            )
        } else if status.homing.is_some() {
            (
                "原点を設定しています",
                "完了すると、開始姿勢に合わせる操作が表示されます。",
            )
        } else if status.preparation == Waiting {
            (
                "競技開始を待っています",
                "会場の開始合図に合わせて競技を開始してください。",
            )
        } else if status.preparation == Recovery {
            (
                "準備操作を再開できます",
                "機体の状態を確認し、準備を再開するか、最初からやり直してください。",
            )
        } else if status.running {
            (
                "機体を操縦しています",
                "配置が終わったら停止してください。停止時はEEの出力を解除します。",
            )
        } else if status.preparation == Active {
            (
                "競技の操作",
                "操縦を再開するか、最初から準備をやり直せます。",
            )
        } else {
            match status.preparation_step {
                Court => (
                    "1. コートを選ぶ",
                    "使用するコートを選ぶと、接続状態に応じてホーミング操作を表示します。",
                ),
                Connection => (
                    "2. 機体の接続を待っています",
                    "通信と設定の反映を確認できたら、自動でホーミング操作を表示します。",
                ),
                Home => (
                    "3. 原点を設定する",
                    "機体を真正面に向け、移動経路に干渉がない状態でホーミングを開始してください。",
                ),
                Position => (
                    "4. 開始姿勢に合わせる",
                    "操縦を開始して配置し、停止したら開始待ちに切り替えてください。",
                ),
            }
        };
        section(
            ui,
            "競技の準備",
            "操作ボタンはマウスでも押せます。右側はコントローラの割当です。",
        );
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).size(22.0).strong());
            ui.label(RichText::new(description).color(MUTED));
            ui.add_space(12.0);
            if status.emergency {
                self.preparation_action(
                    ui,
                    "ソフト緊停を解除する",
                    "□",
                    "1秒長押しして離す",
                    true,
                    "estop_reset",
                );
            } else if let Some(homing) = &status.homing {
                ui.colored_label(ACCENT, homing);
                self.preparation_action(ui, "ホーミングを中断する", "PS", "押す", true, "stop");
            } else if status.preparation == Waiting {
                self.preparation_action(
                    ui,
                    "競技を開始する",
                    "Options",
                    "1秒長押しして離す",
                    status.preparation_blocker.is_empty(),
                    "preparation_start",
                );
            } else if status.preparation == Recovery {
                self.preparation_action(
                    ui,
                    "準備操作を再開する",
                    "Options",
                    "1秒長押しして離す",
                    true,
                    "preparation_return",
                );
            } else if status.running {
                self.preparation_action(ui, "停止・保持する", "PS", "押す", true, "stop");
            } else if status.preparation == Active {
                self.preparation_action(
                    ui,
                    "操縦を再開する",
                    "Options",
                    "1秒長押しして離す",
                    Self::can_run(status),
                    "run",
                );
            } else {
                match status.preparation_step {
                    Court => {
                        self.preparation_action(
                            ui,
                            "赤コートを選ぶ（θ −90°）",
                            "十字キー ←",
                            "押して離す",
                            true,
                            "court_red",
                        );
                        self.preparation_action(
                            ui,
                            "青コートを選ぶ（θ +90°）",
                            "十字キー →",
                            "押して離す",
                            true,
                            "court_blue",
                        );
                    }
                    Connection => {
                        ui.label(format!(
                            "機体：{}　／　設定：{}",
                            if status.connected {
                                "接続済み"
                            } else {
                                "未接続"
                            },
                            if status.configured {
                                "反映済み"
                            } else {
                                "確認中"
                            }
                        ));
                        if ui.button("通信設定を開く").clicked() {
                            self.diagnosis_view = diagnose::DiagnosisView::Connection;
                            self.switch_screen(Screen::Diagnose);
                        }
                    }
                    Home => {
                        let config = self.shared.config();
                        let distance = |name| {
                            config
                                .machine
                                .axes
                                .iter()
                                .find(|a| a.name == name)
                                .map(|a| a.homing_retreat_mm())
                                .unwrap_or(0.0)
                        };
                        if let Some(court) = status.court {
                            ui.label(format!(
                                "z下端 → zを{:.0} mm上昇 → θ {:+.0}° → r前端 → rを{:.0} mm後退",
                                distance("z"),
                                court.homing_theta(),
                                distance("r")
                            ));
                        }
                        ui.add_space(8.0);
                        self.preparation_action(
                            ui,
                            "自動ホーミングを開始する",
                            "Create",
                            "1秒長押しして離す",
                            status.homing_ready,
                            "home",
                        );
                    }
                    Position => {
                        self.preparation_action(
                            ui,
                            "操縦を開始する",
                            "Options",
                            "1秒長押しして離す",
                            Self::can_run(status),
                            "run",
                        );
                        self.preparation_action(
                            ui,
                            "開始待ちに切り替える",
                            "×",
                            "1秒長押しして離す",
                            status.preparation_blocker.is_empty(),
                            "preparation_wait",
                        );
                        if !status.preparation_blocker.is_empty() {
                            ui.colored_label(WARNING, &status.preparation_blocker);
                        }
                        ui.collapsing("原点を設定し直す", |ui| {
                            self.preparation_action(
                                ui,
                                "自動ホーミングを開始する",
                                "Create",
                                "1秒長押しして離す",
                                status.homing_ready,
                                "home",
                            );
                        });
                    }
                }
            }
            if status.guide_release {
                ui.colored_label(ACCENT, "ボタンを離すと実行します。");
            }
            if status.preparation == Waiting && !status.preparation_blocker.is_empty() {
                ui.colored_label(WARNING, &status.preparation_blocker);
            }
        });
        ui.add_space(8.0);
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            self.preparation_action(
                ui,
                "停止して最初に戻る",
                "○",
                "1秒長押しして離す",
                !status.ai_active,
                "preparation_restart",
            );
            ui.label(
                RichText::new("コート選択からやり直します。原点も再設定します。").color(MUTED),
            );
        });
        if status.gamepad.is_empty() {
            ui.label(RichText::new("コントローラ未接続：マウスで操作できます。").color(MUTED));
        }
        if status.preparation == Active {
            return true;
        }
        let positioning = status.preparation == Setting
            && matches!(status.preparation_step, Position)
            && status.homing.is_none();
        if !status.emergency && !status.preparation.locked() && (positioning || status.running) {
            ui.add_space(8.0);
            self.manual_controls(ui, status);
            self.operate_ee(ui, status);
            ui.collapsing("登録した動作を使う", |ui| {
                self.operate_sequence(ui, status)
            });
            if !status.screen_control && status.running {
                ui.collapsing("操縦中のコントローラ割当", |ui| {
                    egui::Grid::new("preparation-pad-help")
                        .spacing([24.0, 8.0])
                        .show(ui, |ui| {
                            for (key, action) in [
                                ("スティック", "アームを操縦"),
                                ("↑ / ↓", "畳み機構を動かす"),
                                ("← / →", "把持機構を動かす"),
                                ("△", "先端を反転する"),
                                ("PS", "停止・保持する"),
                            ] {
                                chip(ui, key, ACCENT);
                                ui.label(action);
                                ui.end_row();
                            }
                        });
                });
            }
        }
        false
    }
}

use super::*;
use crate::application::app_state::{Court as SelectedCourt, PreparationPhase, PreparationStep};

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
                    "使用するコートを選び、機体の接続を確認して原点設定へ進みます。",
                ),
                Connection => (
                    "2. 機体の接続を待っています",
                    "通信と設定の反映を確認できたら、原点設定の操作を表示します。",
                ),
                Home => (
                    "3. 原点を設定する",
                    "自動ホーミングか、手で位置を合わせる原点設定を選んでください。",
                ),
                ManualHome => (
                    "3. 手で原点を設定する",
                    "r・zをそれぞれ手でリミットに当て、θは真正面に合わせて記録してください。3軸がそろうと次へ進みます。",
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
            if status.preparation == Setting
                && status.preparation_step == ManualHome
                && !status.emergency
            {
                "×：正面のθを記録　○：最初に戻る　PS：手動設定を中断"
            } else if status.preparation == Setting
                && status.preparation_step == Home
                && !status.emergency
                && status.homing.is_none()
                && !status.running
            {
                "×：自動ホーミング　□：手動で原点設定　○：最初に戻る　PS：停止"
            } else if status.guide_restart_hold {
                "×：主操作　□：配置完了　停止中の○を1秒長押し：最初に戻る　PS：停止。操縦中の○は受け渡し開です。"
            } else {
                "×：主操作　□：配置完了　停止中の○：最初に戻る　PS：停止。操縦中の○は受け渡し開です。"
            },
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
                    "×",
                    "押して離す",
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
                    "×",
                    "押して離す",
                    status.preparation_blocker.is_empty(),
                    "preparation_start",
                );
            } else if status.preparation == Recovery {
                self.preparation_action(
                    ui,
                    "準備操作を再開する",
                    "×",
                    "押して離す",
                    true,
                    "preparation_return",
                );
            } else if status.running {
                self.preparation_action(ui, "停止・保持する", "PS", "押す", true, "stop");
            } else if status.preparation == Active {
                self.preparation_action(
                    ui,
                    "操縦を再開する",
                    "×",
                    "押して離す",
                    Self::can_run(status),
                    "run",
                );
            } else {
                match status.preparation_step {
                    Court => {
                        let angle = self.shared.config().machine.homing_theta_deg;
                        self.preparation_action(
                            ui,
                            &format!(
                                "赤コートを選ぶ（θ {:+.1}°）",
                                SelectedCourt::Red.homing_theta(angle)
                            ),
                            "十字キー ←",
                            "押して離す",
                            true,
                            "court_red",
                        );
                        self.preparation_action(
                            ui,
                            &format!(
                                "青コートを選ぶ（θ {:+.1}°）",
                                SelectedCourt::Blue.homing_theta(angle)
                            ),
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
                        ui.label(RichText::new("自動ホーミングでは、θを真正面に合わせ、z・θ・rの移動経路を確認してから開始してください。").color(MUTED));
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
                                "z下端 → zを{:.0} mm上昇 → θ {:+.1}° → r前端 → rを{:.0} mm後退",
                                distance("z"),
                                court.homing_theta(config.machine.homing_theta_deg),
                                distance("r")
                            ));
                        }
                        ui.add_space(8.0);
                        self.preparation_action(
                            ui,
                            "自動ホーミングを開始する",
                            "×",
                            "押して離す",
                            status.homing_ready,
                            "home",
                        );
                        self.preparation_action(
                            ui,
                            "手動で原点を設定する",
                            "□",
                            "押して離す（全トルク解除）",
                            status.manual_origin_blocker.is_empty(),
                            "preparation_manual_begin",
                        );
                        ui.label(RichText::new("手動設定では全トルクを解除します。機構を支えてから選んでください。").color(MUTED));
                        if !status.manual_origin_blocker.is_empty() {
                            ui.colored_label(WARNING, &status.manual_origin_blocker);
                        }
                    }
                    ManualHome => {
                        let config = self.shared.config();
                        for name in ["r", "z", "theta"] {
                            if let (Some(axis), Some(origin)) = (
                                config.machine.axes.iter().find(|axis| axis.name == name),
                                status.origins.iter().find(|origin| origin.name == name),
                            ) {
                                let label = if name == "theta" { "θ" } else { name };
                                let detail = if origin.captured {
                                    format!("原点設定済み ／ 現在 {:.1} {}", origin.position, origin.unit)
                                } else if name == "theta" {
                                    "真正面に合わせて下のボタンで記録".into()
                                } else {
                                    format!("リミット待ち ／ 到達位置を {:.1} {} として採用", axis.origin_position, axis.unit)
                                };
                                ui.label(format!("{label}：{detail}"));
                            }
                        }
                        ui.add_space(8.0);
                        self.preparation_action(ui, "正面をθ=0°として記録する", "×", "押して離す",
                            status.manual_origin_blocker.is_empty(), "preparation_manual_theta");
                        self.preparation_action(ui, "手動設定を中断する", "PS", "押す", true, "stop");
                        ui.label(RichText::new("r・zは同時に当てる必要はありません。採用後はリミットから離しても原点を維持します。").color(MUTED));
                        if !status.manual_origin_blocker.is_empty() {
                            ui.colored_label(WARNING, &status.manual_origin_blocker);
                        }
                    }
                    Position => {
                        self.preparation_action(
                            ui,
                            "操縦を開始する",
                            "×",
                            "押して離す",
                            Self::can_run(status),
                            "run",
                        );
                        self.preparation_action(
                            ui,
                            "配置を完了して開始待ちへ",
                            "□",
                            "押して離す",
                            status.preparation_blocker.is_empty(),
                            "preparation_wait",
                        );
                        if !status.preparation_blocker.is_empty() {
                            ui.colored_label(WARNING, &status.preparation_blocker);
                        }
                    }
                }
            }
            if status.guide_restart_holding {
                ui.colored_label(
                    ACCENT,
                    "○を1秒押し続けると最初に戻ります。途中で離すと取り消します。",
                );
            } else if status.guide_release {
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
                if status.running { "PS → ○" } else { "○" },
                if status.running && status.guide_restart_hold {
                    "PSで停止して離し、○を1秒長押し"
                } else if status.running {
                    "PSで停止し、離してから○"
                } else if status.guide_restart_hold {
                    "1秒長押し（短押しでは戻りません）"
                } else {
                    "押して離す"
                },
                !status.ai_active,
                "preparation_restart",
            );
            ui.label(
                RichText::new("θの保持を解除し、コート選択からやり直します。保持中のzはそのままです。原点も再設定します。").color(MUTED),
            );
        });
        if !status.operation_sound_available {
            ui.label(
                RichText::new(
                    "操作音は使用できません：CCTLが未接続、または操作音対応FWへの更新が必要です。",
                )
                .color(MUTED),
            );
        }
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
                                ("左スティック", status.planar_mode.label()),
                                ("L3", "中立でr・θ移動 / XY移動を切替"),
                                ("L2 / R2", "z下降 / z上昇"),
                                ("右スティック左右", "EEの向きを微調整・離すと保持"),
                                ("L1", "押している間は低速"),
                                ("↑ / ↓", "畳み機構を動かす"),
                                ("← / →", "取得時開 / 把持閉"),
                                ("○", "受け渡し時の開度へ動かす"),
                                ("△", "先端を反転する"),
                                ("R1 + 各ボタン", "ボーナスハンド操作（画面内の割当を参照）"),
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

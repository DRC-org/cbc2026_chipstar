//! 操縦、調整、診断。操作はローカルAPIと同じ受付を通す。
use crate::{app_state::Shared, control_api::Request, machine::MachineProfile};
use eframe::egui::{self, Color32, FontData, RichText};
use std::{sync::Arc, time::Duration};

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Operate,
    Tune,
    Diagnose,
}
pub struct BridgeApp {
    shared: Arc<Shared>,
    screen: Screen,
    edit: MachineProfile,
    source: String,
    base: String,
    message: String,
    connection: crate::app_state::Connection,
}
impl BridgeApp {
    pub fn new(shared: Arc<Shared>) -> Self {
        let edit = shared.config().machine;
        let source = toml::to_string_pretty(&edit).unwrap_or_default();
        let config = shared.config();
        let connection = crate::app_state::Connection {
            serial_device: config.serial_device,
            baud_rate: config.baud_rate,
        };
        Self {
            connection,
            shared,
            screen: Screen::Operate,
            edit,
            base: source.clone(),
            source,
            message: String::new(),
        }
    }
    fn request(&mut self, request: Request) {
        let reply = self.shared.submit(request, true);
        self.message = if reply.ok && !reply.data.is_empty() {
            reply.data
        } else {
            reply.message
        };
    }
    fn operation(&mut self, action: &str) {
        self.request(Request::new(action));
    }
    fn reload(&mut self) {
        self.edit = self.shared.config().machine;
        self.source = toml::to_string_pretty(&self.edit).unwrap_or_default();
        self.base = self.source.clone();
    }
    fn operate(&mut self, ui: &mut egui::Ui) {
        let status = self.shared.status_snapshot();
        ui.add_space(12.0);
        ui.label(
            RichText::new(if status.running {
                "操縦可能"
            } else {
                "停止中"
            })
            .size(34.0)
            .color(if status.running {
                Color32::LIGHT_GREEN
            } else {
                Color32::WHITE
            }),
        );
        ui.label(RichText::new(&status.reason).size(19.0));
        ui.add_space(18.0);
        ui.horizontal_wrapped(|ui| {
            badge(
                ui,
                if status.connected {
                    "機体 接続済み"
                } else {
                    "機体 未接続"
                },
                status.connected,
            );
            badge(
                ui,
                if status.configured {
                    "設定 一致"
                } else {
                    "設定 未確認"
                },
                status.configured,
            );
            if status.slow {
                badge(ui, "低速", true);
            }
            if status.origin_adjustment {
                ui.colored_label(Color32::YELLOW, "原点調整中・機体座標の可動域制限解除");
            }
        });
        ui.add_space(20.0);
        ui.columns(3, |columns| {
            for (i, axis) in status.origins.iter().enumerate() {
                let ui = &mut columns[i % 3];
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_min_height(130.0);
                    ui.label(
                        RichText::new(if axis.name == "theta" {
                            "θ"
                        } else {
                            &axis.name
                        })
                        .size(24.0),
                    );
                    ui.label(
                        RichText::new(format!("{:.1} {}", axis.position, axis.unit)).size(30.0),
                    );
                    ui.label(if axis.captured {
                        "原点 採用済み"
                    } else if axis.lost {
                        "原点 喪失・再確認が必要"
                    } else {
                        "原点 未採用・暫定座標"
                    });
                    if axis.at_limit == Some(true) {
                        ui.colored_label(Color32::YELLOW, "リミット到達");
                    }
                });
            }
        });
        ui.add_space(24.0);
        ui.label("左スティック：r・θ　右スティック上下：z　L1：低速");
        ui.label("Options：運転再開　PS：停止・保持");
        ui.label(if status.gamepad.is_empty() {
            "DualSense 未接続"
        } else {
            &status.gamepad
        });
        if status.simulated {
            ui.add_space(16.0);
            ui.collapsing("模擬スティック（実機出力なし）", |ui| {
                for axis in self.shared.config().machine.axes {
                    let mut value = axis.input_axis.map(|n| status.axes[n]).unwrap_or(0.0);
                    ui.horizontal(|ui| {
                        ui.label(&axis.name);
                        if ui.add(egui::Slider::new(&mut value, -1.0..=1.0)).changed() {
                            self.request(Request {
                                axis: Some(axis.name.clone()),
                                value: Some(value),
                                ..Request::new("input")
                            });
                        }
                        if ui.button("中立").clicked() {
                            self.request(Request {
                                axis: Some(axis.name),
                                value: Some(0.0),
                                ..Request::new("input")
                            });
                        }
                    });
                }
            });
        }
    }
    fn tune(&mut self, ui: &mut egui::Ui) {
        let status = self.shared.status_snapshot();
        ui.heading("原点と可動域");
        ui.label(
            "原点採用は現在位置に設定上の原点座標を割り当てます。モータのゼロ位置は変更しません。",
        );
        let mut adjustment = status.origin_adjustment;
        if ui
            .checkbox(
                &mut adjustment,
                "原点調整モード（低速固定・機体座標の可動域制限を解除）",
            )
            .changed()
        {
            self.request(Request {
                flag: Some(adjustment),
                ..Request::new("adjustment")
            });
        }
        for axis in &status.origins {
            ui.horizontal(|ui| {
                ui.label(format!("{}  {:.1} {}", axis.name, axis.position, axis.unit));
                if ui.button("現在位置を原点に採用").clicked() {
                    self.request(Request {
                        axis: Some(axis.name.clone()),
                        ..Request::new("origin")
                    });
                }
            });
        }
        ui.separator();
        ui.heading("機体設定");
        ui.label(format!(
            "保存先：{}",
            self.shared.config().profile_path.display()
        ));
        ui.label(format!(
            "{}（{}/{}項目）",
            status.configuration, status.parameters_confirmed, status.parameters_expected
        ));
        let current = toml::to_string_pretty(&self.shared.config().machine).unwrap_or_default();
        if current != self.base {
            ui.colored_label(
                Color32::YELLOW,
                "別の操作で設定が更新されました。現在の設定を読み直してください。",
            );
        }
        let mut edited = false;
        for axis in &mut self.edit.axes {
            ui.push_id(&axis.name, |ui| {
                ui.collapsing(format!("{} [{}]", axis.name, axis.unit), |ui| {
                    egui::Grid::new("axis").num_columns(2).show(ui, |ui| {
                        for (label, value) in [
                            ("通常速度 / 秒", &mut axis.speed_per_second),
                            ("最小位置", &mut axis.minimum),
                            ("最大位置", &mut axis.maximum),
                            ("原点採用時の座標", &mut axis.origin_position),
                            ("ネイティブ単位 / 機体単位", &mut axis.native_per_unit),
                        ] {
                            ui.label(label);
                            edited |= ui.add(egui::DragValue::new(value).speed(0.1)).changed();
                            ui.end_row();
                        }
                    });
                    ui.label(
                        "可動域の値は実機で校正してください。θの安全な全周回転高さはありません。",
                    );
                });
            });
        }
        if edited {
            self.source = toml::to_string_pretty(&self.edit).unwrap_or_default();
        }
        ui.collapsing("基板の調整値", |ui| {
            egui::Grid::new("parameters").striped(true).show(ui, |ui| {
                for (name, value) in &mut self.edit.parameters {
                    ui.label(name);
                    edited |= ui.add(egui::DragValue::new(value).speed(0.01)).changed();
                    ui.end_row();
                }
            });
        });
        if edited {
            self.source = toml::to_string_pretty(&self.edit).unwrap_or_default();
        }
        ui.collapsing("全設定を編集（接点・サーボを含む）", |ui| {
            if ui
                .add(
                    egui::TextEdit::multiline(&mut self.source)
                        .font(egui::TextStyle::Monospace)
                        .desired_rows(20)
                        .desired_width(f32::INFINITY),
                )
                .changed()
                && let Ok(profile) = MachineProfile::parse(&self.source)
            {
                self.edit = profile;
            }
        });
        ui.horizontal(|ui| {
            if ui.button("現在の設定を読み直す").clicked() {
                self.reload();
            }
            if ui
                .add_enabled(
                    current == self.base && !status.running,
                    egui::Button::new("一時適用"),
                )
                .clicked()
            {
                match MachineProfile::parse(&self.source) {
                    Ok(profile) => {
                        self.request(Request {
                            text: Some(self.source.clone()),
                            ..Request::new("apply")
                        });
                        if self.shared.config().machine == profile {
                            self.reload();
                        }
                    }
                    Err(error) => self.message = error.to_string(),
                }
            }
            if ui
                .add_enabled(!status.running, egui::Button::new("適用中の設定を保存"))
                .clicked()
            {
                self.operation("save");
            }
        });
        ui.label(if status.saved {
            "PCへの保存済み"
        } else {
            "PCへの保存は未確認。一時適用だけでは次回起動に引き継がれません。"
        });
    }
    fn diagnose(&mut self, ui: &mut egui::Ui) {
        let status = self.shared.status_snapshot();
        ui.heading("接続と診断");
        ui.horizontal(|ui| {
            ui.label("接続先");
            ui.text_edit_singleline(&mut self.connection.serial_device);
            ui.add(egui::DragValue::new(&mut self.connection.baud_rate));
            if ui.button("再接続").clicked() {
                self.request(Request {
                    text: Some(toml::to_string(&self.connection).unwrap()),
                    ..Request::new("connection")
                });
            }
        });
        for (board, state) in &status.peripherals {
            ui.label(format!("{board}（最終受信値）: {state}"));
        }
        ui.collapsing("モータ再初期化", |ui| {
            ui.label("モータを再通電した後に制御モードを設定し直します。完了後は原点の再確認が必要です。");
            for axis in self.shared.config().machine.axes {
                if ui.button(format!("{}を再初期化", axis.name)).clicked() { self.request(Request { axis: Some(axis.name), ..Request::new("reinit") }); }
            }
        });
        ui.label(format!("基板状態：{}", status.board_mode));
        ui.label(format!(
            "応答から {} ms / 送信 {} 行",
            status.telemetry_age_ms, status.tx_count
        ));
        ui.horizontal(|ui| {
            if ui.button("SAFE（出力を切る）").clicked() {
                self.operation("safe");
            }
            if ui.button("全出力停止").clicked() {
                self.operation("cut");
            }
        });
        if status.simulated {
            ui.horizontal(|ui| {
                for (label, fault) in [
                    ("通信断を模擬", "disconnect"),
                    ("再接続", "reconnect"),
                    ("次の指令を拒否", "reject"),
                ] {
                    if ui.button(label).clicked() {
                        self.request(Request {
                            text: Some(fault.into()),
                            ..Request::new("fault")
                        });
                    }
                }
            });
        }
        ui.collapsing("配線ガイド", |ui| {
            ui.label(include_str!("../../docs/wiring.md"));
        });
        ui.separator();
        ui.label("通信ログ（直近300行）");
        egui::ScrollArea::vertical()
            .max_height(430.0)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in &status.logs {
                    ui.monospace(line);
                }
            });
    }
}
impl eframe::App for BridgeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.ctx().request_repaint_after(Duration::from_millis(50));
        let status = self.shared.status_snapshot();
        let color = if status.ai_active {
            Color32::from_rgb(220, 48, 48)
        } else {
            ui.visuals().window_fill
        };
        egui::Frame::new()
            .stroke(egui::Stroke::new(
                if status.ai_active { 5.0 } else { 0.0 },
                color,
            ))
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                ui.horizontal(|ui| {
                    ui.heading("キャチロボクワガタ");
                    if status.simulated {
                        ui.colored_label(Color32::YELLOW, "模擬接続");
                    }
                    if status.ai_active {
                        ui.colored_label(color, "AI操作中");
                    }
                });
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.screen, Screen::Operate, "操縦");
                    ui.selectable_value(&mut self.screen, Screen::Tune, "調整");
                    ui.selectable_value(&mut self.screen, Screen::Diagnose, "診断");
                    ui.separator();
                    if ui
                        .add_sized(
                            [160.0, 42.0],
                            egui::Button::new(RichText::new("停止・保持").size(21.0))
                                .fill(Color32::from_rgb(150, 35, 35)),
                        )
                        .clicked()
                    {
                        self.operation("stop");
                    }
                    if ui
                        .add_enabled(
                            !status.ai_active && !status.running,
                            egui::Button::new("運転再開").min_size(egui::vec2(100.0, 42.0)),
                        )
                        .clicked()
                    {
                        self.operation("run");
                    }
                });
                if !status.error.is_empty() {
                    ui.colored_label(Color32::LIGHT_RED, &status.error);
                }
                if !self.message.is_empty() {
                    ui.label(&self.message);
                }
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| match self.screen {
                    Screen::Operate => self.operate(ui),
                    Screen::Tune => {
                        ui.add_enabled_ui(!status.ai_active, |ui| self.tune(ui));
                    }
                    Screen::Diagnose => self.diagnose(ui),
                });
            });
    }
    fn on_exit(&mut self) {
        self.shared.request_stop();
    }
}
fn badge(ui: &mut egui::Ui, text: &str, good: bool) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.colored_label(
            if good {
                Color32::LIGHT_GREEN
            } else {
                Color32::YELLOW
            },
            text,
        );
    });
}
pub fn install_japanese_font(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "noto_sans_jp".into(),
        FontData::from_static(include_bytes!("../assets/NotoSansJP-Regular.ttf")).into(),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "noto_sans_jp".into());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "noto_sans_jp".into());
    ctx.set_fonts(fonts);
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(Color32::from_rgb(220, 224, 230));
    ctx.set_visuals(visuals);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(16.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(16.0));
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    });
}

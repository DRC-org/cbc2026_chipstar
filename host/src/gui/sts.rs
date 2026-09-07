use super::*;
use crate::application::sts::{Operation, Step, Target};

pub(super) struct StsPanel {
    id: u8,
    register: usize,
    value: u16,
    single: bool,
    last_id: u8,
    targets: Vec<Target>,
    steps: Vec<Step>,
    path: String,
    held: bool,
    metric: usize,
    file_message: String,
}
impl Default for StsPanel {
    fn default() -> Self {
        Self {
            id: 1,
            register: 0,
            value: 1,
            single: false,
            last_id: 20,
            targets: vec![Target::default()],
            steps: vec![],
            path: "host/config/sts_sequence.toml".into(),
            held: false,
            metric: 0,
            file_message: String::new(),
        }
    }
}
const REGISTERS: &[(u8, u8, &str)] = &[
    (5, 1, "ID（1〜253）"),
    (21, 1, "位置P係数（0〜254）"),
    (23, 1, "位置I係数（0〜254）"),
    (22, 1, "位置D係数（0〜254）"),
    (6, 1, "通信速度コード（0〜7）"),
    (33, 1, "動作モード（0 / 1 / 3）"),
    (9, 2, "最小角度（0〜4095）"),
    (11, 2, "最大角度（0〜4095）"),
    (26, 1, "正方向デッドバンド"),
    (27, 1, "逆方向デッドバンド"),
    (31, 2, "位置オフセット符号化値"),
    (41, 1, "加速度（0〜254）"),
];
impl BridgeApp {
    fn sts_operation(&mut self, operation: Operation) {
        match toml::to_string(&operation) {
            Ok(text) => self.request(Request {
                text: Some(text),
                ..Request::new("sts")
            }),
            Err(error) => {
                self.message = error.to_string();
                self.message_error = true;
            }
        }
    }
    pub(super) fn sts_panel(&mut self, ui: &mut egui::Ui) {
        let status = self.shared.status_snapshot();
        let idle = status.connected
            && status.configured
            && !status.running
            && !status.test_mode
            && !status.ai_active
            && !status.emergency
            && !status.sts.busy;
        let mut operation = None;
        section(ui, "STSサーボ管理", "ID設定・監視・同期移動・シーケンス");
        ui.horizontal_wrapped(|ui| {
            ui.label(&status.sts.message);
            if status.sts.active {
                chip(ui, "出力中", WARNING);
            }
            if status.sts.busy {
                chip(ui, "処理中", MUTED);
            }
            if ui.button("停止・出力解除  s").clicked() {
                self.dispatch(Action::Stop);
            }
        });
        ui.label(
            "この画面の操作は通常操縦・個別テストと排他です。タブを離れると出力を停止します。",
        );
        panel().show(ui, |ui| {
            ui.heading("接続・内部設定");
            ui.add_enabled_ui(idle && !status.sts.active, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("対象ID"); ui.add(egui::DragValue::new(&mut self.sts_ui.id).range(1..=253));
                    ui.label("探索上限ID"); ui.add(egui::DragValue::new(&mut self.sts_ui.last_id).range(1..=253));
                    if ui.button("ID探索").clicked() { operation = Some(Operation::Scan { first: 1, last: self.sts_ui.last_id }); }
                });
                ui.label(format!("検出ID：{:?}", status.sts.discovered));
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("sts-register").selected_text(REGISTERS[self.sts_ui.register].2).show_ui(ui, |ui| {
                        for (index, (_, _, label)) in REGISTERS.iter().enumerate() { ui.selectable_value(&mut self.sts_ui.register, index, *label); }
                    });
                    let (address, width, _) = REGISTERS[self.sts_ui.register];
                    ui.add(egui::DragValue::new(&mut self.sts_ui.value).range(0..=if width == 1 { 255 } else { 4095 }));
                    if ui.button("読取り").clicked() { operation = Some(Operation::Read { id: self.sts_ui.id, address, width }); }
                    if ui.add_enabled(self.sts_ui.single, egui::Button::new("書込み・読戻し確認")).clicked() {
                        operation = Some(Operation::Configure { id: self.sts_ui.id, address, width, value: self.sts_ui.value, single_servo: self.sts_ui.single });
                    }
                });
                ui.checkbox(&mut self.sts_ui.single, "バス上に設定対象の1台だけを接続した");
            });
            ui.label("baudコード：0=1M / 1=500k / 2=250k / 3=128k / 4=115200 / 5=76800 / 6=57600 / 7=38400");
            ui.label("モード0：絶対位置、1：速度、3：相対ステップ。モード3設定時は角度上下限も0へ変更します。モード0へ戻す際は上下限を設定し直してください。");
            ui.label(RichText::new("保存設定は再通電後に読取り確認してください。オフセット変更は座標基準が変わります。").color(WARNING));
        });
        ui.add_space(10.0);
        panel().show(ui, |ui| {
            ui.heading("複数台の同期移動");
            ui.add_enabled_ui(!status.sts.active && !status.sts.busy, |ui| target_editor(ui, &mut self.sts_ui.targets, "move"));
            ui.label("モードはサーボの保存設定と一致させます。位置は通常0〜4095、角度上下限が両方0の多回転設定では±28672。速度は±1000、相対ステップは±28672カウント。速度モードでは指令値が回転速度になります。");
            let velocity = self.sts_ui.targets.iter().any(|t| t.mode == 1);
            let response = ui.add_enabled(idle || (velocity && status.sts.active), egui::Button::new(if velocity { "押している間だけ速度出力" } else { "同期移動・保持" }));
            let held = velocity && response.is_pointer_button_down_on() && ui.input(|input| input.focused) && !status.ai_active && !status.emergency;
            if velocity {
                if held && !self.sts_ui.held && idle { operation = Some(Operation::Move { targets: self.sts_ui.targets.clone() }); }
                else if held && self.sts_ui.held { operation = Some(Operation::Renew); }
                if !held && self.sts_ui.held { self.dispatch(Action::Stop); }
            } else if response.clicked() { operation = Some(Operation::Move { targets: self.sts_ui.targets.clone() }); }
            self.sts_ui.held = held;
        });
        ui.add_space(10.0);
        panel().show(ui, |ui| {
            ui.heading("シーケンス");
            ui.label("各ステップを同期送信し、指定時間待って次へ進みます。到達判定ではありません。完了時は出力を解除します。");
            ui.add_enabled_ui(!status.sts.active && !status.sts.busy, |ui| {
                let mut remove = None;
                for (index, step) in self.sts_ui.steps.iter_mut().enumerate() {
                    ui.push_id(index, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(format!("ステップ {}", index+1));
                            ui.add(egui::DragValue::new(&mut step.wait_ms).range(100..=60000).suffix(" ms待機"));
                            if ui.button("削除").clicked() { remove = Some(index); }
                        });
                        target_editor(ui, &mut step.targets, "step");
                    });
                }
                if let Some(index) = remove { self.sts_ui.steps.remove(index); }
                if ui.add_enabled(self.sts_ui.steps.len() < 64, egui::Button::new("現在の同期目標をステップ追加")).clicked() {
                    self.sts_ui.steps.push(Step { wait_ms: 1000, targets: self.sts_ui.targets.clone() });
                }
                ui.horizontal_wrapped(|ui| {
                    ui.text_edit_singleline(&mut self.sts_ui.path);
                    if ui.button("ファイル保存").clicked() {
                        self.sts_ui.file_message = (|| -> anyhow::Result<()> { let text = toml::to_string_pretty(&Operation::Sequence { steps: self.sts_ui.steps.clone() })?; std::fs::write(&self.sts_ui.path, text)?; Ok(()) })().map(|_| "保存しました".into()).unwrap_or_else(|e| e.to_string());
                    }
                    if ui.button("ファイル読込").clicked() {
                        self.sts_ui.file_message = (|| -> anyhow::Result<()> { let op: Operation = toml::from_str(&std::fs::read_to_string(&self.sts_ui.path)?)?; let Operation::Sequence { steps } = op else { anyhow::bail!("シーケンス形式ではありません"); }; self.sts_ui.steps = steps; Ok(()) })().map(|_| "読み込みました".into()).unwrap_or_else(|e| e.to_string());
                    }
                });
                ui.label(&self.sts_ui.file_message);
                if ui.add_enabled(idle && !self.sts_ui.steps.is_empty(), egui::Button::new("シーケンス実行")).clicked() { operation = Some(Operation::Sequence { steps: self.sts_ui.steps.clone() }); }
            });
        });
        ui.add_space(10.0);
        panel().show(ui, |ui| {
            ui.heading("継続監視");
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(idle, egui::Button::new("同期目標のIDを監視")).clicked() { operation = Some(Operation::Monitor { ids: self.sts_ui.targets.iter().map(|t| t.id).collect() }); }
                if ui.add_enabled(idle, egui::Button::new("監視終了")).clicked() { operation = Some(Operation::Monitor { ids: vec![] }); }
                ui.label("表示ID"); ui.add(egui::DragValue::new(&mut self.sts_ui.id).range(1..=253));
                egui::ComboBox::from_id_salt("sts-metric").selected_text(METRICS[self.sts_ui.metric]).show_ui(ui, |ui| { for (index, label) in METRICS.iter().enumerate() { ui.selectable_value(&mut self.sts_ui.metric, index, *label); } });
            });
            if let Some(s) = status.sts.samples.iter().rev().find(|s| s.id == self.sts_ui.id) {
                ui.label(format!("最終受信 {} ms前 · 位置 {} · 速度 {} · 負荷 {} · {:.1} V · {} °C · 電流 {:.1} mA · moving={}", status.sts.elapsed_ms.saturating_sub(s.elapsed_ms), s.position, s.speed, s.load, s.voltage, s.temperature, s.current_ma, s.moving));
            }
            plot(ui, &status.sts.samples, self.sts_ui.id, self.sts_ui.metric);
            ui.label(RichText::new("低周期の状態監視です。電流は6.5 mA/countで換算し、負荷はサーボの報告値を表示します。サンプル時刻を横軸に表示します。").size(12.0).color(MUTED));
        });
        if let Some(operation) = operation {
            self.sts_operation(operation);
        }
    }
}
const METRICS: &[&str] = &[
    "位置[count]",
    "速度[count/s]",
    "負荷[raw]",
    "電圧[V]",
    "温度[°C]",
    "電流[mA]",
];
fn target_editor(ui: &mut egui::Ui, targets: &mut Vec<Target>, salt: &str) {
    let mut remove = None;
    for (index, t) in targets.iter_mut().enumerate() {
        ui.push_id((salt, index), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("ID");
                ui.add(egui::DragValue::new(&mut t.id).range(1..=253));
                egui::ComboBox::from_id_salt("mode")
                    .selected_text(match t.mode {
                        1 => "速度",
                        3 => "相対ステップ",
                        _ => "位置",
                    })
                    .show_ui(ui, |ui| {
                        for (mode, label) in [(0, "位置"), (1, "速度"), (3, "相対ステップ")]
                        {
                            ui.selectable_value(&mut t.mode, mode, label);
                        }
                    });
                ui.label("指令");
                ui.add(egui::DragValue::new(&mut t.value).range(match t.mode {
                    1 => -1000..=1000,
                    3 => -28672..=28672,
                    _ => -28672..=28672,
                }));
                if t.mode != 1 {
                    ui.label("速度");
                    ui.add(egui::DragValue::new(&mut t.speed).range(0..=1000));
                }
                ui.label("加速");
                ui.add(egui::DragValue::new(&mut t.acceleration).range(0..=254));
                if ui.button("−").clicked() {
                    remove = Some(index);
                }
            })
        });
    }
    if let Some(index) = remove {
        targets.remove(index);
    }
    if ui
        .add_enabled(targets.len() < 16, egui::Button::new("サーボ追加"))
        .clicked()
    {
        let id = (1..=253)
            .find(|id| targets.iter().all(|t| t.id != *id))
            .unwrap_or(1);
        targets.push(Target {
            id,
            ..Target::default()
        });
    }
}
fn plot(
    ui: &mut egui::Ui,
    samples: &std::collections::VecDeque<crate::application::sts::Sample>,
    id: u8,
    metric: usize,
) {
    let points: Vec<_> = samples
        .iter()
        .filter(|s| s.id == id)
        .map(|s| {
            (
                s.elapsed_ms as f32 / 1000.0,
                match metric {
                    0 => s.position as f32,
                    1 => s.speed as f32,
                    2 => s.load as f32,
                    3 => s.voltage,
                    4 => f32::from(s.temperature),
                    _ => s.current_ma,
                },
            )
        })
        .collect();
    if points.len() < 2 {
        ui.label("グラフ：2サンプル以上の受信を待っています");
        return;
    }
    let x0 = points[0].0;
    let x1 = points.last().unwrap().0.max(x0 + 0.001);
    let min = points.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
    let max = points
        .iter()
        .map(|p| p.1)
        .fold(f32::NEG_INFINITY, f32::max)
        .max(min + 1.0);
    ui.label(format!("{x0:.1}〜{x1:.1}秒 · 縦軸 {min:.2}〜{max:.2}"));
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 160.0),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(rect, 4.0, BG);
    let points = points
        .into_iter()
        .map(|(x, y)| {
            egui::pos2(
                rect.left() + (x - x0) / (x1 - x0) * rect.width(),
                rect.bottom() - (y - min) / (max - min) * rect.height(),
            )
        })
        .collect();
    ui.painter()
        .add(egui::Shape::line(points, egui::Stroke::new(1.5, ACCENT)));
}

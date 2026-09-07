use super::*;
use crate::application::response_history::{self, Sample};
use std::collections::VecDeque;

pub(super) struct Panel {
    axis: String,
    seconds: f64,
    frozen: Option<VecDeque<Arc<Sample>>>,
    path: String,
    message: String,
}
impl Default for Panel {
    fn default() -> Self {
        Self {
            axis: String::new(),
            seconds: 10.0,
            frozen: None,
            path: "pid-response.csv".into(),
            message: String::new(),
        }
    }
}
impl BridgeApp {
    pub(super) fn pid_response(&mut self, ui: &mut egui::Ui) {
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.heading("位置応答・追従誤差");
            ui.label("全軸を常時記録（直近120秒・最大6000点）。表示停止はモータを停止しません。");
            ui.horizontal_wrapped(|ui| {
                let paused = self.pid_plot.frozen.is_some();
                if ui.button(if paused { "ライブ表示へ戻る" } else { "表示を停止" }).clicked() {
                    self.pid_plot.frozen = if paused { None } else { Some(self.shared.response_history.lock().unwrap().samples.clone()) };
                }
                if ui.button("履歴クリア").clicked() {
                    self.shared.response_history.lock().unwrap().samples.clear();
                    if let Some(samples)=&mut self.pid_plot.frozen {samples.clear();}
                }
                ui.label("表示幅");
                for seconds in [5.0,10.0,30.0,60.0,120.0] { ui.selectable_value(&mut self.pid_plot.seconds,seconds,format!("{seconds:.0}秒")); }
            });
            let samples = self.pid_plot.frozen.clone().unwrap_or_else(|| self.shared.response_history.lock().unwrap().samples.clone());
            let names: std::collections::BTreeSet<_> = samples.iter().flat_map(|s| s.axes.iter().map(|a| a.name.clone())).collect();
            if !names.contains(&self.pid_plot.axis) { self.pid_plot.axis=names.first().cloned().unwrap_or_default(); }
            ui.horizontal_wrapped(|ui| {
                ui.label("軸");
                for name in names { ui.selectable_value(&mut self.pid_plot.axis,name.clone(),name); }
                ui.colored_label(WARNING,"━ 目標位置");ui.colored_label(ACCENT,"━ 実測位置");
                if self.pid_plot.frozen.is_some() { chip(ui,"表示停止中",WARNING); }
                else if !self.shared.status_snapshot().connected || self.shared.status_snapshot().telemetry_age_ms > 250 { chip(ui,"受信更新なし",DANGER); }
            });
            let end=samples.back().map_or(0.0,|s|s.seconds);
            let start=(end-self.pid_plot.seconds).max(0.0);
            let points:Vec<_>=samples.iter().filter(|s|s.seconds>=start).filter_map(|s|s.axes.iter().find(|a|a.name==self.pid_plot.axis).map(|a|(s.as_ref(),a))).collect();
            if let Some((_,last))=points.last() {
                ui.label(format!("位置 {:.3} / 目標 {:.3} {} · 誤差 {:+.3} {}{}",last.measured,last.target,last.unit,last.target-last.measured,last.unit,if last.valid {""} else {" · 実測無効"}));
                let errors:Vec<_>=points.iter().filter(|(_,a)|a.valid && a.enabled && a.origin_confirmed).map(|(_,a)|f64::from(a.target-a.measured)).collect();
                if !errors.is_empty() {
                    let rms=(errors.iter().map(|e|e*e).sum::<f64>()/errors.len() as f64).sqrt();
                    let peak=errors.iter().map(|e|e.abs()).fold(0.0,f64::max);
                    ui.label(format!("出力中・原点確認済みの誤差：RMS {rms:.3} · 最大絶対値 {peak:.3} {} · {}点",last.unit,errors.len())).on_hover_text("RMSは誤差の二乗平均平方根。停止中と原点未確認のサンプルは統計から除外します。");
                }
                chart(ui,&points,start,end.max(start+0.01),false);
                chart(ui,&points,start,end.max(start+0.01),true);
            } else { ui.label("軸の計測データを待っています。"); }
            ui.label(RichText::new("CCTLの配信周期で取得する位置応答です。速度PID内部のrpm・電流波形は含みません。欠測や原点変更の区間は線を切ります。").color(MUTED).size(12.0));
            ui.horizontal_wrapped(|ui| {
                ui.text_edit_singleline(&mut self.pid_plot.path);
                if ui.add_enabled(!samples.is_empty(),egui::Button::new("全軸の履歴をCSV保存")).clicked() {
                    self.pid_plot.message=std::fs::write(&self.pid_plot.path,response_history::csv(&samples)).map(|_|"保存しました".into()).unwrap_or_else(|e|e.to_string());
                }
                ui.label(&self.pid_plot.message);
            });
        });
    }
}
fn chart(
    ui: &mut egui::Ui,
    points: &[(&Sample, &response_history::AxisSample)],
    start: f64,
    end: f64,
    error: bool,
) {
    let values = |a: &response_history::AxisSample| {
        if error {
            [a.target - a.measured, 0.0]
        } else {
            [a.target, a.measured]
        }
    };
    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    for (_, a) in points.iter().filter(|(_, a)| a.valid) {
        for v in values(a) {
            lo = lo.min(v);
            hi = hi.max(v);
        }
    }
    if !lo.is_finite() {
        ui.label("有効な実測なし");
        return;
    }
    let pad = ((hi - lo) * 0.1).max(0.01);
    lo -= pad;
    hi += pad;
    ui.label(if error {
        "追従誤差（目標 − 実測）"
    } else {
        "位置"
    });
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), if error { 120.0 } else { 200.0 }),
        egui::Sense::hover(),
    );
    let plot = rect.shrink2(egui::vec2(55.0, 18.0));
    ui.painter().rect_filled(rect, 4.0, BG);
    let xy = |t: f64, v: f32| {
        egui::pos2(
            plot.left() + ((t - start) / (end - start)) as f32 * plot.width(),
            plot.bottom() - (v - lo) / (hi - lo) * plot.height(),
        )
    };
    for i in 0..=4 {
        let v = lo + (hi - lo) * i as f32 / 4.0;
        let y = xy(start, v).y;
        ui.painter().line_segment(
            [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
            egui::Stroke::new(0.5, BORDER),
        );
        ui.painter().text(
            egui::pos2(plot.left() - 4.0, y),
            egui::Align2::RIGHT_CENTER,
            format!("{v:.2}"),
            egui::FontId::monospace(10.0),
            MUTED,
        );
        let t = start + (end - start) * f64::from(i) / 4.0;
        ui.painter().text(
            egui::pos2(xy(t, lo).x, rect.bottom() - 2.0),
            egui::Align2::CENTER_BOTTOM,
            format!("{:.1}s", t - end),
            egui::FontId::monospace(10.0),
            MUTED,
        );
    }
    for pair in points.windows(2) {
        let [(s, a), (t, b)] = pair else { continue };
        if !response_history::continuous(s, a, t, b) {
            continue;
        }
        for channel in 0..if error { 1 } else { 2 } {
            ui.painter().line_segment(
                [
                    xy(s.seconds, values(a)[channel]),
                    xy(t.seconds, values(b)[channel]),
                ],
                egui::Stroke::new(
                    1.5,
                    if error {
                        DANGER
                    } else if channel == 0 {
                        WARNING
                    } else {
                        ACCENT
                    },
                ),
            );
        }
    }
    if let Some(pos) = response.hover_pos().filter(|p| plot.contains(*p)) {
        let time = start + f64::from((pos.x - plot.left()) / plot.width()) * (end - start);
        if let Some((s, a)) = points.iter().min_by(|(a, _), (b, _)| {
            (a.seconds - time)
                .abs()
                .total_cmp(&(b.seconds - time).abs())
        }) {
            let x = xy(s.seconds, lo).x;
            ui.painter().line_segment(
                [egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())],
                egui::Stroke::new(1.0, MUTED),
            );
            response.on_hover_text(format!(
                "{:.3}s · 目標 {:.3} · 実測 {:.3} · 誤差 {:+.3} {}{}",
                s.seconds - end,
                a.target,
                a.measured,
                a.target - a.measured,
                a.unit,
                if a.valid { "" } else { "（無効）" }
            ));
        }
    }
}

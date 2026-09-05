//! DualSense コントローラの入力を読み取り、cctl (STM32) へ USB-CDC シリアルで
//! 送信するブリッジ。起動と同時に egui の GUI を表示し、ステータス確認・設定・
//! 操作を行う。実際のブリッジ処理はワーカースレッドで動作する。

mod app_state;
mod controller;
mod dcmd;
mod device;
mod frame;
mod fw_test;
mod fw_test_gui;
mod fw_test_transport;
mod gui;
mod inputs;
mod keymap;
mod machine;
mod serial;
mod svmd;
mod telemetry;
mod worker;

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use clap::Parser;
use eframe::egui;

use crate::app_state::{BridgeConfig, Shared};
use crate::machine::MachineProfile;

#[derive(Parser)]
#[command(about = "DualSense controller to cctl USB-CDC bridge (GUI)")]
struct Args {
    /// 送信先シリアルデバイス（GUI の初期値）。
    #[arg(short, long, default_value = "/dev/ttyACM0")]
    serial_device: String,

    /// ボーレート（GUI の初期値）。
    #[arg(short, long, default_value_t = 115200)]
    baud_rate: u32,

    /// 送信周期（Hz、GUI の初期値）。
    #[arg(short, long, default_value_t = 20.0)]
    rate_hz: f64,

    /// 機体固有の軸構成。省略時は内蔵のrthetaプロファイルを使う。
    #[arg(long)]
    machine_profile: Option<PathBuf>,
}

fn main() -> eframe::Result<()> {
    let args = Args::parse();
    let machine = MachineProfile::load(args.machine_profile.as_deref())
        .unwrap_or_else(|err| panic!("{err:#}"));

    let shared = Arc::new(Shared::new(BridgeConfig {
        serial_device: args.serial_device,
        baud_rate: args.baud_rate,
        rate_hz: args.rate_hz,
        machine,
    }));

    // ブリッジ処理をワーカースレッドで起動する。
    let worker_shared = shared.clone();
    let worker = thread::spawn(move || worker::run(worker_shared));

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([840.0, 520.0])
            .with_title("host - controller bridge"),
        ..Default::default()
    };

    let app_shared = shared.clone();
    let result = eframe::run_native(
        "host",
        options,
        Box::new(move |cc| {
            gui::install_japanese_font(&cc.egui_ctx);
            Ok(Box::new(gui::BridgeApp::new(app_shared)))
        }),
    );

    // GUI 終了後、ワーカーを停止させて回収する。
    shared.request_stop();
    let _ = worker.join();

    result
}

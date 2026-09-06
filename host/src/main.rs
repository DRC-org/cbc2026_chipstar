//! 機体操作GUIと、同じ接続を使うローカル操作API。
use clap::Parser;
use eframe::egui;
use host::{
    application::{
        app_state::{BridgeConfig, Shared},
        worker,
    },
    gui,
    interface::{api_server, control_api},
    transport::profile_store,
};
use std::{path::PathBuf, sync::Arc, thread};

#[derive(Parser)]
#[command(about = "機体操作GUI / ローカル操作API")]
struct Args {
    #[arg(short, long, default_value = "/dev/ttyACM0")]
    serial_device: String,
    #[arg(short, long, default_value_t = 115200)]
    baud_rate: u32,
    #[arg(short, long, default_value_t = 20.0)]
    rate_hz: f64,
    #[arg(long)]
    machine_profile: Option<PathBuf>,
    #[arg(long)]
    simulate: bool,
    /// GUIを開かず、同じワーカーとAPIを起動する。
    #[arg(long)]
    headless: bool,
    #[arg(long)]
    socket: Option<PathBuf>,
}
fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    anyhow::ensure!(
        args.rate_hz.is_finite() && (20.0..=100.0).contains(&args.rate_hz),
        "rate-hzは20..100で指定してください"
    );
    let path = args
        .machine_profile
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/rtheta.toml"));
    let machine = profile_store::load(&path)?;
    let server =
        api_server::Server::bind(&args.socket.unwrap_or_else(control_api::default_socket))?;
    let shared = Arc::new(Shared::new(BridgeConfig {
        serial_device: args.serial_device,
        baud_rate: args.baud_rate,
        rate_hz: args.rate_hz,
        machine,
        profile_path: path,
        simulate: args.simulate,
    }));
    let worker_shared = shared.clone();
    let worker = thread::spawn(move || worker::run(worker_shared));
    let api_shared = shared.clone();
    let api = thread::spawn(move || server.run(api_shared));
    let result = if args.headless {
        while shared.is_running() {
            thread::sleep(std::time::Duration::from_millis(100));
        }
        Ok(())
    } else {
        let app_shared = shared.clone();
        eframe::run_native(
            "host",
            eframe::NativeOptions {
                viewport: egui::ViewportBuilder::default()
                    .with_inner_size([1100.0, 760.0])
                    .with_title("キャチロボクワガタ — 操縦"),
                ..Default::default()
            },
            Box::new(move |cc| {
                gui::install_japanese_font(&cc.egui_ctx);
                Ok(Box::new(gui::BridgeApp::new(app_shared)))
            }),
        )
        .map_err(|error| anyhow::anyhow!(error.to_string()))
    };
    shared.request_stop();
    let _ = worker.join();
    let _ = api.join();
    result
}

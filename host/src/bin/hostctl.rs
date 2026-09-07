//! 起動済みhostへ接続する。同じユーザのローカルソケットのみを使用する。
use anyhow::{Result, bail};
use clap::Parser;
use control_api::{Request, call};
use host::interface::control_api;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
#[derive(Parser)]
#[command(
    about = "起動済みhostの観測・設定・操作。action: status/config/claim/heartbeat/release/run/stop/estop/cut/safe/recover/origin/adjustment/input/apply/save/connection/reinit/fault"
)]
struct Args {
    action: String,
    #[arg(long)]
    socket: Option<PathBuf>,
    #[arg(long)]
    token: Option<String>,
    #[arg(long)]
    axis: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    value: Option<f32>,
    #[arg(long, action = clap::ArgAction::Set)]
    flag: Option<bool>,
    #[arg(long)]
    file: Option<PathBuf>,
    #[arg(long)]
    text: Option<String>,
    /// inputを50ms周期で送り、最後にゼロへ戻す。最大30秒。
    #[arg(long)]
    seconds: Option<f32>,
}
fn main() -> Result<()> {
    let args = Args::parse();
    let socket = args.socket.unwrap_or_else(control_api::default_socket);
    let text = match args.file {
        Some(path) => Some(std::fs::read_to_string(path)?),
        None => args.text,
    };
    let mut req = Request {
        action: args.action,
        token: args.token,
        axis: args.axis,
        value: args.value,
        flag: args.flag,
        text,
    };
    if let Some(seconds) = args.seconds {
        if req.action != "input" || !seconds.is_finite() || !(0.0..=30.0).contains(&seconds) {
            bail!("secondsはinputに0..30秒で指定してください");
        }
        let end = Instant::now() + Duration::from_secs_f32(seconds);
        let result: Result<()> = (|| {
            while Instant::now() < end {
                let reply = call(&socket, &req)?;
                if !reply.ok {
                    bail!("{}", reply.message);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(())
        })();
        req.value = Some(0.0);
        let _ = call(&socket, &req);
        result?;
    } else {
        let reply = call(&socket, &req)?;
        if reply.ok && matches!(req.action.as_str(), "status" | "config") {
            print!("{}", reply.data);
        } else {
            println!("{}", toml::to_string_pretty(&reply)?);
        }
        if !reply.ok {
            std::process::exit(1);
        }
    }
    Ok(())
}

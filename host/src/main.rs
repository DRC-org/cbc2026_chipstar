//! DualSense コントローラの入力を読み取り、cctl (STM32) へ USB-CDC シリアルで
//! 送信するスタンドアロンブリッジ。
//!
//! 従来の ROS2 3 ノード（joy_node → dualsense_teleop → cctl_usb_cdc_bridge）を
//! 単一バイナリに統合したもので、cctl 側のシリアル行フォーマットは維持している。

mod controller;
mod frame;
mod serial;

use std::thread;
use std::time::Duration;

use anyhow::{Result, anyhow};
use clap::Parser;
use gilrs::Gilrs;

use crate::serial::SerialLink;

#[derive(Parser)]
#[command(about = "DualSense controller to cctl USB-CDC bridge")]
struct Args {
    /// 送信先シリアルデバイス。
    #[arg(short, long, default_value = "/dev/ttyACM0")]
    serial_device: String,

    /// ボーレート。
    #[arg(short, long, default_value_t = 115200)]
    baud_rate: u32,

    /// 送信周期（Hz）。
    #[arg(short, long, default_value_t = 20.0)]
    rate_hz: f64,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let mut gilrs = Gilrs::new().map_err(|err| anyhow!("failed to initialize gilrs: {err}"))?;
    let mut link = SerialLink::new(args.serial_device.clone(), args.baud_rate);

    let period = Duration::from_secs_f64(1.0 / args.rate_hz);
    let mut serial_ok = true;

    loop {
        // イベントを消費して各ゲームパッドの状態を最新化する。
        while let Some(event) = gilrs.next_event() {
            gilrs.update(&event);
        }

        if let Some((_, gamepad)) = gilrs.gamepads().find(|(_, pad)| pad.is_connected()) {
            let state = controller::read(&gamepad);
            let line = frame::format_controller_input(&state);

            match link.write_line(&line) {
                Ok(()) => {
                    if !serial_ok {
                        eprintln!("serial link to {} restored", args.serial_device);
                        serial_ok = true;
                    }
                }
                Err(err) => {
                    if serial_ok {
                        eprintln!("{err:#}");
                        serial_ok = false;
                    }
                }
            }
        }

        thread::sleep(period);
    }
}

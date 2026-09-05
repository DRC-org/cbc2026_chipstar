//! ブリッジ処理スレッド。
//!
//! gilrs でコントローラを読み、設定された周期で cctl へシリアル送信する。
//! GUI とは [`Shared`] を介して設定・状態をやり取りする。

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use gilrs::Gilrs;

use crate::app_state::Shared;
use crate::device::parse_device_info;
use crate::machine::MachineController;
use crate::telemetry::parse_telemetry;
use crate::{controller, serial::SerialLink};

fn period_from_hz(rate_hz: f64) -> Duration {
    let hz = if rate_hz > 0.0 { rate_hz } else { 1.0 };
    Duration::from_secs_f64(1.0 / hz)
}

pub fn run(shared: Arc<Shared>) {
    let mut gilrs = match Gilrs::new() {
        Ok(gilrs) => gilrs,
        Err(err) => {
            shared.update_status(|s| s.last_error = Some(format!("gilrs 初期化失敗: {err}")));
            // 入力が扱えないため待機のみ。GUI は継続動作させる。
            while shared.is_running() {
                thread::sleep(Duration::from_millis(200));
            }
            return;
        }
    };

    let mut cfg = shared.config();
    let mut last_gen = shared.config_generation();
    let mut link = SerialLink::new(cfg.serial_device.clone(), cfg.baud_rate);
    let mut period = period_from_hz(cfg.rate_hz);
    let mut machine = MachineController::new(cfg.machine.clone());
    shared.queue_line(machine.hello_line());
    let mut last_hello = Instant::now();

    while shared.is_running() {
        // 設定変更を検知したらシリアルを開き直す。
        let generation = shared.config_generation();
        if generation != last_gen {
            last_gen = generation;
            cfg = shared.config();
            link = SerialLink::new(cfg.serial_device.clone(), cfg.baud_rate);
            period = period_from_hz(cfg.rate_hz);
            machine = MachineController::new(cfg.machine.clone());
            shared.queue_line(machine.hello_line());
            shared.update_status(|s| s.device = None);
            last_hello = Instant::now();
        }

        // USBの抜き差しは明示イベントを持たないため、能力照会を定期再送する。
        if last_hello.elapsed() >= Duration::from_secs(1) {
            let _ = link.write_line(&machine.hello_line());
            last_hello = Instant::now();
        }

        // イベントを消費して各ゲームパッドの状態を最新化する。
        while let Some(event) = gilrs.next_event() {
            gilrs.update(&event);
        }

        // 指令はコントローラの有無にも送信停止にも関係なく送る。
        // 立ち上げ中はコントローラを繋がないこともあり、STOP は常に届く必要がある。
        for line in shared.take_commands() {
            if let Err(err) = link.write_line(&line) {
                shared.update_status(|s| {
                    s.serial_connected = false;
                    s.last_error = Some(format!("{err:#}"));
                });
            } else {
                shared.update_status(|s| {
                    s.serial_connected = true;
                    s.last_line = line;
                });
            }
        }

        let gamepad = gilrs.gamepads().find(|(_, pad)| pad.is_connected());

        match gamepad {
            Some((_, pad)) => {
                let state = controller::read(&pad);
                let name = pad.name().to_owned();

                let send_result = if shared.sending_enabled() {
                    let mut result = Ok(());
                    let targets = machine.update(&state, period.as_secs_f32());
                    for target in &targets {
                        if let Err(err) = link.write_line(target) {
                            result = Err(err);
                            break;
                        }
                    }
                    Some((result, targets.last().cloned().unwrap_or_default()))
                } else {
                    None
                };

                shared.update_status(|s| {
                    s.gamepad_connected = true;
                    s.gamepad_name = Some(name);
                    s.axes = state.axes;
                    s.buttons = state.buttons;
                    match send_result {
                        Some((Ok(()), last_line)) => {
                            s.serial_connected = true;
                            s.last_error = None;
                            s.last_line = last_line;
                            s.tx_count = s.tx_count.wrapping_add(1);
                        }
                        Some((Err(err), _)) => {
                            s.serial_connected = false;
                            s.last_error = Some(format!("{err:#}"));
                        }
                        None => {}
                    }
                });
            }
            None => {
                shared.update_status(|s| {
                    s.gamepad_connected = false;
                    s.gamepad_name = None;
                });
            }
        }

        // cctl からのテレメトリを取り込む。
        for line in link.read_lines() {
            if let Some(device) = parse_device_info(&line) {
                shared.update_status(|s| {
                    s.device = Some(device);
                    s.serial_connected = true;
                    s.last_error = None;
                });
            } else if let Some(telemetry) = parse_telemetry(&line) {
                shared.update_status(|s| {
                    s.telemetry = Some(telemetry);
                    s.telemetry_count = s.telemetry_count.wrapping_add(1);
                });
            }
        }

        thread::sleep(period);
    }
}

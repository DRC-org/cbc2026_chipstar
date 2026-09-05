//! 汎用FWの対話型動作テスト。通常のGUI/ゲームパッド処理とは独立して実行する。
#[path = "../device.rs"]
mod device;
#[path = "../fw_test.rs"]
mod fw_test;
#[path = "../serial.rs"]
mod serial;

use anyhow::{Result, bail};
#[path = "../fw_test_transport.rs"]
mod fw_test_transport;
use clap::Parser;
use fw_test::{Board, Session};
use fw_test_transport::{connect, send};
use std::{
    io::{self, BufRead},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(about = "汎用FWの機能別テスト。起動時は全テストOFF")]
struct Args {
    #[arg(long, value_enum)]
    board: Board,
    /// cctl/svmd/dcmd: cctlのUSB、serial-svmd: USART2のUSBシリアル
    #[arg(long)]
    serial_device: String,
    /// 省略時: serial-svmd=38400、それ以外=115200
    #[arg(long)]
    baud_rate: Option<u32>,
    /// 各出力の自動OFF時間（1〜30秒）
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..=30))]
    seconds: u64,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let baud = args
        .baud_rate
        .unwrap_or(if args.board == Board::SerialSvmd {
            38400
        } else {
            115200
        });
    let mut link = serial::SerialLink::new(args.serial_device, baud);
    connect(&mut link, args.board)?;
    let mut session = Session::new(args.board, Duration::from_secs(args.seconds));
    fw_test_transport::prepare(&mut link, args.board, &mut session)?;
    println!("{}", session.help());
    println!(
        "出力は最大{}秒。機構を安全に固定し、通常hostを終了して使用してください。",
        args.seconds
    );
    let (tx, rx) = mpsc::sync_channel(16);
    thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            match line {
                Ok(line) => {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
                _ => break,
            }
        }
    });
    let result = (|| -> Result<()> {
        loop {
            let now = Instant::now();
            let previous_outputs = session.active_outputs();
            match rx.try_recv() {
                Ok(line) if line.trim() == "quit" => break,
                Ok(line) if line.trim() == "help" => println!("{}", session.help()),
                Ok(line) => match session.command(&line, now) {
                    Ok(lines) => {
                        send(&mut link, lines)?;
                        println!("設定: {}", line.trim());
                        println!(
                            "ON機能: {:?}, 通信断テスト: {}",
                            session.switches(),
                            session.watchdog_running()
                        );
                    }
                    Err(error) => eprintln!("{error:#}"),
                },
                Err(mpsc::TryRecvError::Disconnected) => break,
                Err(mpsc::TryRecvError::Empty) => {}
            }
            send(&mut link, session.tick(now))?;
            if previous_outputs != session.active_outputs() {
                println!("テスト出力ON（指令状態）: {:?}", session.active_outputs());
            }
            for line in link.read_lines() {
                if line.starts_with("ERR ") {
                    bail!("FWが指令を拒否しました: {line}");
                }
                if line.starts_with("CAN_RX bus=2 id=769 data=0101")
                    || line.starts_with("CAN_RX bus=2 id=785 data=0101")
                {
                    bail!("CAN先のFWが指令を拒否しました: {line}");
                }
                if session.visible(&line) {
                    println!("{}", fw_test::describe(&line));
                }
            }
            thread::sleep(Duration::from_millis(50));
        }
        Ok(())
    })();
    // 再開は行わず終了する。送信不能時の最終停止はFWのWatchdogが担う。
    let stopped = send(&mut link, session.stop());
    result.and(stopped)
}

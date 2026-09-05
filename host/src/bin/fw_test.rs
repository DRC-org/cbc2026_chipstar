//! 汎用FWの対話型動作テスト。通常のGUI/ゲームパッド処理とは独立して実行する。
#[path = "../device.rs"]
mod device;
#[path = "../fw_test.rs"]
mod fw_test;
#[path = "../serial.rs"]
mod serial;

use anyhow::{Result, bail};
use clap::Parser;
use fw_test::{Board, Session};
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

fn send(link: &mut serial::SerialLink, lines: Vec<String>) -> Result<()> {
    for line in lines {
        link.write_line(&line)?;
        // USB受信キューと低速UARTに一括投入しない。
        thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

fn connect(link: &mut serial::SerialLink, board: Board) -> Result<()> {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        link.write_line("HELLO 1")?;
        thread::sleep(Duration::from_millis(100));
        for line in link.read_lines() {
            if let Some(info) = device::parse_device_info(&line) {
                let expected = if board == Board::SerialSvmd {
                    "serial_svmd"
                } else {
                    "cctl"
                };
                if info.protocol != 1 || info.board != expected {
                    bail!("接続先の能力が不一致: {line}");
                }
                if matches!(board, Board::Svmd | Board::Dcmd) && !info.can_buses.contains(&2) {
                    bail!("FDCAN2ゲートウェイがありません");
                }
                return Ok(());
            }
        }
    }
    bail!("能力通知がありません。接続とボーレートを確認してください")
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
    if matches!(args.board, Board::Svmd | Board::Dcmd) {
        send(&mut link, vec!["STOP".into(), "ENABLE 7 0".into()])?;
    }
    send(&mut link, session.stop())?;
    if matches!(args.board, Board::Svmd | Board::Dcmd) {
        let (probe, prefix) = if args.board == Board::Svmd {
            (
                "CAN 2 768 0103000000000000",
                "CAN_RX bus=2 id=769 data=0100",
            )
        } else {
            (
                "CAN 2 784 0100000000000000",
                "CAN_RX bus=2 id=785 data=0100",
            )
        };
        let start = Instant::now();
        let mut found = false;
        while start.elapsed() < Duration::from_secs(3) {
            link.write_line(probe)?;
            thread::sleep(Duration::from_millis(100));
            if link
                .read_lines()
                .iter()
                .any(|line| line.starts_with(prefix))
            {
                found = true;
                break;
            }
        }
        if !found {
            bail!("CAN先の基板応答がありません。電源・CAN配線・終端抵抗を確認してください");
        }
    }
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

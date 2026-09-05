use crate::fw_test::{Board, Session};
use crate::{device, serial};
use anyhow::{Result, bail};
use std::{
    thread,
    time::{Duration, Instant},
};

pub fn send(link: &mut serial::SerialLink, lines: Vec<String>) -> Result<()> {
    for line in lines {
        link.write_line(&line)?;
        // USB受信キューと低速UARTに一括投入しない。
        thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

pub fn connect(link: &mut serial::SerialLink, board: Board) -> Result<()> {
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

pub fn prepare(link: &mut serial::SerialLink, board: Board, session: &mut Session) -> Result<()> {
    if matches!(board, Board::Svmd | Board::Dcmd) {
        send(link, vec!["STOP".into(), "ENABLE 7 0".into()])?;
    }
    send(link, session.stop())?;
    if matches!(board, Board::Svmd | Board::Dcmd) {
        let (probe, prefix) = if board == Board::Svmd {
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
    Ok(())
}

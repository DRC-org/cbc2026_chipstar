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
                if matches!(board, Board::Svmd | Board::Dcmd | Board::Network)
                    && !info.can_buses.contains(&2)
                {
                    bail!("FDCAN2ゲートウェイがありません");
                }
                return Ok(());
            }
        }
    }
    bail!("能力通知がありません。接続とボーレートを確認してください")
}

/// CAN先の基板を1つ叩いて応答を待つ。
fn probe(link: &mut serial::SerialLink, board: Board) -> Result<bool> {
    let (request, reply) = match board {
        Board::Svmd => (
            "CAN 2 768 0103000000000000",
            "CAN_RX bus=2 id=769 data=0100",
        ),
        _ => (
            "CAN 2 784 0100000000000000",
            "CAN_RX bus=2 id=785 data=0100",
        ),
    };
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        link.write_line(request)?;
        thread::sleep(Duration::from_millis(100));
        if link.read_lines().iter().any(|line| line.starts_with(reply)) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// 出力を止めてから、CAN先の基板を確かめる。応答した基板を返す。
///
/// 単独対象では応答なしを失敗とするが、ネットワーク確認では
/// 「どこまで見えているか」を知ることが目的なので、欠けたまま続ける。
pub fn prepare(
    link: &mut serial::SerialLink,
    board: Board,
    session: &mut Session,
) -> Result<Vec<Board>> {
    if board.via_cctl() && board != Board::Cctl {
        send(link, vec!["STOP".into(), "ENABLE 7 0".into()])?;
    }
    send(link, session.stop())?;

    let mut reachable = Vec::new();
    for target in board.members() {
        if !matches!(target, Board::Svmd | Board::Dcmd) {
            continue;
        }
        if probe(link, target)? {
            reachable.push(target);
        } else if board != Board::Network {
            bail!("CAN先の基板応答がありません。電源・CAN配線・終端抵抗を確認してください");
        }
    }
    Ok(reachable)
}

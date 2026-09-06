use anyhow::{Result, anyhow, bail, ensure};
use clap::ValueEnum;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Board {
    Cctl,
    Svmd,
    SerialSvmd,
    Dcmd,
    /// cctlのUSB1本で、cctl・svmd・DCMDを繋ぎ替えずにまとめて扱う。
    Network,
}

impl Board {
    pub fn key(self) -> &'static str {
        match self {
            Board::Cctl => "cctl",
            Board::Svmd => "svmd",
            Board::SerialSvmd => "serial_svmd",
            Board::Dcmd => "dcmd",
            Board::Network => "network",
        }
    }

    /// この対象が実際に触る基板。
    pub fn members(self) -> Vec<Board> {
        match self {
            Board::Network => vec![Board::Cctl, Board::Svmd, Board::Dcmd],
            other => vec![other],
        }
    }

    /// cctlのUSB CDCへ繋ぐ対象か。
    pub fn via_cctl(self) -> bool {
        !matches!(self, Board::SerialSvmd)
    }
}

/// 対象ごとの状態。`Network` では扱う基板ぶんだけ持つ。
struct Part {
    board: Board,
    outputs: BTreeMap<u8, Instant>,
    reads: BTreeSet<u8>,
    status: bool,
    encoder: bool,
    inputs: bool,
    last_read: u8,
}

impl Part {
    fn new(board: Board) -> Self {
        Self {
            board,
            outputs: BTreeMap::new(),
            reads: BTreeSet::new(),
            status: false,
            encoder: false,
            inputs: false,
            last_read: 0,
        }
    }

    fn check_id(&self, id: u8) -> Result<()> {
        ensure!(
            match self.board {
                Board::Cctl => id < 3,
                Board::Svmd => id < 4,
                Board::SerialSvmd => (1..=253).contains(&id),
                Board::Dcmd => id == 0,
                Board::Network => false,
            },
            "未対応のチャネル/IDです"
        );
        Ok(())
    }

    fn off(&mut self, id: u8) -> Vec<String> {
        self.outputs.remove(&id);
        match self.board {
            Board::Cctl => vec![format!("ENABLE {} 0", 1u8 << id)],
            Board::Svmd => vec![can(768, 2, id, 0, 0)],
            Board::SerialSvmd => vec![format!("SERVO ENABLE {id} 0")],
            Board::Dcmd | Board::Network => vec![can(784, 3, 0, 0, 0)],
        }
    }

    fn stop(&mut self) -> Vec<String> {
        self.outputs.clear();
        self.reads.clear();
        match self.board {
            Board::Cctl => vec!["STOP".into(), "ENABLE 7 0".into()],
            Board::SerialSvmd => vec!["STOP".into()],
            Board::Svmd => vec![can(768, 0, 0, 0, 0)],
            Board::Dcmd | Board::Network => vec![can(784, 3, 0, 0, 0)],
        }
    }

    fn drive(&mut self, id: u8, value: f32) -> Result<Vec<String>> {
        Ok(match self.board {
            Board::Cctl => {
                let limit = if id == 1 { 26000.0 } else { 12.5 };
                ensure!(value.abs() <= limit, "FWの位置上限を超えています");
                vec![
                    "HELLO 1".into(),
                    format!("TARGET {id} {value}"),
                    format!("ENABLE {} 1", 1u8 << id),
                    "RUN".into(),
                ]
            }
            Board::Svmd => {
                ensure!(
                    value.fract() == 0.0 && (500.0..=2500.0).contains(&value),
                    "500..2500の整数を指定してください"
                );
                vec![can(768, 1, id, 0, value as i16), can(768, 2, id, 1, 0)]
            }
            Board::SerialSvmd => {
                ensure!(
                    value.fract() == 0.0 && (0.0..=4095.0).contains(&value),
                    "0..4095の整数を指定してください"
                );
                ensure!(
                    self.outputs.contains_key(&id) || self.outputs.len() < 16,
                    "同時に最大16出力です"
                );
                vec![
                    "HELLO 1".into(),
                    format!("SERVO TARGET {id} {} 100 10", value as u16),
                    format!("SERVO ENABLE {id} 1"),
                    "RUN".into(),
                ]
            }
            Board::Dcmd | Board::Network => {
                ensure!(
                    value.fract() == 0.0 && value.abs() <= 100.0,
                    "テスト出力は-100..100の整数です（最大10%）"
                );
                vec![
                    can(784, 0, 0, 0, 0),
                    can(784, 4, 0, 0, value as i16),
                    can(784, 2, 1, 0, 0),
                ]
            }
        })
    }

    /// 出力を保つための定期送信。
    fn keepalive(&self, communication: bool) -> Option<String> {
        if self.outputs.is_empty() && !communication && !(self.board == Board::Svmd && self.status)
        {
            return None;
        }
        Some(match self.board {
            Board::Cctl | Board::SerialSvmd => if communication {
                "HELLO 1"
            } else {
                "HEARTBEAT"
            }
            .into(),
            Board::Svmd => can(768, 3, 0, 0, 0),
            Board::Dcmd | Board::Network => {
                can(784, if self.outputs.is_empty() { 0 } else { 5 }, 0, 0, 0)
            }
        })
    }

    fn visible(&self, line: &str) -> bool {
        if self.inputs && crate::protocol::inputs::parse(line, self.board).is_some() {
            return true;
        }
        match self.board {
            Board::Cctl => self.status && line.starts_with("STATE "),
            Board::Svmd => self.status && line.starts_with("CAN_RX bus=2 id=769 "),
            Board::Dcmd | Board::Network => {
                (self.status && line.starts_with("CAN_RX bus=2 id=785 "))
                    || (self.encoder && line.starts_with("CAN_RX bus=2 id=786 "))
            }
            Board::SerialSvmd => line
                .split_whitespace()
                .nth(1)
                .and_then(|v| v.strip_prefix("id="))
                .and_then(|id| id.parse::<u8>().ok())
                .is_some_and(|id| line.starts_with("SERVO_STATE ") && self.reads.contains(&id)),
        }
    }
}

pub struct Session {
    board: Board,
    duration: Duration,
    parts: Vec<Part>,
    communication: bool,
    silent_until: Option<Instant>,
}

fn can(id: u16, op: u8, channel: u8, flag: u8, value: i16) -> String {
    let [hi, lo] = value.to_be_bytes();
    format!("CAN 2 {id} 01{op:02X}{channel:02X}{flag:02X}{hi:02X}{lo:02X}0000")
}

impl Session {
    pub fn new(board: Board, duration: Duration) -> Self {
        Self {
            board,
            duration,
            parts: board.members().into_iter().map(Part::new).collect(),
            communication: false,
            silent_until: None,
        }
    }

    /// CAN先が応答しなかった基板を対象から外す。ネットワーク確認でのみ使う。
    pub fn retain_boards(&mut self, reachable: &[Board]) {
        self.parts
            .retain(|part| part.board == Board::Cctl || reachable.contains(&part.board));
    }

    /// 機能名に基板を付ける。単独対象では付けない。
    fn qualify(&self, part: &Part, name: &str) -> String {
        if self.board == Board::Network {
            format!("{}.{name}", part.board.key())
        } else {
            name.to_owned()
        }
    }

    fn resolve<'a>(&self, name: &'a str) -> Result<(usize, &'a str)> {
        if self.board != Board::Network {
            return Ok((0, name));
        }
        let (key, rest) = name
            .split_once('.')
            .ok_or_else(|| anyhow!("cctl.motor0 のように基板を指定してください"))?;
        let index = self
            .parts
            .iter()
            .position(|part| part.board.key() == key)
            .ok_or_else(|| anyhow!("この対象に {key} はありません"))?;
        Ok((index, rest))
    }

    pub fn switches(&self) -> Vec<String> {
        let mut switches = Vec::new();
        for part in &self.parts {
            switches.extend(
                part.outputs
                    .keys()
                    .map(|id| self.qualify(part, &format!("motor{id}"))),
            );
            switches.extend(
                part.reads
                    .iter()
                    .map(|id| self.qualify(part, &format!("read{id}"))),
            );
            for (name, enabled) in [
                ("status", part.status),
                ("encoder", part.encoder),
                ("inputs", part.inputs),
            ] {
                if enabled {
                    switches.push(self.qualify(part, name));
                }
            }
        }
        if self.communication {
            switches.push("communication".into());
        }
        switches
    }

    pub fn watchdog_running(&self) -> bool {
        self.silent_until.is_some()
    }

    /// 出力中の機能名と自動OFFの期限。
    pub fn output_expiries(&self) -> Vec<(String, Instant)> {
        self.parts
            .iter()
            .flat_map(|part| {
                part.outputs
                    .iter()
                    .map(move |(id, at)| (self.qualify(part, &format!("motor{id}")), *at))
            })
            .collect()
    }

    pub fn help(&self) -> String {
        let features = match self.board {
            Board::Cctl => "motor0..2 <位置>（0/2: rad、1: motor deg）; status".to_owned(),
            Board::Svmd => "motor0..3 <パルス幅500..2500 us>; status".to_owned(),
            Board::SerialSvmd => {
                "motor1..253 <位置0..4095>; read1..253（同時最大16 ID）".to_owned()
            }
            Board::Dcmd => "motor0 <duty -100..100 permille>; encoder; status".to_owned(),
            Board::Network => format!(
                "基板名を前置きする。応答のあった基板: {}\n  cctl.motor0..2 <位置>; cctl.status\n  svmd.motor0..3 <500..2500 us>; svmd.status\n  dcmd.motor0 <-100..100 permille>; dcmd.encoder; dcmd.status; dcmd.inputs",
                self.parts
                    .iter()
                    .map(|part| part.board.key())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        let inputs = if self.board == Board::Network {
            ""
        } else if crate::protocol::inputs::supported(self.board) {
            "; inputs (SW/DIP読取り)"
        } else if self.board == Board::Cctl {
            "。接点はstatusのSTATE行にsw=として出る"
        } else {
            ""
        };
        format!(
            "on <機能> [値] / off <機能> / stop / watchdog / help / quit\n機能: {features}; communication{inputs}\nwatchdog: 全送信を500ms止め、出力をOFF状態に戻す。自動再開なし。"
        )
    }

    pub fn stop(&mut self) -> Vec<String> {
        self.communication = false;
        self.silent_until = None;
        self.parts.iter_mut().flat_map(Part::stop).collect()
    }

    pub fn command(&mut self, line: &str, now: Instant) -> Result<Vec<String>> {
        let tokens: Vec<_> = line.split_whitespace().collect();
        if tokens == ["stop"] {
            return Ok(self.stop());
        }
        ensure!(
            self.silent_until.is_none(),
            "Watchdogテスト中です。stopで中断できます"
        );
        if tokens == ["watchdog"] {
            ensure!(
                self.parts.iter().any(|part| !part.outputs.is_empty()),
                "出力テストをONにしてから実行してください"
            );
            self.silent_until = Some(now + Duration::from_millis(500));
            return Ok(vec![]);
        }
        ensure!(
            tokens.len() == 2 || tokens.len() == 3,
            "on/off <機能> [値] を指定してください"
        );
        let enabled = match tokens[0] {
            "on" => true,
            "off" => false,
            _ => bail!("on/offを指定してください"),
        };
        if tokens[1] == "communication" {
            ensure!(tokens.len() == 2, "この機能に値は不要です");
            self.communication = enabled;
            return Ok(vec![]);
        }
        let (index, name) = self.resolve(tokens[1])?;
        let duration = self.duration;

        if let Some(id) = name.strip_prefix("motor") {
            let id: u8 = id.parse()?;
            let part = &mut self.parts[index];
            part.check_id(id)?;
            if !enabled {
                ensure!(tokens.len() == 2, "offに値は不要です");
                return Ok(part.off(id));
            }
            ensure!(tokens.len() == 3, "出力目標を明示してください");
            let value: f32 = tokens[2].parse()?;
            ensure!(value.is_finite(), "有限値を指定してください");
            let commands = part.drive(id, value)?;
            part.outputs.insert(id, now + duration);
            return Ok(commands);
        }

        ensure!(tokens.len() == 2, "この機能に値は不要です");
        let board = self.parts[index].board;
        let part = &mut self.parts[index];
        match name {
            "inputs" => {
                ensure!(
                    crate::protocol::inputs::supported(board),
                    "この基板に要求応答型の接点報告はありません"
                );
                part.inputs = enabled;
            }
            "status" => {
                ensure!(board != Board::SerialSvmd, "read<ID>を使用してください");
                part.status = enabled;
            }
            "encoder" => {
                ensure!(board == Board::Dcmd, "encoderはDCMD専用です");
                part.encoder = enabled;
            }
            _ => {
                ensure!(board == Board::SerialSvmd, "未対応の機能です");
                let id: u8 = name
                    .strip_prefix("read")
                    .ok_or_else(|| anyhow!("未対応の機能です"))?
                    .parse()?;
                part.check_id(id)?;
                if enabled {
                    ensure!(
                        part.reads.contains(&id) || part.reads.len() < 16,
                        "同時に最大16読取りです"
                    );
                    part.reads.insert(id);
                } else {
                    part.reads.remove(&id);
                }
            }
        }
        Ok(vec![])
    }

    pub fn tick(&mut self, now: Instant) -> Vec<String> {
        if let Some(until) = self.silent_until {
            return if now >= until { self.stop() } else { vec![] };
        }
        let communication = self.communication;
        let mut lines = Vec::new();
        for part in &mut self.parts {
            let expired: Vec<_> = part
                .outputs
                .iter()
                .filter(|(_, until)| now >= **until)
                .map(|(&id, _)| id)
                .collect();
            for id in expired {
                lines.extend(part.off(id));
            }
            lines.extend(part.keepalive(communication));
            // 1周期1IDに制限し、出力のWatchdog更新を遅らせない。
            if part.inputs {
                lines.push(match part.board {
                    Board::Dcmd => can(784, 6, 0, 0, 0),
                    _ => "INPUT READ".into(),
                });
            }
            if let Some(id) = part
                .reads
                .iter()
                .copied()
                .find(|&id| id > part.last_read)
                .or_else(|| part.reads.first().copied())
            {
                lines.push(format!("SERVO READ {id}"));
                part.last_read = id;
            }
        }
        lines
    }

    pub fn visible(&self, line: &str) -> bool {
        self.communication || self.parts.iter().any(|part| part.visible(line))
    }
}

/// CANの生フレームに配線確認用の値を併記する。
pub fn describe(line: &str) -> String {
    for board in [Board::Cctl, Board::SerialSvmd, Board::Dcmd, Board::Svmd] {
        if let Some(state) = crate::protocol::inputs::parse(line, board) {
            return crate::protocol::inputs::describe(&state);
        }
    }
    let Some((prefix, payload)) = line.split_once(" data=") else {
        return line.into();
    };
    if payload.len() != 16 || !payload.is_ascii() {
        return line.into();
    }
    let data: Option<Vec<u8>> = (0..8)
        .map(|i| u8::from_str_radix(&payload[i * 2..i * 2 + 2], 16).ok())
        .collect();
    let Some(data) = data.filter(|data| data[0] == 1) else {
        return line.into();
    };
    let detail = match prefix {
        "CAN_RX bus=2 id=786" if data[1] == 1 => format!(
            "encoder={} index={}",
            i32::from_be_bytes(data[2..6].try_into().unwrap()),
            u16::from_be_bytes([data[6], data[7]])
        ),
        "CAN_RX bus=2 id=785" => format!(
            "result={} mode={} enabled={} duty={}",
            data[1],
            data[2],
            data[3],
            i16::from_be_bytes([data[4], data[5]])
        ),
        "CAN_RX bus=2 id=769" => format!(
            "result={} channel={} enabled_mask={} pulse_us={}",
            data[1],
            data[3],
            data[4],
            u16::from_be_bytes([data[5], data[6]])
        ),
        _ => return line.into(),
    };
    format!("{line}  [{detail}]")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn session(board: Board) -> Session {
        Session::new(board, Duration::from_secs(5))
    }
    #[test]
    fn defaults_are_off_for_every_board() {
        for board in [Board::Cctl, Board::Svmd, Board::SerialSvmd, Board::Dcmd] {
            assert!(session(board).tick(Instant::now()).is_empty());
        }
    }
    #[test]
    fn svmd_status_polls_without_enabling_outputs() {
        let mut s = session(Board::Svmd);
        let now = Instant::now();
        s.command("on status", now).unwrap();
        assert_eq!(s.tick(now), ["CAN 2 768 0103000000000000"]);
        assert!(s.output_expiries().is_empty());
        s.command("off status", now).unwrap();
        assert!(s.tick(now).is_empty());
    }
    #[test]
    fn network_drives_every_board_over_one_link() {
        let now = Instant::now();
        let mut s = Session::new(Board::Network, Duration::from_secs(5));

        // 基板を前置きしないと、どこへの指令か決まらない。
        assert!(s.command("on motor0 0.1", now).is_err());

        let cctl = s.command("on cctl.motor0 0.1", now).unwrap();
        assert!(cctl.iter().any(|line| line == "TARGET 0 0.1"));
        let svmd = s.command("on svmd.motor2 1500", now).unwrap();
        assert!(svmd.iter().any(|line| line.starts_with("CAN 2 768 ")));
        let dcmd = s.command("on dcmd.motor0 50", now).unwrap();
        assert!(dcmd.iter().any(|line| line.starts_with("CAN 2 784 ")));

        let switches = s.switches();
        for name in ["cctl.motor0", "svmd.motor2", "dcmd.motor0"] {
            assert!(switches.contains(&name.to_owned()), "{name} がない");
        }

        // 定期送信は3基板ぶんまとめて出る。
        let tick = s.tick(now);
        assert!(tick.iter().any(|line| line == "HEARTBEAT"));
        assert!(tick.iter().any(|line| line.starts_with("CAN 2 768 ")));
        assert!(tick.iter().any(|line| line.starts_with("CAN 2 784 ")));

        // stopは全基板へ届く。
        let stop = s.stop();
        assert!(stop.iter().any(|line| line == "STOP"));
        assert!(stop.iter().any(|line| line.starts_with("CAN 2 768 ")));
        assert!(stop.iter().any(|line| line.starts_with("CAN 2 784 ")));
        assert!(s.output_expiries().is_empty());
    }

    #[test]
    fn network_drops_boards_that_did_not_answer() {
        let now = Instant::now();
        let mut s = Session::new(Board::Network, Duration::from_secs(5));
        s.retain_boards(&[Board::Svmd]);

        assert!(s.command("on svmd.motor0 1500", now).is_ok());
        // 応答がなかったDCMDへは指令を作らない。
        assert!(s.command("on dcmd.motor0 50", now).is_err());
        // cctlは接続先そのものなので常に残る。
        assert!(s.command("on cctl.motor0 0.1", now).is_ok());
    }

    #[test]
    fn single_board_keeps_unprefixed_names() {
        let now = Instant::now();
        let mut s = Session::new(Board::Cctl, Duration::from_secs(5));
        assert!(s.command("on motor0 0.1", now).is_ok());
        assert!(s.switches().contains(&"motor0".to_owned()));
        assert!(s.command("on cctl.motor0 0.1", now).is_err());
    }

    #[test]
    fn independent_outputs_and_expiry() {
        let now = Instant::now();
        for board in [Board::Cctl, Board::Svmd, Board::SerialSvmd] {
            let mut s = session(board);
            s.command("on motor1 1500", now)
                .unwrap_or_else(|_| s.command("on motor1 0", now).unwrap());
            s.command("on motor2 1500", now + Duration::from_secs(1))
                .unwrap_or_else(|_| {
                    s.command("on motor2 0", now + Duration::from_secs(1))
                        .unwrap()
                });
            s.tick(now + Duration::from_secs(5));
            assert!(!s.switches().contains(&"motor1".to_owned()));
            assert!(s.switches().contains(&"motor2".to_owned()));
            s.command("off motor2", now).unwrap();
            assert!(s.output_expiries().is_empty());
        }
    }
    #[test]
    fn dcmd_signed_target_and_stop() {
        let mut s = session(Board::Dcmd);
        let commands = s.command("on motor0 -100", Instant::now()).unwrap();
        assert_eq!(commands[1], "CAN 2 784 01040000FF9C0000");
        assert_eq!(commands[2], "CAN 2 784 0102010000000000");
        assert_eq!(
            s.command("off motor0", Instant::now()).unwrap(),
            ["CAN 2 784 0103000000000000"]
        );
    }
    #[test]
    fn invalid_commands_do_not_arm() {
        for (board, commands) in [
            (
                Board::Cctl,
                vec!["on motor3 0", "on motor0 NaN", "on motor2 13"],
            ),
            (
                Board::Svmd,
                vec!["on motor4 1500", "on motor0 499", "on motor0 1500.5"],
            ),
            (Board::SerialSvmd, vec!["on motor0 2000", "on motor1 4096"]),
            (
                Board::Dcmd,
                vec!["on motor1 10", "on motor0 101", "on motor0"],
            ),
        ] {
            let mut s = session(board);
            for command in commands {
                assert!(s.command(command, Instant::now()).is_err());
            }
            assert!(s.output_expiries().is_empty());
        }
    }
    #[test]
    fn watchdog_silences_reads_and_heartbeat_without_restarting() {
        let mut s = session(Board::SerialSvmd);
        let now = Instant::now();
        s.command("on motor1 2000", now).unwrap();
        s.command("on read1", now).unwrap();
        s.command("on communication", now).unwrap();
        s.command("watchdog", now).unwrap();
        assert!(s.tick(now + Duration::from_millis(400)).is_empty());
        assert!(s.command("on motor2 2000", now).is_err());
        assert_eq!(s.tick(now + Duration::from_millis(500)), ["STOP"]);
        assert!(s.tick(now + Duration::from_secs(1)).is_empty());
    }
    #[test]
    fn encoder_observation_never_enables_motor() {
        let mut s = session(Board::Dcmd);
        s.command("on encoder", Instant::now()).unwrap();
        assert!(s.tick(Instant::now()).is_empty());
        assert!(s.visible("CAN_RX bus=2 id=786 data=0101000000010000"));
        s.command("off encoder", Instant::now()).unwrap();
        assert!(!s.visible("CAN_RX bus=2 id=786 data=0101000000010000"));
    }
    #[test]
    fn reads_rotate_and_can_be_disabled_independently() {
        let mut s = session(Board::SerialSvmd);
        let now = Instant::now();
        s.command("on read1", now).unwrap();
        s.command("on read12", now).unwrap();
        assert_eq!(s.tick(now), ["SERVO READ 1"]);
        assert_eq!(s.tick(now), ["SERVO READ 12"]);
        s.command("off read1", now).unwrap();
        assert_eq!(s.tick(now), ["SERVO READ 12"]);
        assert!(s.output_expiries().is_empty());
    }
    #[test]
    fn decodes_physical_feedback() {
        assert!(
            describe("CAN_RX bus=2 id=786 data=0101FFFFFFFE0003").ends_with("[encoder=-2 index=3]")
        );
        assert!(
            describe("CAN_RX bus=2 id=785 data=01000101FF9C0000")
                .ends_with("[result=0 mode=1 enabled=1 duty=-100]")
        );
        assert!(
            describe("CAN_RX bus=2 id=769 data=010001020405DC00")
                .ends_with("[result=0 channel=2 enabled_mask=4 pulse_us=1500]")
        );
        assert_eq!(
            describe("CAN_RX bus=2 id=786 data=invalid"),
            "CAN_RX bus=2 id=786 data=invalid"
        );
    }
}

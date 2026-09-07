use super::*;
use crate::application::sts::{self, Operation, Sample, Step, Target};
use std::collections::VecDeque;

struct Command {
    packet: [u8; 8],
    expected: Option<u16>,
    wait_ms: u32,
}
#[derive(Default)]
pub(super) struct Control {
    pub interested: bool,
    pub active: bool,
    armed: bool,
    queue: VecDeque<Command>,
    pending: Option<(Command, Instant)>,
    wait_until: Option<Instant>,
    lease: Option<Instant>,
    sequence: bool,
    monitor: Vec<u8>,
    monitor_index: usize,
    last_poll: Option<Instant>,
    bytes: [u8; 15],
    parts: u8,
    tag: u8,
    epoch: Option<Instant>,
}
impl Control {
    pub fn stop_monitoring(&mut self) {
        self.monitor.clear();
    }
    fn tag(&mut self) -> u8 {
        self.tag = self.tag.wrapping_add(1);
        self.tag
    }
    pub fn cancel(&mut self) {
        self.active = false;
        self.armed = false;
        self.queue.clear();
        self.pending = None;
        self.wait_until = None;
        self.lease = None;
        self.sequence = false;
    }
    fn command(&mut self, op: u8, id: u8, args: [u8; 4], expected: Option<u16>) {
        let tag = self.tag();
        self.queue.push_back(Command {
            packet: [1, op, id, tag, args[0], args[1], args[2], args[3]],
            expected,
            wait_ms: 0,
        });
    }
    fn motion(&mut self, targets: &[Target], wait_ms: u32) {
        self.command(27, 0, [0; 4], None);
        for t in targets {
            self.command(20, t.id, [33, 1, 0, 0], Some(u16::from(t.mode)));
        }
        for t in targets {
            let position = if t.mode == 1 { 0 } else { sts::signed(t.value) };
            let speed = if t.mode == 1 {
                sts::signed(t.value)
            } else {
                t.speed
            };
            self.queue.push_back(Command {
                packet: [
                    1,
                    22,
                    t.id,
                    t.acceleration,
                    (position >> 8) as u8,
                    position as u8,
                    (speed >> 8) as u8,
                    speed as u8,
                ],
                expected: None,
                wait_ms: 0,
            });
        }
        self.queue.push_back(Command {
            packet: [1, 2, 0, 0, 0, 0, 0, 0],
            expected: None,
            wait_ms: 0,
        });
        self.command(23, targets.len() as u8, [0; 4], None);
        self.queue.back_mut().unwrap().wait_ms = wait_ms;
    }
    pub fn busy(&self) -> bool {
        self.pending.is_some() || !self.queue.is_empty() || self.wait_until.is_some()
    }
    pub fn elapsed_ms(&self) -> u64 {
        self.epoch
            .map_or(0, |epoch| epoch.elapsed().as_millis() as u64)
    }
}

impl Runtime {
    pub(super) fn sts_request(&mut self, req: &Request) -> Result<Reply> {
        let operation: Operation =
            toml::from_str(req.text.as_deref().context("STS操作のTOMLが必要です")?)?;
        if matches!(operation, Operation::Renew) {
            if self.sts.lease.is_some() {
                self.sts.lease = Some(Instant::now());
            }
            return Ok(Reply::accepted());
        }
        anyhow::ensure!(
            self.fresh() && self.device.is_some() && self.settings.ready(),
            "接続・設定照合の完了を待ってください"
        );
        anyhow::ensure!(
            !self.drive.running() && self.drive.awaiting().is_none() && !self.test.enabled,
            "通常運転と個別テストを終了してください"
        );
        anyhow::ensure!(!self.sts.busy(), "STS操作の完了を待つか停止してください");
        self.sts.interested = true;
        self.send("CAN 2 800 0100000000000000")?;
        match operation {
            Operation::Monitor { ids } => {
                anyhow::ensure!(
                    ids.len() <= 16 && ids.iter().all(|id| (1..=253).contains(id)),
                    "監視IDは1〜253、最大16台です"
                );
                self.sts.monitor = ids;
            }
            Operation::Read { id, address, width } => {
                anyhow::ensure!(
                    (1..=253).contains(&id)
                        && (width == 1 || width == 2)
                        && u16::from(address) + u16::from(width) <= 71,
                    "読取り範囲が不正です"
                );
                self.sts.command(20, id, [address, width, 0, 0], None);
            }
            Operation::Configure {
                id,
                address,
                width,
                value,
                single_servo,
            } => {
                anyhow::ensure!(
                    !self.sts.active && single_servo,
                    "出力停止と1台だけの接続確認が必要です"
                );
                anyhow::ensure!(
                    (1..=253).contains(&id) && (width == 1 || width == 2),
                    "ID・幅が不正です"
                );
                self.send("CAN 2 800 0103000000000000")?;
                self.sts.command(
                    21,
                    id,
                    [address, width, (value >> 8) as u8, value as u8],
                    None,
                );
            }
            Operation::Scan { first, last } => {
                anyhow::ensure!(
                    !self.sts.active && first >= 1 && last <= 253 && first <= last,
                    "停止中にID1〜253の範囲で探索してください"
                );
                self.send("CAN 2 800 0103000000000000")?;
                self.shared.update_status(|s| s.sts.discovered.clear());
                for id in first..=last {
                    self.sts.command(25, id, [0; 4], None);
                }
            }
            Operation::Move { targets } => {
                sts::validate_targets(&targets)?;
                self.sts.active = true;
                self.sts.lease = targets.iter().any(|t| t.mode == 1).then(Instant::now);
                self.sts.motion(&targets, 0);
                self.sts.monitor = targets.iter().map(|t| t.id).collect();
            }
            Operation::Sequence { steps } => {
                anyhow::ensure!(
                    !self.sts.active && !steps.is_empty() && steps.len() <= 64,
                    "停止中に1〜64ステップを指定してください"
                );
                let mut ids = std::collections::BTreeSet::new();
                for Step { wait_ms, targets } in &steps {
                    sts::validate_targets(targets)?;
                    ids.extend(targets.iter().map(|t| t.id));
                    anyhow::ensure!(
                        (100..=60000).contains(wait_ms),
                        "待機時間は100〜60000msです"
                    );
                }
                anyhow::ensure!(ids.len() <= 16, "シーケンス全体で最大16台です");
                self.sts.monitor = ids.into_iter().collect();
                self.sts.active = true;
                self.sts.sequence = true;
                for step in steps {
                    self.sts.motion(&step.targets, step.wait_ms);
                }
            }
            Operation::Renew => unreachable!(),
        }
        self.shared
            .update_status(|s| s.sts.message = "要求受付。基板の確認応答を待っています".into());
        Ok(Reply::accepted())
    }

    pub(super) fn observe_sts(&mut self, line: &str) -> Result<()> {
        if let Some(state) = sts::frame(line, 801) {
            anyhow::ensure!(
                !self.sts.armed || (state[1] == 0 && state[2] == 1),
                "SerialSVMDが停止または指令拒否を通知しました"
            );
        }
        if let Some(p) = sts::frame(line, 807)
            && let Some((command, _)) = &self.sts.pending
            && command.packet[1] == 24
            && p[1] == command.packet[2]
            && p[2] == command.packet[3]
            && p[3] < 4
        {
            for i in 0..4 {
                let n = usize::from(p[3]) * 4 + i;
                if n < 15 {
                    self.sts.bytes[n] = p[4 + i];
                }
            }
            self.sts.parts |= 1 << p[3];
        }
        let Some(reply) = sts::frame(line, 806) else {
            return Ok(());
        };
        let Some((pending, _)) = &self.sts.pending else {
            return Ok(());
        };
        if reply[1..4] != pending.packet[1..4] {
            return Ok(());
        }
        let (command, _) = self.sts.pending.take().unwrap();
        let value = u16::from(reply[5]) | u16::from(reply[6]) << 8;
        if reply[4] != 0 && !(reply[1] == 25 && reply[4] == 3) {
            anyhow::bail!(
                "STS ID {} op={} result={} flags=0x{:02X}。設定変更中なら新旧ID/baudを確認してください",
                reply[2],
                reply[1],
                reply[4],
                reply[7]
            );
        }
        if let Some(expected) = command.expected {
            anyhow::ensure!(
                expected == value,
                "ID {} のモードが不一致です（設定={}、要求={}）",
                reply[2],
                value,
                expected
            );
        }
        if reply[1] == 23 {
            self.sts.armed = true;
        }
        if reply[1] == 25 && reply[4] == 0 {
            self.shared.update_status(|s| {
                if !s.sts.discovered.contains(&reply[2]) {
                    s.sts.discovered.push(reply[2]);
                }
            });
        }
        if reply[1] == 21 && matches!(command.packet[4], 5 | 6) {
            let changed = u16::from_be_bytes([command.packet[6], command.packet[7]]);
            if command.packet[4] == 5 {
                if let Some(board) = &mut self.cfg.machine.serial_svmd {
                    for servo in &mut board.servos {
                        if servo.id == reply[2] {
                            servo.id = changed as u8;
                        }
                    }
                }
                for id in &mut self.sts.monitor {
                    if *id == reply[2] {
                        *id = changed as u8;
                    }
                }
            } else {
                const BAUDS: [f32; 8] = [
                    1000000., 500000., 250000., 128000., 115200., 76800., 57600., 38400.,
                ];
                let baud = *BAUDS
                    .get(usize::from(changed))
                    .context("通信速度コードが不正です")?;
                self.cfg
                    .machine
                    .serial_svmd_parameters
                    .insert("servo_baud".into(), baud);
                self.settings = Settings::new(&self.cfg.machine);
                self.setup = true;
            }
            self.shared.set_config(self.cfg.clone());
            self.shared.update_status(|s| s.saved = false);
        }
        if reply[1] == 24 {
            anyhow::ensure!(self.sts.parts == 15, "STS計測フレームが欠落しました");
            let d = self.sts.bytes;
            let word = |i| u16::from(d[i]) | u16::from(d[i + 1]) << 8;
            let sample = Sample {
                id: reply[2],
                elapsed_ms: self
                    .sts
                    .epoch
                    .get_or_insert_with(Instant::now)
                    .elapsed()
                    .as_millis() as u64,
                position: sts::decode_signed(word(0), 15),
                speed: sts::decode_signed(word(2), 15),
                load: sts::decode_signed(word(4), 10),
                voltage: f32::from(d[6]) / 10.0,
                temperature: d[7],
                current_raw: sts::decode_signed(word(13), 15),
                current_ma: sts::decode_signed(word(13), 15) as f32 * 6.5,
                moving: d[10] != 0,
            };
            self.shared.update_status(|s| {
                s.sts.samples.push_back(sample);
                while s.sts.samples.len() > 1200 {
                    s.sts.samples.pop_front();
                }
            });
        }
        if command.wait_ms > 0 {
            self.sts.wait_until =
                Some(Instant::now() + Duration::from_millis(u64::from(command.wait_ms)));
        }
        if reply[1] != 24 {
            self.shared.update_status(|s| {
                s.sts.message = if reply[1] == 20 {
                    format!("ID {} レジスタ{} = {}", reply[2], command.packet[4], value)
                } else if reply[1] == 25 {
                    format!(
                        "ID {} まで探索{}・{}台検出",
                        reply[2],
                        if self.sts.queue.is_empty() {
                            "完了"
                        } else {
                            "中"
                        },
                        s.sts.discovered.len()
                    )
                } else {
                    format!(
                        "ID {} op={} 確認済み（result={}）",
                        reply[2], reply[1], reply[4]
                    )
                }
            });
        }
        Ok(())
    }

    pub(super) fn tick_sts(&mut self, now: Instant) -> Result<()> {
        if !self.sts.interested {
            return Ok(());
        }
        if !self.fresh() {
            self.sts.cancel();
            return Ok(());
        }
        if self
            .sts
            .lease
            .is_some_and(|last| now.duration_since(last) > Duration::from_millis(150))
        {
            self.stop(true)?;
            return Ok(());
        }
        if self.sts.active {
            self.send("CAN 2 800 0105000000000000")?;
        }
        if let Some((_, sent)) = &self.sts.pending {
            anyhow::ensure!(
                sent.elapsed() < Duration::from_millis(1500),
                "STS拡張操作の確認応答がありません。対応FWと接続を確認してください"
            );
            return Ok(());
        }
        if let Some(until) = self.sts.wait_until {
            if now < until {
                self.poll_sts_monitor(now)?;
                return Ok(());
            }
            self.sts.wait_until = None;
        }
        if let Some(command) = self.sts.queue.pop_front() {
            self.send(&sts::line(command.packet))?;
            if command.packet[1] >= 20 {
                self.sts.parts = 0;
                self.sts.pending = Some((command, now));
            }
            return Ok(());
        }
        if self.sts.sequence {
            self.stop(true)?;
            self.shared
                .update_status(|s| s.sts.message = "シーケンス完了・出力停止".into());
            return Ok(());
        }
        self.poll_sts_monitor(now)?;
        Ok(())
    }
    fn poll_sts_monitor(&mut self, now: Instant) -> Result<()> {
        if !self.sts.monitor.is_empty()
            && self
                .sts
                .last_poll
                .is_none_or(|last| last.elapsed() >= Duration::from_millis(100))
        {
            self.sts.monitor_index %= self.sts.monitor.len();
            let id = self.sts.monitor[self.sts.monitor_index];
            self.sts.monitor_index += 1;
            self.sts.last_poll = Some(now);
            let tag = self.sts.tag();
            let command = Command {
                packet: [1, 24, id, tag, 0, 0, 0, 0],
                expected: None,
                wait_ms: 0,
            };
            self.sts.parts = 0;
            self.send(&sts::line(command.packet))?;
            self.sts.pending = Some((command, now));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn runtime() -> Runtime {
        let shared = Arc::new(Shared::new(BridgeConfig {
            serial_device: "unused".into(),
            baud_rate: 115200,
            rate_hz: 20.0,
            machine: MachineProfile::parse(include_str!(
                "../../../config/serial_svmd_bringup.toml"
            ))
            .unwrap(),
            profile_path: "/dev/null".into(),
            simulate: true,
        }));
        let mut runtime = Runtime::new(shared);
        pump(&mut runtime, 30);
        runtime
    }
    fn pump(r: &mut Runtime, n: usize) {
        for _ in 0..n {
            r.tick().unwrap();
        }
    }
    fn request(r: &mut Runtime, operation: Operation) {
        r.request(
            &Request {
                text: Some(toml::to_string(&operation).unwrap()),
                ..Request::new("sts")
            },
            true,
        )
        .unwrap();
    }
    #[test]
    fn scan_configure_new_id_and_readback_use_simulator() {
        let mut r = runtime();
        request(&mut r, Operation::Scan { first: 1, last: 4 });
        pump(&mut r, 12);
        assert_eq!(r.shared.status_snapshot().sts.discovered, vec![1, 2, 3]);
        request(
            &mut r,
            Operation::Configure {
                id: 1,
                address: 5,
                width: 1,
                value: 9,
                single_servo: true,
            },
        );
        pump(&mut r, 4);
        request(
            &mut r,
            Operation::Read {
                id: 9,
                address: 5,
                width: 1,
            },
        );
        pump(&mut r, 4);
        assert!(r.shared.status_snapshot().sts.message.contains("= 9"));
    }
    #[test]
    fn synchronized_motion_is_exclusive_and_stop_cancels_pending_steps() {
        let mut r = runtime();
        request(
            &mut r,
            Operation::Move {
                targets: vec![
                    Target::default(),
                    Target {
                        id: 2,
                        value: 2200,
                        ..Target::default()
                    },
                ],
            },
        );
        assert!(r.request(&Request::new("run"), true).is_err());
        pump(&mut r, 30);
        assert!(r.sts.active, "{}", r.error);
        assert!(
            r.shared
                .status_snapshot()
                .sts
                .samples
                .iter()
                .any(|s| s.position == 2048)
        );
        r.stop(false).unwrap();
        assert!(!r.sts.active);
        assert!(r.sts.queue.is_empty());
        assert!(r.sts.pending.is_none());
    }
    #[test]
    fn mode_mismatch_and_velocity_lease_stop_without_restarting() {
        let mut r = runtime();
        request(
            &mut r,
            Operation::Move {
                targets: vec![Target {
                    mode: 1,
                    value: 100,
                    ..Target::default()
                }],
            },
        );
        pump(&mut r, 6);
        assert!(!r.sts.active);
        assert!(r.error.contains("モードが不一致"));
        r.error.clear();
        request(
            &mut r,
            Operation::Configure {
                id: 1,
                address: 33,
                width: 1,
                value: 1,
                single_servo: true,
            },
        );
        pump(&mut r, 4);
        request(
            &mut r,
            Operation::Move {
                targets: vec![Target {
                    mode: 1,
                    value: -100,
                    ..Target::default()
                }],
            },
        );
        pump(&mut r, 20);
        assert!(r.sts.active, "{}", r.error);
        r.sts.lease = Some(Instant::now() - Duration::from_millis(151));
        pump(&mut r, 1);
        assert!(!r.sts.active);
    }
    #[test]
    fn stale_ack_cannot_complete_request_and_sequence_waits_then_stops() {
        let mut r = runtime();
        request(
            &mut r,
            Operation::Read {
                id: 1,
                address: 33,
                width: 1,
            },
        );
        r.tick_sts(Instant::now()).unwrap();
        let expected = r.sts.pending.as_ref().unwrap().0.packet;
        let reply = [1, 20, 2, expected[3], 0, 0, 0, 0];
        let line = format!(
            "CAN_RX bus=2 id=806 data={}",
            reply.iter().map(|b| format!("{b:02X}")).collect::<String>()
        );
        r.observe_sts(&line).unwrap();
        assert!(r.sts.pending.is_some());
        pump(&mut r, 4);
        request(
            &mut r,
            Operation::Sequence {
                steps: vec![Step {
                    wait_ms: 100,
                    targets: vec![Target::default()],
                }],
            },
        );
        pump(&mut r, 20);
        assert!(r.sts.wait_until.is_some());
        assert!(r.sts.active);
        r.sts.wait_until = Some(Instant::now());
        pump(&mut r, 2);
        assert!(!r.sts.active);
        assert!(r.shared.status_snapshot().sts.message.contains("完了"));
    }
}

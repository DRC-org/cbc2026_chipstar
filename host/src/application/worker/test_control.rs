use super::*;
use crate::diagnostics::individual::{Kind, Target};

#[derive(Default)]
pub(super) struct TestControl {
    pub enabled: bool,
    pub selected: Option<(Target, Kind)>,
    pub active: bool,
    pub restart_blocked: bool,
    pub renewed: Option<Instant>,
    pub started: Option<Instant>,
    pub confirmed: bool,
    pub peers: std::collections::BTreeMap<&'static str, Instant>,
    pub last_poll: Option<Instant>,
}
impl Runtime {
    pub(super) fn uses_test_board(&self, board: &str) -> bool {
        (board == "sts" && self.sts.interested)
            || self.test.peers.contains_key(board)
            || self
                .test
                .selected
                .is_some_and(|(target, _)| target.board() == board)
            || match board {
                "pwm" => {
                    !self.cfg.machine.pwm_servos.is_empty()
                        || !self.cfg.machine.svmd_parameters.is_empty()
                }
                "dc" => {
                    !self.cfg.machine.dc_motors.is_empty()
                        || !self.cfg.machine.dcmd_parameters.is_empty()
                }
                "sts" => {
                    self.cfg.machine.requires_serial_svmd()
                        || !self.cfg.machine.serial_svmd_parameters.is_empty()
                }
                _ => false,
            }
    }
    pub(super) fn observe_test_reply(&mut self, line: &str) {
        let settled = self
            .test
            .started
            .is_some_and(|time| time.elapsed() > Duration::from_millis(500));
        for (id, board) in [(769, "pwm"), (785, "dc"), (801, "sts")] {
            let Some(data) = line.strip_prefix(&format!("CAN_RX bus=2 id={id} data=")) else {
                continue;
            };
            if data.len() != 16
                || !data.starts_with("01")
                || !data.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                continue;
            }
            let bytes: Vec<u8> = (0..8)
                .map(|i| u8::from_str_radix(&data[i * 2..i * 2 + 2], 16).expect("validated hex"))
                .collect();
            self.test.peers.insert(board, Instant::now());
            self.shared.update_status(|s| {
                s.peripherals
                    .insert(board.into(), format!("状態応答 {data}"));
            });
            if self.test.active
                && let Some((target, _)) = self.test.selected
                && target.board() == board
            {
                let disabled = match target {
                    Target::Pwm(channel) => bytes[4] & (1 << channel) == 0,
                    Target::Dc => bytes[2] != 1 || bytes[3] != 1,
                    Target::Sts(_) => bytes[2] != 1,
                    _ => false,
                };
                if bytes[1] != 0 || (settled && disabled) {
                    self.fault(format!(
                        "テスト対象基板 {board} が指令を拒否、または出力を解除しました: {data}"
                    ));
                }
            }
        }
        if let Some(servo) = crate::protocol::serial_svmd::parse_state(line)
            && self.test.active
            && self
                .test
                .selected
                .is_some_and(|(target, _)| target == Target::Sts(servo.id))
            && (servo.error != 0 || (settled && !servo.enabled))
        {
            self.fault(format!(
                "テスト対象サーボ {} の応答・出力が異常です",
                servo.id
            ));
        }
    }
    pub(super) fn test_request(&mut self, req: &Request, manual: bool) -> Result<Reply> {
        if !manual {
            bail!("個別テストはGUIから操作してください");
        }
        match req.action.as_str() {
            "test_mode" => {
                let enabled = req.flag.context("モードが必要です")?;
                let result = self.stop(true);
                if result.is_ok() || !enabled {
                    self.test.restart_blocked = false;
                    self.test.enabled = enabled;
                    self.test.selected = None;
                }
                result?;
            }
            "test_select" => {
                if !self.test.enabled {
                    bail!("個別テストモードに切り替えてください");
                }
                let target = Target::parse(req.axis.as_deref().context("対象が必要です")?)?;
                let kind = Kind::parse(req.text.as_deref().context("方式が必要です")?)?;
                target.limits(kind, &self.cfg.machine)?;
                self.stop(true)?;
                self.test.restart_blocked = false;
                self.test.selected = Some((target, kind));
                self.test.last_poll = None;
            }
            "test_off" => {
                let result = self.stop(true);
                self.test.restart_blocked = false;
                result?;
            }
            "test_output" => {
                if !self.test.enabled || self.emergency {
                    bail!("個別テストを開始できません");
                }
                if !self.fresh() || !self.setup || !self.settings.ready() || self.setup_error {
                    bail!("接続と設定反映を確認してください");
                }
                let (target, kind) = self.test.selected.context("対象を選択してください")?;
                let deliberate_position = !kind.momentary() && req.flag == Some(true);
                if self.test.restart_blocked && !deliberate_position {
                    bail!("出力停止済みです。ボタンを離してから押し直してください");
                }
                let value = req.value.context("指令値が必要です")?;
                target.validate(kind, value, &self.cfg.machine)?;
                if matches!(target, Target::Cctl(_)) {
                    if !self
                        .device
                        .as_ref()
                        .is_some_and(|d| d.motor_layout == "el05,m3508,m3508")
                    {
                        bail!("M3508×2台対応のcctl FWが必要です");
                    }
                    let t = self.telemetry.as_ref().unwrap();
                    let Target::Cctl(slot) = target else {
                        unreachable!()
                    };
                    let error = t.error_bits[slot as usize];
                    if t.buses & 1 == 0 || t.stale_slots & (1 << slot) != 0 || error != 0 {
                        bail!("対象モータの応答・異常を確認してください");
                    }
                } else if self.telemetry.as_ref().unwrap().buses & 2 == 0
                    || !self
                        .test
                        .peers
                        .get(target.board())
                        .is_some_and(|seen| seen.elapsed() < Duration::from_secs(2))
                {
                    bail!("対象基板の応答がありません");
                }
                if deliberate_position {
                    self.test.restart_blocked = false;
                }
                if !self.test.active {
                    self.stop(true)?;
                    self.error.clear();
                    // 送信途中の失敗でも後続の周期で停止するため、送信前に出力中とする。
                    self.test.active = true;
                    self.test.started = Some(Instant::now());
                    self.test.confirmed = false;
                    for line in target.begin() {
                        self.send(&line)?;
                    }
                }
                self.test.renewed = Some(Instant::now());
                for line in target.command(kind, value) {
                    self.send(&line)?;
                }
            }
            _ => bail!("未対応の個別テスト操作です"),
        }
        Ok(Reply::accepted())
    }
    pub(super) fn tick_test(&mut self, now: Instant) -> Result<()> {
        if self.test.active {
            let (target, kind) = self.test.selected.context("テスト対象がありません")?;
            if kind.momentary()
                && self.test.renewed.is_none_or(|stamp| {
                    now.saturating_duration_since(stamp) > Duration::from_millis(150)
                })
            {
                self.stop(true)?;
            } else if target.board() != "cctl"
                && !self.test.peers.get(target.board()).is_some_and(|seen| {
                    now.saturating_duration_since(*seen) < Duration::from_secs(2)
                })
            {
                self.fault("テスト対象基板の応答が途切れました".into());
            } else {
                self.send(target.heartbeat())?;
            }
        }
        if self.fresh()
            && self.test.last_poll.is_none_or(|stamp| {
                now.saturating_duration_since(stamp) > Duration::from_millis(500)
            })
        {
            self.test.last_poll = Some(now);
            // 読取りだけで出力を有効化しない。
            for (board, line) in [
                ("pwm", "CAN 2 768 0103000000000000"),
                ("dc", "CAN 2 784 0106000000000000"),
                ("sts", "CAN 2 800 0108000000000000"),
            ] {
                if self.uses_test_board(board) {
                    self.send(line)?;
                }
            }
            if let Some((Target::Sts(id), _)) = self.test.selected {
                self.send(&crate::protocol::serial_svmd::Command::Read { id }.to_cctl_line())?;
            }
        }
        Ok(())
    }
}

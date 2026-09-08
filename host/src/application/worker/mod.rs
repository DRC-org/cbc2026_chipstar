//! 機体接続と操作権を単一ワーカーが所有する。
use super::authority::Authority;
use crate::{
    application::app_state::{BridgeConfig, Shared},
    application::command::{Reply, Request},
    application::settings::Settings,
    input::{ControllerState, controller},
    machine::{MachineController, MachineProfile},
    protocol::device::{DeviceInfo, parse_device_info},
    protocol::telemetry::{RunMode, Telemetry, parse_telemetry},
    transport::link::Link,
};
use anyhow::{Context, Result, bail};
use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const FRESH: Duration = Duration::from_millis(200);

/// RUN要求の受付と、基板での反映を区別する。
#[derive(Clone, Copy)]
enum DriveState {
    Stopped,
    AwaitingRun(Instant),
    Running,
}
impl DriveState {
    fn running(self) -> bool {
        matches!(self, Self::Running)
    }
    fn awaiting(self) -> Option<Instant> {
        match self {
            Self::AwaitingRun(sent) => Some(sent),
            _ => None,
        }
    }
}

struct Runtime {
    shared: Arc<Shared>,
    cfg: BridgeConfig,
    link: Link,
    machine: MachineController,
    settings: Settings,
    device: Option<DeviceInfo>,
    telemetry: Option<Telemetry>,
    rx: Option<Instant>,
    setup: bool,
    setup_error: bool,
    drive: DriveState,
    emergency: bool,
    test: test_control::TestControl,
    sts: sts_control::Control,
    ee: ee_control::Control,
    homing: Option<homing::Homing>,
    pad: pad_control::Control,
    authority: Authority,
    manual_input: ControllerState,
    gamepad_input: Option<ControllerState>,
    screen_control: bool,
    screen_input_times: [Option<Instant>; 6],
    gamepad_name: String,
    adjustment: bool,
    last_hello: Instant,
    reason: String,
    error: String,
    communication_error: Option<String>,
}
impl Runtime {
    fn new(shared: Arc<Shared>) -> Self {
        let cfg = shared.config();
        Self {
            link: Link::new(&cfg.serial_device, cfg.baud_rate, cfg.simulate),
            machine: MachineController::new(cfg.machine.clone()),
            settings: Settings::new(&cfg.machine),
            screen_control: cfg.simulate,
            screen_input_times: [None; 6],
            cfg,
            shared,
            device: None,
            telemetry: None,
            rx: None,
            setup: false,
            setup_error: false,
            drive: DriveState::Stopped,
            emergency: false,
            test: test_control::TestControl::default(),
            sts: sts_control::Control::default(),
            ee: ee_control::Control::default(),
            homing: None,
            pad: pad_control::Control::default(),
            authority: Authority::default(),
            manual_input: ControllerState::default(),
            gamepad_input: None,
            gamepad_name: String::new(),
            adjustment: false,
            last_hello: Instant::now() - Duration::from_secs(2),
            reason: "接続待ち".into(),
            error: String::new(),
            communication_error: None,
        }
    }
    fn send(&mut self, line: &str) -> Result<()> {
        self.shared.log(format!("TX {line}"));
        self.link.write_line(line)?;
        self.shared.update_status(|s| s.tx_count += 1);
        Ok(())
    }
    fn fresh(&self) -> bool {
        self.rx.is_some_and(|t| t.elapsed() <= FRESH)
    }
    fn clear_drive(&mut self) {
        self.drive = DriveState::Stopped;
        self.authority.clear_input();
        if self.screen_control {
            self.manual_input = ControllerState::default();
        }
        self.screen_input_times = [None; 6];
    }
    fn stop(&mut self, cut: bool) -> Result<()> {
        let cut = cut
            // RUN送信後、応答前は保持対象が確定していない。JOG 0ではなくSTOPで競合を閉じる。
            || self.drive.awaiting().is_some()
            || self.test.enabled
            || self.sts.active
            || !self.ee.targets.is_empty()
            || self.homing.is_some();
        self.homing = None;
        self.pad.ee_armed = false;
        self.pad.home_since = None;
        self.pad.home_ready = false;
        self.ee = ee_control::Control::default();
        self.sts.cancel();
        self.test.restart_blocked |= self.test.active;
        self.test.active = false;
        self.test.renewed = None;
        self.test.started = None;
        self.test.confirmed = false;
        self.clear_drive();
        if !self.fresh() {
            let _ = self.send("STOP");
            let _ = self.stop_peripherals();
        } else if cut {
            let main = self.send("STOP");
            let peripherals = self.stop_peripherals();
            main.and(peripherals)?;
        } else if self
            .telemetry
            .as_ref()
            .is_some_and(|t| t.mode == RunMode::Run)
        {
            for axis in self.cfg.machine.axes.clone() {
                self.send(&format!("JOG {} 0", axis.slot))?;
            }
        }
        self.reason = if cut {
            "出力停止。原点と姿勢を確認して再開"
        } else {
            "停止・保持中。再開操作を待っています"
        }
        .into();
        Ok(())
    }
    fn stop_peripherals(&mut self) -> Result<()> {
        // 機体設定に未登録の個別テスト対象も停止する。1基板の送信失敗で残りを省略しない。
        let mut result = Ok(());
        for (board, line) in [
            ("pwm", crate::protocol::svmd::Command::Stop.to_cctl_line()),
            ("dc", crate::protocol::dcmd::line(3, 0, 0)),
            (
                "sts",
                crate::protocol::serial_svmd::Command::Stop.to_cctl_line(),
            ),
        ] {
            if self.uses_test_board(board)
                && let Err(error) = self.send(&line)
            {
                result = Err(error);
            }
        }
        result
    }
    fn engage_emergency(&mut self) -> Result<()> {
        self.emergency = true;
        self.authority.release();
        self.stop(true)
    }
    fn fault(&mut self, reason: String) {
        let _ = self.stop(true);
        self.machine.invalidate_origins();
        self.reason = reason.clone();
        self.error = reason;
    }
    fn communication_failed(&mut self, reason: String) {
        if self.communication_error.as_ref() != Some(&reason) {
            self.shared.log(format!("通信切断: {reason}"));
        }
        self.communication_error = Some(reason);
        let _ = self.stop(true);
        self.test.peers.clear();
        self.clear_drive();
        self.machine.invalidate_origins();
        self.rx = None;
        self.device = None;
        self.telemetry = None;
        self.setup = false;
        self.setup_error = false;
        self.settings = Settings::new(&self.cfg.machine);
        self.reason = "通信失敗。再接続と原点確認が必要です".into();
    }
    fn ready(&self) -> Result<()> {
        if self.cfg.machine.axes.is_empty() {
            bail!("操縦するcctlの軸を指定してください");
        }
        if !self.fresh() {
            bail!("機体の応答がありません");
        }
        if crate::machine::PARAMETER_NAMES
            .iter()
            .filter(|name| !name.starts_with("dm_"))
            .any(|name| !self.cfg.machine.parameters.contains_key(*name))
        {
            bail!("cctlの有効な全36項目をPCの機体設定で指定してください");
        }
        if self.setup_error || !self.setup || !self.settings.ready() {
            bail!("設定の反映が確認できていません");
        }
        let Some(device) = &self.device else {
            bail!("能力確認待ち");
        };
        if device.board != "cctl"
            || device.protocol != 1
            || device.slots < 3
            || !device.jog
            || device.motor_layout != "el05,m3508,m3508"
        {
            bail!("M3508×2台対応のcctl FWが必要です");
        }
        let t = self.telemetry.as_ref().unwrap();
        if t.stale_slots != 0 || t.buses & 1 == 0 || t.error_bits.iter().any(|&e| e != 0) {
            bail!("モータ応答・異常状態を確認してください");
        }
        if self.cfg.machine.requires_can_bus_2()
            && (!device.can_buses.contains(&2) || t.buses & 2 == 0)
        {
            bail!("周辺基板のCAN接続がありません");
        }
        if !self.adjustment
            && self
                .machine
                .origin_states(Some(t))
                .iter()
                .any(|s| !s.captured)
        {
            bail!("原点を設定するか、「調整 > アーム」で原点位置まで手動移動してください");
        }
        Ok(())
    }
    fn start(&mut self) -> Result<()> {
        anyhow::ensure!(self.homing.is_none(), "ホーミングを停止してください");
        anyhow::ensure!(
            !self.sts.active && !self.sts.busy(),
            "STS操作を停止してください"
        );
        if self.emergency {
            bail!("ソフト緊停中です");
        }
        if self.test.enabled {
            bail!("個別テストモードを終了してください");
        }
        self.ready()?;
        let input = self.authority.input().unwrap_or(&self.manual_input);
        if input.axes.iter().any(|v| !v.is_finite() || v.abs() >= 0.1) {
            bail!("スティックを中立に戻してください");
        }
        if !self.authority.active()
            && self.gamepad_name.is_empty()
            && !self.cfg.simulate
            && !self.screen_control
        {
            bail!("DualSenseを接続してください");
        }
        let mask = self
            .cfg
            .machine
            .axes
            .iter()
            .fold(0u8, |m, a| m | 1 << a.slot);
        let excluded = 7 & !mask;
        if excluded != 0 {
            self.send(&format!("ENABLE {excluded} 0"))?;
        }
        self.send(&format!("ENABLE {mask} 1"))?;
        self.send("RUN")?;
        self.drive = DriveState::AwaitingRun(Instant::now());
        self.error.clear();
        self.reason = "基板の運転応答待ち".into();
        Ok(())
    }
    fn receive(&mut self) -> Result<()> {
        for line in self.link.read_lines()? {
            self.observe_test_reply(&line);
            if let Err(error) = self.observe_ee(&line) {
                self.fault(error.to_string());
            }
            if let Err(error) = self.observe_sts(&line) {
                self.sts.stop_monitoring();
                self.shared
                    .update_status(|s| s.sts.message = error.to_string());
                self.fault(error.to_string());
            }
            for board in [
                crate::protocol::board::Board::Dcmd,
                crate::protocol::board::Board::SerialSvmd,
            ] {
                if let Some(input) = crate::protocol::inputs::parse(&line, board) {
                    self.shared.update_status(|s| {
                        s.peripherals.insert(
                            format!("{} 接点", board.key()),
                            crate::protocol::inputs::describe(&input),
                        );
                    });
                }
            }
            self.shared.log(format!("RX {line}"));
            if let Some((id, detail)) = crate::protocol::serial_svmd::parse_diagnostic(&line) {
                self.shared.update_status(|s| {
                    s.peripherals
                        .insert(format!("STS3215 ID {id} 通信診断"), detail);
                });
            }
            if let Some(status) = crate::protocol::dcmd::parse_status(&line) {
                self.shared.update_status(|s| {
                    s.peripherals.insert(
                        "DCMD".into(),
                        format!(
                            "動作モード={} 出力={} デューティ={} 結果コード={}",
                            status.mode, status.enabled, status.duty[0], status.result
                        ),
                    );
                });
            }
            if let Some(encoder) = crate::protocol::dcmd::parse_encoder(&line) {
                self.shared.update_status(|s| {
                    s.peripherals.insert(
                        "ボーナス機構エンコーダ".into(),
                        format!(
                            "位置カウント={} 原点通過回数={}",
                            encoder.count, encoder.index_count
                        ),
                    );
                });
            }
            if let Some(servo) = crate::protocol::serial_svmd::parse_state(&line) {
                self.shared.update_status(|s| {
                    if servo.error == 0 {
                        s.peripherals
                            .remove(&format!("STS3215 ID {} 通信診断", servo.id));
                    }
                    s.peripherals.insert(
                        format!("STS3215 ID {}", servo.id),
                        format!(
                            "位置={} 出力={} エラーコード={}",
                            servo.position,
                            if servo.enabled { "有効" } else { "解除" },
                            servo.error
                        ),
                    );
                });
            }
            if let Some(body) = line.strip_prefix("C620_SCAN mask=")
                && let Some(mask) = body
                    .split_whitespace()
                    .next()
                    .and_then(|v| v.parse::<u8>().ok())
            {
                let ids = (1..=8)
                    .filter(|id| mask & (1 << (id - 1)) != 0)
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                self.shared.update_status(|s| {
                    s.peripherals.insert(
                        "C620 検出ID".into(),
                        if ids.is_empty() {
                            "応答なし（直近500ms）".into()
                        } else {
                            ids
                        },
                    );
                });
            }
            if line.starts_with("ERR ") {
                // 駆動拒否でも停止するが、照合済みの設定まで失敗扱いにしない。
                self.setup_error |= !self.settings.ready();
                self.fault(line);
                continue;
            }
            if let Err(error) = self.settings.receive(&line) {
                self.setup_error = true;
                self.fault(error.to_string());
            }
            if let Some(device) = parse_device_info(&line) {
                if device.motor_layout != "el05,m3508,m3508" {
                    self.setup = false;
                    self.fault("M3508×2台対応のcctl FWへ更新してください".into());
                }
                self.device = Some(device);
            }
            if let Some(t) = parse_telemetry(&line) {
                if self.telemetry.as_ref().is_some_and(|old| {
                    t.uptime_ms < old.uptime_ms
                        && old.uptime_ms.wrapping_sub(t.uptime_ms) < 0x80000000
                }) {
                    self.fault("基板の再起動を検出しました".into());
                    self.setup = false;
                    self.setup_error = false;
                    self.settings = Settings::new(&self.cfg.machine);
                }
                self.rx = Some(Instant::now());
                if let Some(contacts) = t.contacts {
                    self.shared.update_status(|s| {
                        s.peripherals.insert(
                            "cctl 接点".into(),
                            format!("SW1〜SW3（閉=1）={contacts:03b}"),
                        );
                    });
                }
                if self.test.active
                    && let Some((crate::diagnostics::individual::Target::Cctl(slot), _)) =
                        self.test.selected
                {
                    let error = t.error_bits[slot as usize];
                    let enabled = t.mode == RunMode::Run && t.enabled_slots & (1 << slot) != 0;
                    if enabled {
                        self.test.confirmed = true;
                    }
                    if (!enabled
                        && (self.test.confirmed
                            || self
                                .test
                                .started
                                .is_some_and(|time| time.elapsed() > Duration::from_millis(500))))
                        || t.stale_slots & (1 << slot) != 0
                        || error != 0
                    {
                        self.fault("個別テスト対象の出力または応答を失いました".into());
                    }
                }
                let before = self.machine.origin_states(self.telemetry.as_ref());
                self.machine.observe(&t);
                let origin_lost = self
                    .machine
                    .origin_states(Some(&t))
                    .iter()
                    .zip(&before)
                    .any(|(now, old)| now.lost && !old.lost);
                let mask = self
                    .cfg
                    .machine
                    .axes
                    .iter()
                    .fold(0u8, |m, a| m | 1 << a.slot);
                if self.drive.running()
                    && (t.mode != RunMode::Run || t.enabled_slots & mask != mask || origin_lost)
                {
                    self.fault("運転状態または原点を失いました".into());
                }
                if self.drive.awaiting().is_some() && t.mode == RunMode::Run {
                    let mask = self
                        .cfg
                        .machine
                        .axes
                        .iter()
                        .fold(0u8, |m, a| m | 1 << a.slot);
                    if t.enabled_slots & mask == mask {
                        self.drive = DriveState::Running;
                        self.reason = "運転中".into();
                    }
                }
                self.shared.response_history.lock().unwrap().record(
                    &t,
                    &self.cfg.machine,
                    &self.machine.origin_states(Some(&t)),
                );
                self.telemetry = Some(t);
            }
        }
        Ok(())
    }
    fn tick(&mut self) -> Result<()> {
        self.tick_at(Instant::now())
    }
    fn tick_at(&mut self, now: Instant) -> Result<()> {
        self.receive()?;
        if self.authority.expired(now) {
            self.stop(false)?;
            self.authority.release();
            self.reason = "AI操作権の期限切れ。通常操縦は再開待ち".into();
        }
        self.authority.expire_inputs(now);
        if self.screen_control {
            for (index, stamp) in self.screen_input_times.iter_mut().enumerate() {
                if stamp.is_some_and(|time| now.duration_since(time) > Duration::from_millis(150)) {
                    self.manual_input.axes[index] = 0.0;
                    *stamp = None;
                }
            }
        }
        if !self.fresh() && self.rx.is_some() {
            self.communication_failed("機体応答の期限切れ。原点を確認して再開してください".into());
        }
        if self.last_hello.elapsed() > Duration::from_secs(1) {
            self.last_hello = Instant::now();
            self.send("HELLO 1")?;
            if !self.cfg.machine.dc_motors.is_empty() {
                self.send(&crate::protocol::dcmd::line(0, 0, 0))?;
            }
            if self.cfg.machine.requires_serial_svmd() {
                self.send(&crate::protocol::serial_svmd::Command::Hello.to_cctl_line())?;
            }
        }
        if self.fresh()
            && self
                .device
                .as_ref()
                .is_some_and(|d| d.motor_layout == "el05,m3508,m3508")
            && !self.setup
        {
            self.clear_drive();
            self.send("SAFE")?;
            self.stop_peripherals()?;
            self.setup = true;
        }
        if self.setup
            && !self.setup_error
            && self
                .telemetry
                .as_ref()
                .is_some_and(|t| t.mode == RunMode::Safe)
        {
            match self.settings.poll_command() {
                Ok(Some(line)) => self.send(&line)?,
                Ok(None) => {}
                Err(error) => {
                    self.setup_error = true;
                    self.fault(error.to_string());
                }
            }
        }
        if self
            .drive
            .awaiting()
            .is_some_and(|t| t.elapsed() > Duration::from_millis(500))
        {
            self.fault("RUNの反映応答がありません".into());
        }
        if self.drive.running() {
            if let Err(error) = self.ready() {
                self.fault(error.to_string());
            } else {
                let input = self.authority.input().unwrap_or(&self.manual_input);
                let slow = self.adjustment
                    || (!self.authority.active() && self.screen_control)
                    || input.buttons[9] != 0;
                let lines = self
                    .machine
                    .jog_lines(input, self.telemetry.as_ref().unwrap(), slow);
                for line in lines {
                    self.send(&line)?;
                }
            }
        }
        if self.communication_error.is_some() && self.fresh() && self.device.is_some() {
            self.communication_error = None;
            self.shared
                .log("通信復旧: 基板応答を確認。出力停止を維持".into());
            self.reason = "通信復旧。原点を確認して再開してください".into();
        }
        self.tick_test(now)?;
        if let Err(error) = self.tick_homing(now) {
            self.fault(error.to_string());
        }
        if let Err(error) = self.tick_ee(now) {
            self.fault(error.to_string());
        }
        if let Err(error) = self.tick_sts(now) {
            self.sts.stop_monitoring();
            self.shared
                .update_status(|s| s.sts.message = error.to_string());
            self.fault(error.to_string());
        }
        if self.emergency {
            self.stop(true)?;
        }
        if self.fresh() {
            self.send("HEARTBEAT")?;
            if !self.cfg.machine.pwm_servos.is_empty() {
                self.send("CAN 2 768 0103000000000000")?;
            }
            if !self.cfg.machine.dc_motors.is_empty() {
                self.send(&crate::protocol::dcmd::line(5, 0, 0))?;
            }
            if self.cfg.machine.requires_serial_svmd() {
                self.send("CAN 2 800 0105000000000000")?;
            }
        }
        Ok(())
    }
    fn publish(&self) {
        let origins = self.machine.origin_states(self.telemetry.as_ref());
        self.shared.update_status(|s| {
            s.test_mode = self.test.enabled;
            s.ee_targets = self.ee.targets.clone();
            s.homing = self.homing.as_ref().map(|h| h.label.clone());
            s.homing_confirmation = self.pad.home_since.map(|t| t.elapsed().as_secs_f32());
            s.test_active = self.test.active;
            s.test_ready = !self.emergency
                && self.fresh()
                && self.setup
                && self.settings.ready()
                && !self.setup_error
                && self.test.selected.is_some_and(|(target, _)| {
                    target.board() == "cctl"
                        || self
                            .test
                            .peers
                            .get(target.board())
                            .is_some_and(|time| time.elapsed() < Duration::from_secs(2))
                });
            s.test_target = self
                .test
                .selected
                .map(|(target, _)| target.key())
                .unwrap_or_default();
            s.test_kind = self
                .test
                .selected
                .map(|(_, kind)| kind.key().into())
                .unwrap_or_default();
            s.sts.elapsed_ms = self.sts.elapsed_ms();
            s.sts.active = self.sts.active;
            s.sts.busy = self.sts.busy();
            s.emergency = self.emergency;
            s.outputs_active = self.homing.is_some()
                || !self.ee.targets.is_empty()
                || self.sts.active
                || self.test.active
                || self.drive.awaiting().is_some()
                || self
                    .telemetry
                    .as_ref()
                    .is_some_and(|t| t.mode == RunMode::Run && t.enabled_slots != 0);
            s.operating_state = if self.emergency {
                "ソフト緊停中"
            } else if !self.fresh() {
                "接続断 / 状態不明"
            } else if self.homing.is_some() {
                "r・zホーミング中"
            } else if self.test.active {
                "個別テスト出力中"
            } else if self.drive.running() {
                "運転中"
            } else if self.drive.awaiting().is_some() {
                "運転応答待ち"
            } else if s.outputs_active {
                "停止・保持"
            } else {
                "出力停止"
            }
            .into();
            s.simulated = self.cfg.simulate;
            s.screen_control = self.screen_control;
            s.connected = self.fresh();
            s.configured = self.setup && self.settings.ready() && !self.setup_error;
            s.ai_active = self.authority.active();
            s.running = self.drive.running();
            s.origin_adjustment = self.adjustment;
            s.slow = self.adjustment
                || (!self.authority.active() && self.screen_control)
                || self.authority.input().unwrap_or(&self.manual_input).buttons[9] != 0;
            s.axes = self.authority.input().unwrap_or(&self.manual_input).axes;
            s.gamepad_input = self.gamepad_input.clone();
            s.origins = origins;
            s.gamepad = self.gamepad_name.clone();
            s.board_mode = self
                .telemetry
                .as_ref()
                .map(|t| t.mode.label().to_owned())
                .unwrap_or_default();
            s.reason = if let Some(homing) = &self.homing {
                homing.label.clone()
            } else if !self.drive.running() && self.drive.awaiting().is_none() {
                self.ready()
                    .err()
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| self.reason.clone())
            } else {
                self.reason.clone()
            };
            s.error = self
                .communication_error
                .clone()
                .unwrap_or_else(|| self.error.clone());
            s.telemetry_age_ms = self
                .rx
                .map(|t| t.elapsed().as_millis() as u64)
                .unwrap_or(999999);
            s.parameters_confirmed = self.settings.confirmed;
            s.parameters_expected = self.settings.expected;
            s.configuration = if self.setup_error {
                "反映失敗"
            } else if s.configured {
                "基板で反映確認済み"
            } else {
                "反映確認待ち"
            }
            .into();
        });
    }
}

pub fn run(shared: Arc<Shared>) {
    let mut runtime = Runtime::new(shared.clone());
    let mut gilrs = gilrs::Gilrs::new().ok();
    let mut selected = None;
    while shared.is_running() {
        let cycle = Instant::now();
        if shared.take_emergency() {
            let _ = runtime.engage_emergency();
        }
        if let Some(gilrs) = gilrs.as_mut() {
            while let Some(event) = gilrs.next_event() {
                gilrs.update(&event);
            }
            if selected.is_none() {
                let candidates: Vec<_> = gilrs
                    .gamepads()
                    .filter(|(_, p)| {
                        p.is_connected()
                            && (p.name().to_lowercase().contains("dualsense")
                                || (p.vendor_id() == Some(0x054c)
                                    && matches!(p.product_id(), Some(0x0ce6 | 0x0df2))))
                    })
                    .map(|(id, _)| id)
                    .collect();
                if candidates.len() == 1 {
                    selected = candidates.first().copied();
                }
            }
            if let Some(id) = selected {
                let pad = gilrs.gamepad(id);
                if pad.is_connected() {
                    runtime.gamepad_name = pad.name().into();
                    if let Err(error) = runtime.read_pad(controller::read(&pad), cycle) {
                        runtime.error = error.to_string();
                    }
                } else {
                    runtime.disconnect_pad();
                    selected = None;
                }
            }
        }
        let mut cancelled = false;
        for pending in shared.take_requests() {
            if shared.take_emergency() {
                let _ = runtime.engage_emergency();
                cancelled = true;
            }
            if cancelled && pending.request.action != "estop" {
                let _ = pending.reply.send(Reply::error("緊停により取消"));
                continue;
            }
            if Instant::now() > pending.deadline {
                let _ = pending.reply.send(Reply::error("要求の実行期限切れ"));
                continue;
            }
            let reply = runtime
                .request(&pending.request, pending.manual)
                .unwrap_or_else(|e| Reply::error(e.to_string()));
            if !reply.ok {
                runtime.error = reply.message.clone();
            }
            let _ = pending.reply.send(reply);
        }
        if let Err(error) = runtime.tick() {
            runtime.communication_failed(format!("{error:#}"));
        }
        runtime.publish();
        let period = Duration::from_secs_f64(1.0 / runtime.cfg.rate_hz.clamp(20.0, 100.0));
        thread::sleep(period.saturating_sub(cycle.elapsed()));
    }
    let _ = runtime.stop(true);
}

mod ee_control;
mod homing;
mod pad_control;
mod requests;
mod sts_control;
mod test_control;
#[cfg(test)]
mod tests;

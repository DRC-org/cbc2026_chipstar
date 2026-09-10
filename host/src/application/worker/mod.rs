//! 機体接続と操作権を単一ワーカーが所有する。
use super::authority::Authority;
use crate::{
    application::app_state::{
        BridgeConfig, CanBusStatus, CanDeviceStatus, CommunicationHealth, Court, PreparationPhase,
        PreparationStep, Shared,
    },
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
    court: Option<Court>,
    preparation: PreparationPhase,
    guide: pad_guide::Guide,
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
    bonus: bonus_control::Control,
    homing: Option<homing::Homing>,
    sequence: Option<sequence_control::Execution>,
    pad: pad_control::Control,
    authority: Authority,
    manual_input: ControllerState,
    gamepad_input: Option<ControllerState>,
    screen_control: bool,
    screen_input_times: [Option<Instant>; crate::input::MACHINE_INPUT_COUNT],
    gamepad_name: String,
    adjustment: bool,
    last_hello: Instant,
    reason: String,
    error: String,
    communication_error: Option<String>,
    can_diagnostics: std::collections::BTreeMap<u8, (crate::protocol::can::Diagnostics, Instant)>,
    c620_scan: Option<(u8, Instant)>,
    pwm_feedback: std::collections::BTreeMap<u8, PwmFeedback>,
    servo_feedback: std::collections::BTreeMap<u8, ServoFeedback>,
    prepared_rotation_field: Option<f32>,
    last_servo_health_poll: Option<Instant>,
    servo_health_poll_index: usize,
}

#[derive(Clone)]
struct PwmFeedback {
    seen: Instant,
    state: crate::protocol::svmd::State,
}

#[derive(Clone)]
struct ServoFeedback {
    seen: Instant,
    position: i32,
    error: u8,
    detail: String,
}
impl Runtime {
    fn new(shared: Arc<Shared>) -> Self {
        let cfg = shared.config();
        Self {
            court: None,
            preparation: PreparationPhase::Setting,
            guide: pad_guide::Guide::default(),
            link: Link::new(&cfg.serial_device, cfg.baud_rate, cfg.simulate),
            machine: MachineController::new(cfg.machine.clone()),
            settings: Settings::new(&cfg.machine),
            screen_control: cfg.simulate,
            screen_input_times: [None; crate::input::MACHINE_INPUT_COUNT],
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
            bonus: bonus_control::Control::default(),
            homing: None,
            sequence: None,
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
            can_diagnostics: std::collections::BTreeMap::new(),
            c620_scan: None,
            pwm_feedback: std::collections::BTreeMap::new(),
            servo_feedback: std::collections::BTreeMap::new(),
            prepared_rotation_field: None,
            last_servo_health_poll: None,
            servo_health_poll_index: 0,
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
        self.cancel_sequence("中断・停止");
        self.machine.reset_jog();
        self.drive = DriveState::Stopped;
        self.authority.clear_input();
        if self.screen_control {
            self.manual_input = ControllerState::default();
        }
        self.screen_input_times = [None; crate::input::MACHINE_INPUT_COUNT];
    }
    fn stop_with_z_hold(&mut self) -> Result<()> {
        anyhow::ensure!(!self.emergency, "緊停中はz保持を有効化できません");
        if !self.fresh() {
            anyhow::bail!("通信状態を確認できないためz保持へ移行できません");
        }
        let telemetry = self.telemetry.as_ref().context("実測位置がありません")?;
        anyhow::ensure!(
            telemetry.held_slots.is_some(),
            "z保持に対応したCCTLへ書き込んでください"
        );
        let axis = self
            .cfg
            .machine
            .axes
            .iter()
            .find(|a| a.name == "z")
            .context("z軸が設定されていません")?;
        let mask = 1u8 << axis.slot;
        anyhow::ensure!(
            telemetry.stale_slots & mask == 0,
            "zのフィードバックがありません"
        );
        self.send(&format!("HOLD {mask}"))?;
        self.homing = None;
        self.pad.ee_armed = false;
        self.pad.home_since = None;
        self.pad.home_ready = false;
        self.ee = ee_control::Control::default();
        self.bonus.cancel();
        self.sts.cancel();
        self.test.restart_blocked |= self.test.active;
        self.test.active = false;
        self.test.renewed = None;
        self.test.started = None;
        self.test.confirmed = false;
        self.clear_drive();
        self.stop_peripherals()?;
        self.reason = "出力停止・z位置保持中".into();
        Ok(())
    }
    fn configuration_failed(&mut self, error: String) {
        self.setup_error = true;
        if self
            .telemetry
            .as_ref()
            .is_some_and(|t| t.held_slots.unwrap_or(0) != 0)
        {
            self.error = error;
            self.reason = "設定適用失敗・z保持を継続中".into();
        } else {
            self.fault(error);
        }
    }
    fn stop(&mut self, cut: bool) -> Result<()> {
        if self.preparation == PreparationPhase::Waiting {
            self.preparation = PreparationPhase::Recovery;
        }
        let stop_ee = self.sts.active || !self.ee.targets.is_empty() || self.bonus.outputs_active();
        let cut = cut
            // RUN送信後、応答前は保持対象が確定していない。JOG 0ではなくSTOPで競合を閉じる。
            || self.drive.awaiting().is_some()
            || self.test.enabled
            || self.homing.is_some();
        self.homing = None;
        self.pad.ee_armed = false;
        self.pad.home_since = None;
        self.pad.home_ready = false;
        self.ee = ee_control::Control::default();
        self.bonus.cancel();
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
            let enabled_slots = self.telemetry.as_ref().unwrap().enabled_slots;
            let telemetry = self.telemetry.as_ref().unwrap().clone();
            for line in self.machine.hold_lines(&telemetry, enabled_slots) {
                self.send(&line)?;
            }
        }
        if !cut && stop_ee {
            self.stop_peripherals()?;
        }
        self.reason = if cut {
            "全トルク解除。原点と姿勢を確認して再開"
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
        if self.preparation != PreparationPhase::Setting {
            self.preparation = PreparationPhase::Recovery;
        }
        let _ = self.stop(true);
        self.reason = reason.clone();
        self.error = reason;
    }
    fn fault_ee(&mut self, reason: String) {
        if self.sequence.is_some() {
            self.cancel_sequence(&format!("EE異常で中断: {reason}"));
            if let Some(telemetry) = self.telemetry.clone() {
                for line in self.machine.hold_lines(&telemetry, telemetry.enabled_slots) {
                    let _ = self.send(&line);
                }
            }
        }
        self.ee = ee_control::Control::default();
        self.pad.ee_armed = false;
        let mut stop_error = None;
        if !self.cfg.machine.pwm_servos.is_empty()
            && let Err(error) = self.send(&crate::protocol::svmd::Command::Stop.to_cctl_line())
        {
            stop_error = Some(error.to_string());
        }
        if self.cfg.machine.requires_serial_svmd()
            && let Err(error) =
                self.send(&crate::protocol::serial_svmd::Command::Stop.to_cctl_line())
        {
            stop_error = Some(error.to_string());
        }
        let detail = match stop_error {
            Some(error) => format!("{reason} / EE停止指令: {error}"),
            None => reason,
        };
        self.reason = format!("EEを停止しました。r・θ・zの運転は継続しています: {detail}");
        self.error = detail;
    }
    fn communication_failed(&mut self, reason: String) {
        if self.communication_error.as_ref() != Some(&reason) {
            self.shared.log(format!("通信切断: {reason}"));
        }
        self.communication_error = Some(reason);
        let _ = self.stop(true);
        self.test.peers.clear();
        self.bonus.reset_reference();
        self.sts.clear_position_history();
        self.servo_feedback.clear();
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
        self.axes_ready(true)?;
        let device = self.device.as_ref().unwrap();
        let t = self.telemetry.as_ref().unwrap();
        if self.cfg.machine.requires_can_bus_2()
            && (!device.can_buses.contains(&2) || t.buses & 2 == 0)
        {
            bail!("周辺基板のCAN接続がありません");
        }
        Ok(())
    }
    fn axes_ready(&self, require_origins: bool) -> Result<()> {
        if self.cfg.machine.axes.is_empty() {
            bail!("操縦するcctlの軸を指定してください");
        }
        if !self.fresh() {
            bail!("機体の応答がありません");
        }
        if crate::machine::PARAMETER_NAMES
            .iter()
            .filter(|name| !name.starts_with("dm_") && !name.starts_with("reserved_"))
            .any(|name| !self.cfg.machine.parameters.contains_key(*name))
        {
            bail!("cctlの有効な全パラメータをPCの機体設定で指定してください");
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
        let mask = self
            .cfg
            .machine
            .axes
            .iter()
            .fold(0u8, |mask, axis| mask | 1 << axis.slot);
        let axis_error = self
            .cfg
            .machine
            .axes
            .iter()
            .any(|axis| t.error_bits[usize::from(axis.slot)] != 0);
        if t.stale_slots & mask != 0 || t.buses & 1 == 0 || axis_error {
            bail!("モータ応答・異常状態を確認してください");
        }
        if require_origins
            && !self.adjustment
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
        anyhow::ensure!(
            !self.preparation.locked(),
            "開始待ちです。準備画面で開始または再確認してください"
        );
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
                self.fault_ee(error.to_string());
            }
            if let Err(error) = self.observe_sts(&line) {
                self.fail_sts(error.to_string());
            }
            for board in [
                crate::protocol::board::Board::Dcmd,
                crate::protocol::board::Board::SerialSvmd,
            ] {
                if let Some(input) = crate::protocol::inputs::parse(&line, board) {
                    if board == crate::protocol::board::Board::Dcmd {
                        self.bonus.observe_contacts(input.stable, Instant::now());
                    }
                    self.shared.update_status(|s| {
                        s.peripherals.insert(
                            format!("{} 接点", board.key()),
                            crate::protocol::inputs::describe(&input),
                        );
                    });
                }
            }
            self.shared.log(format!("RX {line}"));
            if let Some(state) = crate::protocol::svmd::parse_state(&line) {
                self.pwm_feedback.insert(
                    state.channel,
                    PwmFeedback {
                        seen: Instant::now(),
                        state,
                    },
                );
            }
            if let Some((id, detail)) = crate::protocol::serial_svmd::parse_diagnostic(&line) {
                let position = self
                    .servo_feedback
                    .get(&id)
                    .map_or(0, |state| state.position);
                self.servo_feedback.insert(
                    id,
                    ServoFeedback {
                        seen: Instant::now(),
                        position,
                        error: 0xFF,
                        detail: detail.clone(),
                    },
                );
                self.shared.update_status(|s| {
                    s.peripherals.remove(&format!("STS3215 ID {id}"));
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
                self.bonus.observe_encoder(encoder.count, Instant::now());
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
                // STS3215の実測値は4096カウントごとに折り返す。通常運転の再開時にも
                // 同じ実測値を初回目標へ使えるよう、停止中を含めて連続位置に直す。
                let position = self.sts.unwrap_position(servo.id, servo.position);
                self.servo_feedback.insert(
                    servo.id,
                    ServoFeedback {
                        seen: Instant::now(),
                        position,
                        error: servo.error,
                        detail: format!(
                            "位置={}、出力={}、エラー=0x{:02X}",
                            position,
                            if servo.enabled { "有効" } else { "解除" },
                            servo.error
                        ),
                    },
                );
                self.shared.update_status(|s| {
                    if servo.error == 0 {
                        s.peripherals
                            .remove(&format!("STS3215 ID {} 通信診断", servo.id));
                    }
                    s.peripherals.insert(
                        format!("STS3215 ID {}", servo.id),
                        format!(
                            "位置={} 出力={} エラーコード={}",
                            position,
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
                self.c620_scan = Some((mask, Instant::now()));
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
            if let Some(diagnostics) = crate::protocol::can::parse_diagnostics(&line) {
                self.can_diagnostics
                    .insert(diagnostics.bus, (diagnostics, Instant::now()));
            }
            if line.starts_with("ERR ") {
                // 駆動拒否でも停止するが、照合済みの設定まで失敗扱いにしない。
                if !self.settings.ready() {
                    self.configuration_failed(line);
                } else {
                    self.fault(line);
                }
                continue;
            }
            if let Err(error) = self.settings.receive(&line) {
                self.configuration_failed(error.to_string());
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
                    self.sts.clear_position_history();
                    self.servo_feedback.clear();
                    self.bonus.reset_reference();
                    self.machine.invalidate_origins();
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
                    self.manual_input.set_machine_axis(index, 0.0);
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
            // 読取り専用の診断。FDCAN1/2の状態を同じ周期でGUIへ反映する。
            self.send("CANSTAT")?;
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
                    self.configuration_failed(error.to_string());
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
            } else if self.sequence.is_none() {
                let input = self.authority.input().unwrap_or(&self.manual_input);
                let slow = self.adjustment
                    || (!self.authority.active() && self.screen_control)
                    || input.buttons[9] != 0;
                let lines = self.machine.ramped_jog_lines(
                    input,
                    self.telemetry.as_ref().unwrap(),
                    slow,
                    now,
                );
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
        if self.preparation == PreparationPhase::Waiting && self.ready().is_err() {
            self.fault("開始待ち中に機体状態を失いました。準備を再確認してください".into());
        }
        self.tick_sequence(now)?;
        self.tick_test(now)?;
        if let Err(error) = self.tick_homing(now) {
            self.fault(error.to_string());
        }
        if let Err(error) = self.tick_ee(now) {
            self.fault_ee(error.to_string());
        }
        if let Err(error) = self.tick_bonus(now) {
            self.bonus.cancel();
            self.fault(error.to_string());
        }
        if let Err(error) = self.tick_sts(now) {
            self.fail_sts(error.to_string());
        }
        self.poll_servo_health(now)?;
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

    fn poll_servo_health(&mut self, now: Instant) -> Result<()> {
        let Some(board) = self.cfg.machine.serial_svmd.as_ref() else {
            return Ok(());
        };
        if board.servos.is_empty()
            || !self.fresh()
            || self.sts.busy()
            || self.sts.active
            || self.test.active
            || !self.ee.targets.is_empty()
            || !self
                .telemetry
                .as_ref()
                .is_some_and(|telemetry| telemetry.buses & 2 != 0)
            || !self
                .test
                .peers
                .get("sts")
                .is_some_and(|seen| now.saturating_duration_since(*seen) < Duration::from_secs(2))
            || self.last_servo_health_poll.is_some_and(|last| {
                now.saturating_duration_since(last) < Duration::from_millis(250)
            })
        {
            return Ok(());
        }
        let id = board.servos[self.servo_health_poll_index % board.servos.len()].id;
        self.servo_health_poll_index = (self.servo_health_poll_index + 1) % board.servos.len();
        self.last_servo_health_poll = Some(now);
        self.send(&crate::protocol::serial_svmd::Command::Read { id }.to_cctl_line())
    }

    fn can_bus_statuses(&self) -> Vec<CanBusStatus> {
        let telemetry = self.telemetry.as_ref();
        (1..=2)
            .map(|bus| {
                let available = self.fresh()
                    && telemetry.is_some_and(|state| state.buses & (1 << (bus - 1)) != 0);
                let diagnostic = self.can_diagnostics.get(&bus);
                let age_ms = diagnostic
                    .map(|(_, seen)| seen.elapsed().as_millis().min(u128::from(u64::MAX)) as u64);
                let health = if !self.fresh() {
                    CommunicationHealth::Unknown
                } else if diagnostic.is_some_and(|(value, _)| !value.started || value.bus_off)
                    || !available
                {
                    CommunicationHealth::Fault
                } else if diagnostic.is_some_and(|(value, _)| value.tec >= 128 || value.rec >= 96) {
                    CommunicationHealth::Warning
                } else {
                    CommunicationHealth::Healthy
                };
                let detail = if !self.fresh() {
                    "cctlの状態応答待ち".into()
                } else if let Some((value, _)) = diagnostic {
                    if !value.started {
                        "CANコントローラ未起動".into()
                    } else if value.bus_off {
                        "bus-off：配線・終端・通信速度・ID重複を確認".into()
                    } else if !available {
                        "cctlが使用不可と判定".into()
                    } else {
                        "通信可能".into()
                    }
                } else if available {
                    "通信可能・詳細応答待ち".into()
                } else {
                    "使用不可".into()
                };
                CanBusStatus {
                    bus,
                    label: if bus == 1 {
                        "FDCAN1（モータ）".into()
                    } else {
                        "FDCAN2（周辺基板）".into()
                    },
                    health,
                    detail,
                    age_ms,
                    started: diagnostic.map(|(value, _)| value.started),
                    bus_off: diagnostic.map(|(value, _)| value.bus_off),
                    lec: diagnostic.map(|(value, _)| value.lec),
                    tec: diagnostic.map(|(value, _)| value.tec),
                    rec: diagnostic.map(|(value, _)| value.rec),
                    cel: diagnostic.map(|(value, _)| value.cel),
                    tx_failed: diagnostic.map(|(value, _)| value.tx_failed),
                }
            })
            .collect()
    }

    fn can_device_statuses(&self) -> Vec<CanDeviceStatus> {
        let mut devices = Vec::new();
        let telemetry = self.telemetry.as_ref();
        let motor_bus = self.fresh()
            && telemetry.is_some_and(|value| value.buses & 1 != 0)
            && !self
                .can_diagnostics
                .get(&1)
                .is_some_and(|(value, _)| !value.started || value.bus_off);
        let telemetry_age = self
            .rx
            .map(|seen| seen.elapsed().as_millis().min(u128::from(u64::MAX)) as u64);
        let scan = self
            .c620_scan
            .filter(|(_, seen)| seen.elapsed() < Duration::from_secs(2));
        for axis in &self.cfg.machine.axes {
            let (kind, id) = match axis.slot {
                0 => (
                    "EL05",
                    self.cfg.machine.parameters.get("el05_motor_id").copied(),
                ),
                1 => (
                    "M3508 + C620",
                    self.cfg.machine.parameters.get("c620_esc_id").copied(),
                ),
                2 => (
                    "M3508 + C620",
                    self.cfg
                        .machine
                        .parameters
                        .get("c620_slot2_esc_id")
                        .copied(),
                ),
                _ => ("モータ", None),
            };
            let address = id.map_or_else(
                || format!("slot {}", axis.slot),
                |id| format!("slot {} / ID {}", axis.slot, id as u8),
            );
            let (health, detail) = if !self.fresh() {
                (CommunicationHealth::Unknown, "cctlの状態応答待ち".into())
            } else if !motor_bus {
                (CommunicationHealth::Fault, "FDCAN1が使用不可".into())
            } else if let Some(state) = telemetry.and_then(|t| t.slots.get(axis.slot as usize)) {
                let stale = telemetry.is_some_and(|t| t.stale_slots & (1 << axis.slot) != 0);
                let error = telemetry.map_or(0, |t| t.error_bits[axis.slot as usize]);
                let missing_c620 = axis.slot > 0
                    && id.is_some_and(|id| {
                        scan.is_some_and(|(mask, _)| mask & (1 << (id as u8 - 1)) == 0)
                    });
                if stale {
                    (
                        CommunicationHealth::Fault,
                        format!("フィードバック途絶 / error=0x{error:02X}"),
                    )
                } else if error != 0 {
                    (
                        CommunicationHealth::Fault,
                        format!("モータ異常 / error=0x{error:02X}"),
                    )
                } else if missing_c620 {
                    (CommunicationHealth::Fault, "設定IDへのC620応答なし".into())
                } else {
                    (
                        CommunicationHealth::Healthy,
                        format!("フィードバック正常 / 実測 {:.3}", state.measured),
                    )
                }
            } else {
                (CommunicationHealth::Unknown, "slot状態なし".into())
            };
            devices.push(CanDeviceStatus {
                name: format!("{}軸 · {kind}", axis.name),
                bus: 1,
                address,
                health,
                detail,
                age_ms: telemetry_age,
            });
        }

        let peripheral_bus = self.fresh()
            && telemetry.is_some_and(|value| value.buses & 2 != 0)
            && !self
                .can_diagnostics
                .get(&2)
                .is_some_and(|(value, _)| !value.started || value.bus_off);
        let pwm_configured =
            !self.cfg.machine.pwm_servos.is_empty() || !self.cfg.machine.svmd_parameters.is_empty();
        if pwm_configured || self.test.peers.contains_key("pwm") {
            devices.push(self.peripheral_board_status(
                "PWMサーボ基板",
                "pwm",
                "応答 0x301",
                peripheral_bus,
                pwm_configured,
            ));
        }
        if pwm_configured {
            for servo in &self.cfg.machine.pwm_servos {
                let board = self.peripheral_health("pwm", peripheral_bus);
                devices.push(CanDeviceStatus {
                    name: format!("{} · PWMサーボ", servo.name),
                    bus: 2,
                    address: format!("channel {}", servo.channel),
                    health: if board == CommunicationHealth::Healthy {
                        CommunicationHealth::Warning
                    } else {
                        board
                    },
                    detail: if board == CommunicationHealth::Healthy {
                        "基板応答あり・サーボ個体の応答は取得不可".into()
                    } else {
                        "PWMサーボ基板の応答なし".into()
                    },
                    age_ms: self.peer_age("pwm"),
                });
            }
        }
        let dc_configured =
            !self.cfg.machine.dc_motors.is_empty() || !self.cfg.machine.dcmd_parameters.is_empty();
        if dc_configured || self.test.peers.contains_key("dc") {
            devices.push(self.peripheral_board_status(
                "DCモータ基板",
                "dc",
                "応答 0x311",
                peripheral_bus,
                dc_configured,
            ));
        }
        if dc_configured {
            for motor in &self.cfg.machine.dc_motors {
                let board = self.peripheral_health("dc", peripheral_bus);
                devices.push(CanDeviceStatus {
                    name: format!("{} · DCモータ", motor.name),
                    bus: 2,
                    address: format!("channel {}", motor.channel),
                    health: if board == CommunicationHealth::Healthy {
                        CommunicationHealth::Warning
                    } else {
                        board
                    },
                    detail: if board == CommunicationHealth::Healthy {
                        "基板応答あり・モータ個体の応答は取得不可".into()
                    } else {
                        "DCモータ基板の応答なし".into()
                    },
                    age_ms: self.peer_age("dc"),
                });
            }
        }
        let sts_configured = self.cfg.machine.serial_svmd.is_some()
            || !self.cfg.machine.serial_svmd_parameters.is_empty();
        if sts_configured || self.test.peers.contains_key("sts") {
            devices.push(self.peripheral_board_status(
                "STS3215基板",
                "sts",
                "応答 0x321",
                peripheral_bus,
                sts_configured,
            ));
        }
        if let Some(board) = &self.cfg.machine.serial_svmd {
            let board_health = self.peripheral_health("sts", peripheral_bus);
            for servo in &board.servos {
                let feedback = self.servo_feedback.get(&servo.id);
                let fresh =
                    feedback.is_some_and(|value| value.seen.elapsed() < Duration::from_secs(2));
                let (health, detail, age_ms) = if board_health != CommunicationHealth::Healthy {
                    (board_health, "STS3215基板の応答なし".into(), None)
                } else if let Some(feedback) = feedback.filter(|_| fresh) {
                    (
                        if feedback.error == 0 {
                            CommunicationHealth::Healthy
                        } else {
                            CommunicationHealth::Fault
                        },
                        feedback.detail.clone(),
                        Some(feedback.seen.elapsed().as_millis() as u64),
                    )
                } else {
                    (
                        CommunicationHealth::Fault,
                        "サーボ個体から応答なし".into(),
                        feedback.map(|value| value.seen.elapsed().as_millis() as u64),
                    )
                };
                devices.push(CanDeviceStatus {
                    name: format!("{} · STS3215", servo.name),
                    bus: 2,
                    address: format!("servo ID {}", servo.id),
                    health,
                    detail,
                    age_ms,
                });
            }
        }
        devices
    }

    fn peer_age(&self, key: &str) -> Option<u64> {
        self.test
            .peers
            .get(key)
            .map(|seen| seen.elapsed().as_millis().min(u128::from(u64::MAX)) as u64)
    }

    fn peripheral_health(&self, key: &str, bus_available: bool) -> CommunicationHealth {
        if !self.fresh() {
            CommunicationHealth::Unknown
        } else if !bus_available {
            CommunicationHealth::Fault
        } else if self.peer_age(key).is_some_and(|age| age < 2000) {
            CommunicationHealth::Healthy
        } else if self.setup {
            CommunicationHealth::Fault
        } else {
            CommunicationHealth::Unknown
        }
    }

    fn peripheral_board_status(
        &self,
        name: &str,
        key: &str,
        address: &str,
        bus_available: bool,
        configured: bool,
    ) -> CanDeviceStatus {
        let age_ms = self.peer_age(key);
        let health = if configured {
            self.peripheral_health(key, bus_available)
        } else if !self.fresh() {
            CommunicationHealth::Unknown
        } else if !bus_available {
            CommunicationHealth::Fault
        } else if age_ms.is_some_and(|age| age < 2000) {
            CommunicationHealth::Warning
        } else {
            CommunicationHealth::Unknown
        };
        CanDeviceStatus {
            name: name.into(),
            bus: 2,
            address: address.into(),
            health,
            detail: match (configured, health) {
                (false, CommunicationHealth::Warning) => "状態応答あり・機体設定なし".into(),
                (false, CommunicationHealth::Fault) => "検出済み・FDCAN2が使用不可".into(),
                (false, _) => "以前検出・機体設定なし".into(),
                (true, CommunicationHealth::Healthy) => "状態応答を受信".into(),
                (true, CommunicationHealth::Fault) if !bus_available => "FDCAN2が使用不可".into(),
                (true, CommunicationHealth::Fault) => "状態応答が2秒以上ありません".into(),
                (true, _) => "応答待ち".into(),
            },
            age_ms,
        }
    }

    fn publish(&self) {
        let origins = self.machine.origin_states(self.telemetry.as_ref());
        let can_buses = self.can_bus_statuses();
        let can_devices = self.can_device_statuses();
        self.shared.update_status(|s| {
            s.test_mode = self.test.enabled;
            s.ee_targets = self.ee.targets.clone();
            self.publish_bonus(&mut s.bonus);
            s.homing = self.homing.as_ref().map(|h| h.label.clone());
            s.homing_ready = self.court.is_some()
                && !self.preparation.locked()
                && self.homing_idle()
                && !self.authority.active()
                && self.axes_ready(false).is_ok();
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
            s.sts.teach_id = self.sts.teach_id;
            s.sts.elapsed_ms = self.sts.elapsed_ms();
            s.sts.active = self.sts.active;
            s.sts.busy = self.sts.busy();
            s.court = self.court;
            s.preparation = self.preparation;
            s.pad_guide = self.guide.enabled;
            s.preparation_step = self.preparation_step();
            s.guide_release = self.guide.confirmed;
            s.operation_sound_available =
                self.fresh() && self.device.as_ref().is_some_and(|d| d.tone);
            s.preparation_blocker = self
                .preparation_ready()
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            s.emergency = self.emergency;
            s.outputs_active = self.homing.is_some()
                || !self.ee.targets.is_empty()
                || self.bonus.outputs_active()
                || self.sts.active
                || self.test.active
                || self.drive.awaiting().is_some()
                || self.telemetry.as_ref().is_some_and(|t| {
                    (t.mode == RunMode::Run && t.enabled_slots != 0)
                        || t.held_slots.unwrap_or(0) != 0
                });
            s.operating_state = if self.emergency {
                "ソフト緊停中"
            } else if !self.fresh() {
                "接続断 / 状態不明"
            } else if self.homing.is_some() {
                "r・zホーミング中"
            } else if self.sequence.is_some() {
                "シーケンス実行中"
            } else if self.bonus.active() {
                "ボーナスハンド動作中"
            } else if self.test.active {
                "個別テスト出力中"
            } else if self.drive.running() {
                "運転中"
            } else if self.drive.awaiting().is_some() {
                "運転応答待ち"
            } else if self
                .telemetry
                .as_ref()
                .is_some_and(|t| t.held_slots.unwrap_or(0) != 0)
            {
                "出力停止・z保持"
            } else if s.outputs_active {
                "停止・保持"
            } else {
                "全トルク解除"
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
            s.can_buses = can_buses;
            s.can_devices = can_devices;
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

mod bonus_control;
mod ee_control;
mod homing;
mod pad_control;
mod preparation;
mod requests;
mod sequence_control;
mod sts_control;
mod test_control;
#[cfg(test)]
mod tests;

mod pad_guide;

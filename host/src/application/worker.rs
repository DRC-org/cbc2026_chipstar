//! 機体接続と操作権を単一ワーカーが所有する。
use crate::{
    application::app_state::{BridgeConfig, Shared},
    application::settings::Settings,
    input::controller::{self, ControllerState},
    interface::control_api::{Reply, Request},
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
const AI_INPUT_TTL: Duration = Duration::from_millis(150);
const AI_LEASE: Duration = Duration::from_secs(30);

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
    running: bool,
    awaiting_run: Option<Instant>,
    ai: Option<String>,
    ai_contact: Instant,
    ai_input: ControllerState,
    ai_input_time: [Option<Instant>; 6],
    manual_input: ControllerState,
    gamepad_name: String,
    adjustment: bool,
    last_hello: Instant,
    reason: String,
    error: String,
}
impl Runtime {
    fn new(shared: Arc<Shared>) -> Self {
        let cfg = shared.config();
        Self {
            link: Link::new(&cfg.serial_device, cfg.baud_rate, cfg.simulate),
            machine: MachineController::new(cfg.machine.clone()),
            settings: Settings::new(&cfg.machine),
            cfg,
            shared,
            device: None,
            telemetry: None,
            rx: None,
            setup: false,
            setup_error: false,
            running: false,
            awaiting_run: None,
            ai: None,
            ai_contact: Instant::now(),
            ai_input: ControllerState::default(),
            ai_input_time: [None; 6],
            manual_input: ControllerState::default(),
            gamepad_name: String::new(),
            adjustment: false,
            last_hello: Instant::now() - Duration::from_secs(2),
            reason: "接続待ち".into(),
            error: String::new(),
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
        self.running = false;
        self.awaiting_run = None;
        self.ai_input = ControllerState::default();
        self.ai_input_time = [None; 6];
    }
    fn stop(&mut self, cut: bool) -> Result<()> {
        self.clear_drive();
        if !self.fresh() {
            let _ = self.send("STOP");
            let _ = self.stop_peripherals();
        } else if cut {
            self.send("STOP")?;
            self.stop_peripherals()?;
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
        if !self.cfg.machine.pwm_servos.is_empty() {
            self.send(&crate::protocol::svmd::Command::Stop.to_cctl_line())?;
        }
        if !self.cfg.machine.dc_motors.is_empty() {
            self.send(&crate::protocol::dcmd::line(3, 0, 0))?;
        }
        if self.cfg.machine.requires_serial_svmd() {
            self.send(&crate::protocol::serial_svmd::Command::Stop.to_cctl_line())?;
        }
        Ok(())
    }
    fn fault(&mut self, reason: String) {
        let _ = self.stop(true);
        self.machine.invalidate_origins();
        self.reason = reason.clone();
        self.error = reason;
    }
    fn ready(&self) -> Result<()> {
        if self.cfg.machine.axes.is_empty() {
            bail!("操縦するcctlの軸を指定してください");
        }
        if !self.fresh() {
            bail!("機体の応答がありません");
        }
        if self.cfg.machine.parameters.len() != crate::machine::PARAMETER_NAMES.len() {
            bail!("cctlの全33項目をPCの機体設定で指定してください");
        }
        if self.setup_error || !self.setup || !self.settings.ready() {
            bail!("設定の反映が確認できていません");
        }
        let Some(device) = &self.device else {
            bail!("能力確認待ち");
        };
        if device.board != "cctl" || device.protocol != 1 || device.slots < 3 || !device.jog {
            bail!("JOG対応のcctl FWが必要です");
        }
        let t = self.telemetry.as_ref().unwrap();
        if t.stale_slots != 0
            || t.buses & 1 == 0
            || t.error_bits
                .iter()
                .enumerate()
                .any(|(i, &e)| if i == 2 { e & 0xf0 != 0 } else { e != 0 })
        {
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
            bail!("原点を採用するか、調整画面で原点調整モードを選んでください");
        }
        Ok(())
    }
    fn start(&mut self) -> Result<()> {
        self.ready()?;
        let input = if self.ai.is_some() {
            &self.ai_input
        } else {
            &self.manual_input
        };
        if input.axes.iter().any(|v| !v.is_finite() || v.abs() >= 0.1) {
            bail!("スティックを中立に戻してください");
        }
        if self.ai.is_none() && self.gamepad_name.is_empty() && !self.cfg.simulate {
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
        self.awaiting_run = Some(Instant::now());
        self.error.clear();
        self.reason = "基板の運転応答待ち".into();
        Ok(())
    }
    fn authorize(&mut self, request: &Request, manual: bool) -> Result<()> {
        if request.action == "stop" {
            return Ok(());
        }
        if manual {
            if self.ai.is_some() {
                bail!("AI操作中です。停止は常に操作できます");
            }
        } else {
            if self.ai.is_none() || self.ai != request.token {
                bail!("claimで操作権を取得し、tokenを指定してください");
            }
            self.ai_contact = Instant::now();
        }
        Ok(())
    }
    fn request(&mut self, req: &Request, manual: bool) -> Result<Reply> {
        if req.action == "claim" {
            if manual || self.ai.is_some() {
                bail!("操作権は使用中です");
            }
            self.stop(false)?;
            let token = format!(
                "{:x}-{:x}",
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
            );
            self.ai = Some(token.clone());
            self.ai_contact = Instant::now();
            return Ok(Reply {
                token: Some(token),
                ..Reply::data(String::new())
            });
        }
        self.authorize(req, manual)?;
        match req.action.as_str() {
            "heartbeat" => {}
            "release" => {
                self.stop(false)?;
                self.ai = None;
            }
            "run" => self.start()?,
            "stop" => self.stop(false)?,
            "cut" => {
                self.stop(true)?;
                self.machine.invalidate_origins();
            }
            "safe" => {
                self.stop(true)?;
                self.send("SAFE")?;
            }
            "origin" => {
                if self.running || self.awaiting_run.is_some() || !self.fresh() {
                    bail!("停止して最新の実測位置を確認してください");
                }
                let index = self
                    .cfg
                    .machine
                    .axes
                    .iter()
                    .position(|a| Some(&a.name) == req.axis.as_ref())
                    .context("軸名が不正です")?;
                if !self.machine.capture_origin(index, self.telemetry.as_ref()) {
                    bail!("原点を採用できません");
                }
                return Ok(Reply::data("hostの原点に採用しました".into()));
            }
            "adjustment" => {
                if self.running || self.awaiting_run.is_some() {
                    bail!("停止してから切り替えてください");
                }
                self.adjustment = req.flag.unwrap_or(false);
                self.machine.set_soft_limits(!self.adjustment);
            }
            "input" => {
                let value = req.value.context("valueが必要です")?;
                if !value.is_finite() || value.abs() > 1.0 {
                    bail!("入力は-1..1で指定してください");
                }
                let axis = self
                    .cfg
                    .machine
                    .axes
                    .iter()
                    .find(|a| Some(&a.name) == req.axis.as_ref())
                    .context("軸名が不正です")?;
                let index = axis.input_axis.context("入力が割り当てられていません")?;
                if manual {
                    if !self.cfg.simulate {
                        bail!("画面からの模擬入力は模擬接続専用です");
                    }
                    self.manual_input.axes[index] = value;
                } else {
                    self.ai_input.axes[index] = value;
                    self.ai_input_time[index] = Some(Instant::now());
                }
            }
            "connection" => {
                if self.running || self.awaiting_run.is_some() {
                    bail!("停止してから接続先を変更してください");
                }
                let connection: crate::application::app_state::Connection =
                    toml::from_str(req.text.as_deref().context("接続設定が必要です")?)?;
                if connection.serial_device.is_empty() || connection.baud_rate == 0 {
                    bail!("接続設定が不正です");
                }
                self.stop(true)?;
                self.cfg.serial_device = connection.serial_device;
                self.cfg.baud_rate = connection.baud_rate;
                self.shared.set_config(self.cfg.clone());
                self.link = Link::new(
                    &self.cfg.serial_device,
                    self.cfg.baud_rate,
                    self.cfg.simulate,
                );
                self.device = None;
                self.telemetry = None;
                self.rx = None;
                self.setup = false;
                self.setup_error = false;
                self.settings = Settings::new(&self.cfg.machine);
                self.machine.invalidate_origins();
                self.error.clear();
                self.last_hello = Instant::now() - Duration::from_secs(2);
            }
            "reinit" => {
                if self.running || self.awaiting_run.is_some() || !self.fresh() {
                    bail!("停止して接続を確認してください");
                }
                let axis = self
                    .cfg
                    .machine
                    .axes
                    .iter()
                    .find(|a| Some(&a.name) == req.axis.as_ref())
                    .context("軸名が不正です")?;
                let mask = 1 << axis.slot;
                self.stop(true)?;
                self.send("SAFE")?;
                self.send(&format!("REINIT {mask}"))?;
                self.machine.invalidate_origins();
            }
            "apply" => {
                if self.running || self.awaiting_run.is_some() {
                    bail!("停止してから設定を適用してください");
                }
                let profile =
                    MachineProfile::parse(req.text.as_deref().context("設定本文が必要です")?)?;
                self.stop(true)?;
                if self.fresh() {
                    self.send("SAFE")?;
                }
                self.cfg.machine = profile;
                self.shared.set_config(self.cfg.clone());
                self.machine = MachineController::new(self.cfg.machine.clone());
                self.machine.set_soft_limits(!self.adjustment);
                self.settings = Settings::new(&self.cfg.machine);
                self.setup = self.fresh();
                self.setup_error = false;
                self.error.clear();
                self.shared.update_status(|s| s.saved = false);
            }
            "save" => {
                if self.running || self.awaiting_run.is_some() {
                    bail!("停止してから保存してください");
                }
                let text = toml::to_string_pretty(&self.cfg.machine)?;
                let path = &self.cfg.profile_path;
                let tmp = path.with_extension(format!("toml.{}.new", std::process::id()));
                use std::io::Write;
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&tmp)?;
                file.write_all(text.as_bytes())?;
                file.sync_all()?;
                drop(file);
                std::fs::rename(&tmp, path)?;
                self.shared.update_status(|s| s.saved = true);
                return Ok(Reply::data(format!("保存しました: {}", path.display())));
            }
            "fault" => self.link.fault(req.text.as_deref().unwrap_or(""))?,
            _ => bail!("未対応の操作です"),
        }
        Ok(Reply::accepted())
    }
    fn receive(&mut self) {
        for line in self.link.read_lines() {
            self.shared.log(format!("RX {line}"));
            if let Some(status) = crate::protocol::dcmd::parse_status(&line) {
                self.shared.update_status(|s| {
                    s.peripherals.insert(
                        "DCMD".into(),
                        format!(
                            "mode={} enabled={} duty={} result={}",
                            status.mode, status.enabled, status.duty[0], status.result
                        ),
                    );
                });
            }
            if let Some(encoder) = crate::protocol::dcmd::parse_encoder(&line) {
                self.shared.update_status(|s| {
                    s.peripherals.insert(
                        "ボーナスENC".into(),
                        format!("count={} index={}", encoder.count, encoder.index_count),
                    );
                });
            }
            if let Some(servo) = crate::protocol::serial_svmd::parse_state(&line) {
                self.shared.update_status(|s| {
                    s.peripherals.insert(
                        format!("STS3215 ID {}", servo.id),
                        format!(
                            "position={} enabled={} error={}",
                            servo.position, servo.enabled, servo.error
                        ),
                    );
                });
            }
            if line.starts_with("ERR ") {
                self.setup_error = true;
                self.fault(line);
                continue;
            }
            if let Err(error) = self.settings.receive(&line) {
                self.setup_error = true;
                self.fault(error.to_string());
            }
            if let Some(device) = parse_device_info(&line) {
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
                if self.running
                    && (t.mode != RunMode::Run || t.enabled_slots & mask != mask || origin_lost)
                {
                    self.fault("運転状態または原点を失いました".into());
                }
                if self.awaiting_run.is_some() && t.mode == RunMode::Run {
                    let mask = self
                        .cfg
                        .machine
                        .axes
                        .iter()
                        .fold(0u8, |m, a| m | 1 << a.slot);
                    if t.enabled_slots & mask == mask {
                        self.running = true;
                        self.awaiting_run = None;
                        self.reason = "運転中".into();
                    }
                }
                self.telemetry = Some(t);
            }
        }
    }
    fn tick(&mut self) -> Result<()> {
        self.receive();
        if self.ai.is_some() && self.ai_contact.elapsed() > AI_LEASE {
            self.stop(false)?;
            self.ai = None;
            self.reason = "AI操作権の期限切れ。通常操縦は再開待ち".into();
        }
        for (index, stamp) in self.ai_input_time.iter_mut().enumerate() {
            if stamp.is_some_and(|t| t.elapsed() > AI_INPUT_TTL) {
                self.ai_input.axes[index] = 0.0;
                *stamp = None;
            }
        }
        if !self.fresh() && self.rx.is_some() {
            self.fault("機体応答の期限切れ。原点を確認して再開してください".into());
            self.rx = None;
            self.device = None;
            self.telemetry = None;
            self.setup = false;
            self.setup_error = false;
            self.settings = Settings::new(&self.cfg.machine);
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
        if self.fresh() && self.device.is_some() && !self.setup {
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
            match self.settings.next() {
                Ok(Some(line)) => self.send(&line)?,
                Ok(None) => {}
                Err(error) => {
                    self.setup_error = true;
                    self.fault(error.to_string());
                }
            }
        }
        if self
            .awaiting_run
            .is_some_and(|t| t.elapsed() > Duration::from_millis(500))
        {
            self.fault("RUNの反映応答がありません".into());
        }
        if self.running {
            if let Err(error) = self.ready() {
                self.fault(error.to_string());
            } else {
                let input = if self.ai.is_some() {
                    &self.ai_input
                } else {
                    &self.manual_input
                };
                let slow = self.adjustment || input.buttons[9] != 0;
                let lines = self
                    .machine
                    .jog_lines(input, self.telemetry.as_ref().unwrap(), slow);
                for line in lines {
                    self.send(&line)?;
                }
            }
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
            s.connected = self.fresh();
            s.configured = self.setup && self.settings.ready() && !self.setup_error;
            s.ai_active = self.ai.is_some();
            s.running = self.running;
            s.origin_adjustment = self.adjustment;
            s.slow = self.adjustment || self.manual_input.buttons[9] != 0;
            s.axes = if self.ai.is_some() {
                self.ai_input.axes
            } else {
                self.manual_input.axes
            };
            s.origins = origins;
            s.gamepad = self.gamepad_name.clone();
            s.board_mode = self
                .telemetry
                .as_ref()
                .map(|t| t.mode.label().to_owned())
                .unwrap_or_default();
            s.reason = if !self.running && self.awaiting_run.is_none() {
                self.ready()
                    .err()
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| self.reason.clone())
            } else {
                self.reason.clone()
            };
            s.error = self.error.clone();
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
    let mut previous_buttons = [0u8; 17];
    while shared.is_running() {
        let cycle = Instant::now();
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
                    runtime.manual_input = controller::read(&pad);
                    let buttons = runtime.manual_input.buttons;
                    if buttons[5] != 0 && previous_buttons[5] == 0 {
                        let _ = runtime.stop(false);
                    }
                    if runtime.ai.is_none()
                        && buttons[6] != 0
                        && previous_buttons[6] == 0
                        && let Err(error) = runtime.start()
                    {
                        runtime.error = error.to_string();
                    }
                    previous_buttons = buttons;
                } else {
                    if runtime.ai.is_none() && runtime.running {
                        runtime.fault("DualSenseが切断されました".into());
                    }
                    runtime.gamepad_name.clear();
                    runtime.manual_input = ControllerState::default();
                    selected = None;
                }
            }
        }
        for pending in shared.take_requests() {
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
            runtime.clear_drive();
            runtime.machine.invalidate_origins();
            runtime.error = error.to_string();
            runtime.rx = None;
            runtime.device = None;
            runtime.telemetry = None;
            runtime.setup = false;
            runtime.setup_error = false;
            runtime.settings = Settings::new(&runtime.cfg.machine);
            runtime.reason = "通信失敗。再接続と原点確認が必要です".into();
        }
        runtime.publish();
        let period = Duration::from_secs_f64(1.0 / runtime.cfg.rate_hz.clamp(20.0, 100.0));
        thread::sleep(period.saturating_sub(cycle.elapsed()));
    }
    let _ = runtime.stop(true);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restarting_a_subset_does_not_enable_axes_from_an_older_profile() {
        let mut profile = MachineProfile::load(None).unwrap();
        profile.axes.truncate(1);
        let shared = Arc::new(Shared::new(BridgeConfig {
            serial_device: "unused".into(),
            baud_rate: 115200,
            rate_hz: 20.0,
            machine: profile,
            profile_path: "/dev/null".into(),
            simulate: true,
        }));
        let mut runtime = Runtime::new(shared);
        for _ in 0..40 {
            runtime.tick().unwrap();
        }
        runtime
            .machine
            .capture_origin(0, runtime.telemetry.as_ref());
        runtime.send("ENABLE 7 1").unwrap();
        runtime.start().unwrap();
        runtime.tick().unwrap();
        assert_eq!(runtime.telemetry.as_ref().unwrap().enabled_slots, 1);
    }
    #[test]
    fn ai_control_excludes_manual_changes_and_expires_without_resuming() {
        let shared = Arc::new(Shared::new(BridgeConfig {
            serial_device: "unused".into(),
            baud_rate: 115200,
            rate_hz: 20.0,
            machine: MachineProfile::load(None).unwrap(),
            profile_path: "/dev/null".into(),
            simulate: true,
        }));
        let mut runtime = Runtime::new(shared);
        let token = runtime
            .request(&Request::new("claim"), false)
            .unwrap()
            .token
            .unwrap();
        assert!(
            runtime
                .request(
                    &Request {
                        flag: Some(true),
                        ..Request::new("adjustment")
                    },
                    true
                )
                .is_err()
        );
        assert!(runtime.request(&Request::new("stop"), true).is_ok());
        runtime
            .request(
                &Request {
                    token: Some(token),
                    axis: Some("r".into()),
                    value: Some(1.0),
                    ..Request::new("input")
                },
                false,
            )
            .unwrap();
        runtime.ai_input_time[1] = Some(Instant::now() - Duration::from_secs(1));
        runtime.ai_input_time[0] = Some(Instant::now());
        runtime.ai_input.axes[0] = 0.2;
        runtime.tick().unwrap();
        assert_eq!(runtime.ai_input.axes[1], 0.0);
        assert_eq!(runtime.ai_input.axes[0], 0.2);
        assert!(runtime.ai.is_some());
        runtime.ai_contact = Instant::now() - AI_LEASE - Duration::from_secs(1);
        runtime.tick().unwrap();
        assert!(runtime.ai.is_none());
        assert!(!runtime.running);
        assert!(runtime.awaiting_run.is_none());
    }
}

use super::*;

impl Runtime {
    pub(super) fn request(&mut self, req: &Request, manual: bool) -> Result<Reply> {
        if req.action == "estop" {
            self.engage_emergency()?;
            return Ok(Reply::accepted());
        }
        if req.action == "estop_reset" {
            if !manual {
                bail!("緊停解除は人間がGUIから行ってください");
            }
            if !self.emergency {
                bail!("ソフト緊停は解除済みです");
            }
            self.stop(true)?;
            self.emergency = false;
            return Ok(Reply::data(
                "緊停を解除しました。出力停止を維持しています".into(),
            ));
        }
        if self.emergency
            && !matches!(
                req.action.as_str(),
                "stop" | "cut" | "safe" | "fault" | "connection"
            )
        {
            bail!("ソフト緊停中です。人間が解除するまで操作できません");
        }
        if req.action == "claim" {
            if manual || self.authority.active() || self.test.enabled {
                bail!("操作権は使用中です");
            }
            self.stop(false)?;
            let token = format!(
                "{:x}-{:x}",
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
            );
            self.authority.claim(token.clone(), Instant::now());
            return Ok(Reply {
                token: Some(token),
                ..Reply::data(String::new())
            });
        }
        if req.action == "takeover" {
            if !manual {
                bail!("通常操縦への切替はGUIから操作してください");
            }
            self.stop(false)?;
            self.authority.release();
            return Ok(Reply::data(
                "通常操縦へ戻しました。運転再開を待っています".into(),
            ));
        }
        self.authority.authorize(
            req.token.as_deref(),
            manual,
            req.action == "stop",
            Instant::now(),
        )?;
        if req.action.starts_with("test_") {
            let result = self.test_request(req, manual);
            if result.is_err() && self.test.active {
                let _ = self.stop(true);
            }
            return result;
        }
        if self.test.active
            && matches!(
                req.action.as_str(),
                "apply"
                    | "save"
                    | "connection"
                    | "reinit"
                    | "origin"
                    | "adjustment"
                    | "manual_control"
            )
        {
            bail!("個別テストの出力を停止してから操作してください");
        }
        match req.action.as_str() {
            "heartbeat" => {}
            "manual_control" => {
                if self.drive.running() || self.drive.awaiting().is_some() {
                    bail!("停止してから操作方法を変更してください");
                }
                let screen_control = req.flag.context("操作方法が必要です")?;
                self.stop(false)?;
                self.screen_control = screen_control;
                self.manual_input = ControllerState::default();
                self.screen_input_times = [None; 6];
            }
            "release" => {
                self.stop(false)?;
                self.authority.release();
            }
            "run" => self.start()?,
            "stop" => self.stop(false)?,
            "cut" => self.stop(true)?,
            "safe" => {
                self.stop(true)?;
                self.send("SAFE")?;
            }
            "origin" => {
                if self.drive.running() || self.drive.awaiting().is_some() || !self.fresh() {
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
                if self.drive.running() || self.drive.awaiting().is_some() {
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
                    if !self.screen_control {
                        bail!("画面操作を選択してください");
                    }
                    if value != 0.0 && !self.drive.running() {
                        bail!("運転再開してから操作してください");
                    }
                    self.manual_input.axes[index] = value;
                    self.screen_input_times[index] = Some(Instant::now());
                } else {
                    self.authority.set_input(index, value, Instant::now());
                }
            }
            "connection" => {
                if self.drive.running() || self.drive.awaiting().is_some() {
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
                if let Some(simulate) = connection.simulate {
                    self.cfg.simulate = simulate;
                }
                self.test = test_control::TestControl::default();
                self.screen_control = self.cfg.simulate;
                self.manual_input = ControllerState::default();
                self.screen_input_times = [None; 6];
                self.shared
                    .update_status(|status| status.peripherals.clear());
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
                if self.drive.running() || self.drive.awaiting().is_some() || !self.fresh() {
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
                if self.drive.running() || self.drive.awaiting().is_some() {
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
                if self.drive.running() || self.drive.awaiting().is_some() {
                    bail!("停止してから保存してください");
                }
                let path = req
                    .text
                    .as_ref()
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| self.cfg.profile_path.clone());
                crate::transport::profile_store::save(&path, &self.cfg.machine)?;
                self.cfg.profile_path = path.clone();
                self.shared.set_config(self.cfg.clone());
                self.shared.update_status(|s| s.saved = true);
                return Ok(Reply::data(format!("保存しました: {}", path.display())));
            }
            "fault" => self.link.fault(req.text.as_deref().unwrap_or(""))?,
            _ => bail!("未対応の操作です"),
        }
        Ok(Reply::accepted())
    }
}

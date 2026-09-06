use super::*;

impl Runtime {
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
    pub(super) fn request(&mut self, req: &Request, manual: bool) -> Result<Reply> {
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
}

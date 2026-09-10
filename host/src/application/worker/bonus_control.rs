use super::*;
use crate::machine::bonus::BonusProfile;

#[derive(Default)]
pub(super) struct Control {
    pub semi_auto: bool,
    pub handoff: Option<i32>,
    pub encoder: Option<(i32, Instant)>,
    pub contacts: Option<(u8, Instant)>,
    pub selected_box: usize,
    pub loaded: u8,
    phase: Phase,
    last_dc: Option<(i16, Instant)>,
    servo_started: Option<Instant>,
    manual_until: Option<Instant>,
    last_limit_poll: Option<Instant>,
    cycle_started: Option<Instant>,
    sts_holding: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::bonus::HandoffLimit;

    fn runtime() -> Runtime {
        let mut r = crate::application::worker::tests::screen_runtime();
        let embedded = MachineProfile::embedded().unwrap();
        r.cfg.machine.dc_motors = embedded.dc_motors;
        r.cfg.machine.serial_svmd = embedded.serial_svmd;
        r.cfg.machine.bonus = embedded.bonus;
        let profile = r.cfg.machine.bonus.as_mut().unwrap();
        profile.enabled = true;
        profile.boxes[0].offset_counts = 1000;
        profile.align_position = 2500;
        profile.align_home_position = 2000;
        profile.lid_open_position = 2600;
        profile.lid_closed_position = 2100;
        for servo in &mut r.cfg.machine.serial_svmd.as_mut().unwrap().servos {
            if servo.name.starts_with("bonus_") {
                servo.enabled = true;
            }
        }
        r.bonus.handoff = Some(100);
        r.bonus.observe_encoder(100, Instant::now());
        r
    }

    fn feedback(r: &mut Runtime, name: &str, position: i32, now: Instant) {
        let now = r
            .bonus
            .servo_started
            .map_or(now, |started| now.max(started + Duration::from_millis(1)));
        let id = r
            .cfg
            .machine
            .serial_svmd
            .as_ref()
            .unwrap()
            .servos
            .iter()
            .find(|servo| servo.name == name)
            .unwrap()
            .id;
        r.servo_feedback.insert(
            id,
            ServoFeedback {
                seen: now,
                position,
                error: 0,
                detail: String::new(),
            },
        );
    }

    #[test]
    fn semi_auto_runs_box_alignment_lid_and_return_sequence() {
        let mut r = runtime();
        let start = Instant::now();
        r.bonus.semi_auto = true;
        r.bonus_request(&Request {
            value: Some(6.0),
            ..Request::new("bonus_receive")
        })
        .unwrap();
        assert!(matches!(r.bonus.phase, Phase::ToBox { target: 1100 }));
        r.bonus.observe_encoder(1100, start);
        r.tick_bonus(start).unwrap();
        assert!(matches!(r.bonus.phase, Phase::AlignOut));
        feedback(&mut r, "bonus_align", 2500, start);
        r.tick_bonus(start).unwrap();
        assert!(matches!(r.bonus.phase, Phase::AlignHome));
        feedback(&mut r, "bonus_align", 2000, start);
        r.tick_bonus(start).unwrap();
        assert!(matches!(r.bonus.phase, Phase::LidOpen));
        let after_dwell = start + Duration::from_secs(1);
        feedback(&mut r, "bonus_lid", 2600, after_dwell);
        r.tick_bonus(after_dwell).unwrap();
        assert!(matches!(r.bonus.phase, Phase::LidClose));
        feedback(&mut r, "bonus_lid", 2100, after_dwell);
        r.tick_bonus(after_dwell).unwrap();
        assert!(matches!(r.bonus.phase, Phase::Return { target: 100 }));
        r.bonus.observe_encoder(100, after_dwell);
        r.tick_bonus(after_dwell).unwrap();
        assert!(!r.bonus.active());
        assert_eq!(r.bonus.loaded, 0);
    }

    #[test]
    fn manual_selector_expires_and_capture_uses_current_encoder() {
        let mut r = runtime();
        r.bonus.handoff = None;
        r.bonus_request(&Request::new("bonus_capture")).unwrap();
        assert_eq!(r.bonus.handoff, Some(100));
        r.bonus_request(&Request {
            value: Some(1.0),
            ..Request::new("bonus_jog")
        })
        .unwrap();
        let deadline = r.bonus.manual_until.unwrap();
        r.tick_bonus(deadline).unwrap();
        assert!(r.bonus.manual_until.is_none());
        assert_eq!(r.bonus.last_dc.unwrap().0, 0);
    }

    #[test]
    fn manual_selector_stops_toward_handoff_limit_and_allows_retreat() {
        let mut r = runtime();
        r.cfg.machine.bonus.as_mut().unwrap().handoff_limit = Some(HandoffLimit {
            input: 0,
            direction: -1,
            normally_closed: false,
        });
        r.bonus.observe_contacts(1, Instant::now());
        r.bonus_request(&Request {
            value: Some(-1.0),
            ..Request::new("bonus_jog")
        })
        .unwrap();
        assert!(r.bonus.manual_until.is_none());
        assert_eq!(r.bonus.last_dc.unwrap().0, 0);

        r.bonus_request(&Request {
            value: Some(1.0),
            ..Request::new("bonus_jog")
        })
        .unwrap();
        assert!(r.bonus.manual_until.is_some());
        assert!(r.bonus.last_dc.unwrap().0 > 0);
    }

    #[test]
    fn return_limit_finishes_cycle_and_refreshes_handoff_count() {
        let mut r = runtime();
        r.cfg.machine.bonus.as_mut().unwrap().handoff_limit = Some(HandoffLimit {
            input: 0,
            direction: -1,
            normally_closed: false,
        });
        let now = Instant::now();
        r.bonus.observe_encoder(123, now);
        r.bonus.observe_contacts(1, now);
        r.bonus.loaded = 6;
        r.bonus.phase = Phase::Return { target: 100 };
        r.bonus.cycle_started = Some(now);
        r.tick_bonus(now).unwrap();
        assert!(!r.bonus.active());
        assert_eq!(r.bonus.handoff, Some(123));
        assert_eq!(r.bonus.loaded, 0);
    }

    #[test]
    fn return_with_limit_keeps_seeking_slowly_after_encoder_reference() {
        let mut r = runtime();
        r.cfg.machine.bonus.as_mut().unwrap().handoff_limit = Some(HandoffLimit {
            input: 0,
            direction: -1,
            normally_closed: false,
        });
        let now = Instant::now();
        r.bonus.observe_encoder(100, now);
        r.bonus.observe_contacts(0, now);
        r.bonus.phase = Phase::Return { target: 100 };
        r.bonus.cycle_started = Some(now);
        r.tick_bonus(now).unwrap();
        assert!(r.bonus.active());
        assert_eq!(r.bonus.last_dc.unwrap().0, -40);
    }
}

#[derive(Default, PartialEq)]
enum Phase {
    #[default]
    Idle,
    ToBox {
        target: i32,
    },
    AlignOut,
    AlignHome,
    LidOpen,
    LidClose,
    Return {
        target: i32,
    },
}

impl Control {
    pub fn active(&self) -> bool {
        self.phase != Phase::Idle
    }
    pub fn outputs_active(&self) -> bool {
        self.active() || self.manual_until.is_some() || self.sts_holding
    }
    pub fn cancel(&mut self) {
        self.phase = Phase::Idle;
        self.last_dc = None;
        self.servo_started = None;
        self.manual_until = None;
        self.cycle_started = None;
        self.sts_holding = false;
    }
    pub fn reset_reference(&mut self) {
        self.cancel();
        self.handoff = None;
        self.encoder = None;
        self.contacts = None;
        self.last_limit_poll = None;
    }
    pub fn observe_encoder(&mut self, count: i32, now: Instant) {
        self.encoder = Some((count, now));
    }
    pub fn observe_contacts(&mut self, contacts: u8, now: Instant) {
        self.contacts = Some((contacts, now));
    }
    fn label(&self) -> &'static str {
        match self.phase {
            Phase::Idle => "待機",
            Phase::ToBox { .. } => "ボックスへ移動中",
            Phase::AlignOut => "ワーク整列中",
            Phase::AlignHome => "整列軸を戻しています",
            Phase::LidOpen => "蓋を開いて排出中",
            Phase::LidClose => "蓋を閉じています",
            Phase::Return { .. } => "受け渡し位置へ復帰中",
        }
    }
}

impl Runtime {
    fn bonus_limit_reached(&self, profile: &BonusProfile, now: Instant) -> Result<Option<bool>> {
        let Some(limit) = profile.handoff_limit else {
            return Ok(None);
        };
        let (contacts, seen) = self
            .bonus
            .contacts
            .context("DCMDの受け渡し側リミットを取得できません")?;
        anyhow::ensure!(
            now.saturating_duration_since(seen) < Duration::from_millis(300),
            "DCMDの受け渡し側リミット情報が古いため動かせません"
        );
        Ok(Some(limit.reached(contacts)))
    }

    fn poll_bonus_limit(&mut self, profile: &BonusProfile, now: Instant) -> Result<()> {
        if profile.handoff_limit.is_some()
            && self.bonus.last_limit_poll.is_none_or(|last| {
                now.saturating_duration_since(last) >= Duration::from_millis(100)
            })
        {
            self.send(&crate::protocol::dcmd::line(6, 0, 0))?;
            self.bonus.last_limit_poll = Some(now);
        }
        Ok(())
    }

    fn bonus_profile(&self) -> Result<BonusProfile> {
        self.cfg
            .machine
            .bonus
            .clone()
            .filter(|p| p.enabled)
            .context("ボーナスハンドが設定・有効化されていません")
    }

    fn bonus_servo(&mut self, name: &str, position: i16) -> Result<()> {
        let servo = self
            .cfg
            .machine
            .serial_svmd
            .as_ref()
            .and_then(|board| board.servos.iter().find(|servo| servo.name == name))
            .context("ボーナスハンドのSTS設定がありません")?;
        anyhow::ensure!(
            servo.enabled,
            "{}の通常出力が許可されていません",
            servo.name
        );
        let id = servo.id;
        let speed = servo.speed_position_per_second.round().clamp(1.0, 1000.0) as u16;
        for line in [
            crate::protocol::serial_svmd::Command::Target {
                id,
                position,
                speed,
                acceleration: servo.acceleration,
            }
            .to_cctl_line(),
            crate::protocol::serial_svmd::Command::Enable { id, enabled: true }.to_cctl_line(),
            crate::protocol::serial_svmd::Command::Run.to_cctl_line(),
        ] {
            self.send(&line)?;
        }
        self.bonus.servo_started = Some(Instant::now());
        self.bonus.sts_holding = true;
        Ok(())
    }

    fn bonus_dc(&mut self, duty: i16, force: bool) -> Result<()> {
        let now = Instant::now();
        if force
            || self.bonus.last_dc.is_none_or(|(old, at)| {
                old != duty || now.duration_since(at) >= Duration::from_millis(100)
            })
        {
            self.send(&crate::protocol::dcmd::line(4, 0, duty))?;
            self.bonus.last_dc = Some((duty, now));
        }
        Ok(())
    }

    fn begin_bonus_cycle(&mut self, box_index: usize) -> Result<()> {
        let profile = self.bonus_profile()?;
        anyhow::ensure!(!self.bonus.active(), "ボーナスハンドは動作中です");
        let limit_reached = self.bonus_limit_reached(&profile, Instant::now())?;
        let handoff = self
            .bonus
            .handoff
            .context("受け渡し位置を先に登録してください")?;
        let (encoder, seen) = self.bonus.encoder.context("AMT102-Vの値を取得できません")?;
        anyhow::ensure!(
            seen.elapsed() < Duration::from_millis(300),
            "AMT102-Vの値が古いため開始できません"
        );
        let target = handoff
            .checked_add(
                profile
                    .boxes
                    .get(box_index)
                    .context("ボックス番号が不正です")?
                    .offset_counts,
            )
            .context("ボックス位置がカウント範囲外です")?;
        if limit_reached == Some(true)
            && profile.handoff_limit.is_some_and(|limit| {
                (target - encoder).signum() as i16 == i16::from(limit.direction)
            })
        {
            bail!("受け渡し側リミットへ押し込む向きには開始できません");
        }
        self.bonus.selected_box = box_index;
        self.bonus.phase = if (target - encoder).abs() <= profile.selector_tolerance_counts {
            Phase::AlignOut
        } else {
            Phase::ToBox { target }
        };
        self.bonus.cycle_started = Some(Instant::now());
        self.send(&crate::protocol::dcmd::line(0, 0, 0))?;
        let error = target - encoder;
        let magnitude = if error.abs() <= profile.selector_slow_zone_counts {
            profile.selector_slow_duty
        } else {
            profile.selector_duty
        };
        self.bonus_dc(
            if error.abs() <= profile.selector_tolerance_counts {
                0
            } else {
                error.signum() as i16 * magnitude as i16
            },
            true,
        )?;
        self.send(&crate::protocol::dcmd::line(2, 1, 0))?;
        if self.bonus.phase == Phase::AlignOut {
            self.bonus_servo(&profile.align_servo, profile.align_position)?;
        }
        Ok(())
    }

    pub(super) fn bonus_request(&mut self, req: &Request) -> Result<Reply> {
        anyhow::ensure!(
            self.fresh() && self.setup && !self.emergency,
            "接続・設定・緊停状態を確認してください"
        );
        anyhow::ensure!(
            self.homing.is_none()
                && self.sequence.is_none()
                && !self.test.active
                && !self.sts.active
                && !self.sts.control_busy(),
            "ホーミング・シーケンス・個別テストを停止してください"
        );
        let profile = self.bonus_profile()?;
        match req.action.as_str() {
            "bonus_mode" => {
                anyhow::ensure!(!self.bonus.active(), "動作を停止してから切り替えてください");
                self.bonus.semi_auto = req.flag.context("モードが必要です")?;
            }
            "bonus_capture" => {
                anyhow::ensure!(!self.bonus.active(), "動作を停止してください");
                let (count, seen) = self.bonus.encoder.context("AMT102-Vの値を取得できません")?;
                anyhow::ensure!(
                    seen.elapsed() < Duration::from_millis(300),
                    "AMT102-Vの値が古いため登録できません"
                );
                self.bonus.handoff = Some(count);
                return Ok(Reply::data(format!(
                    "現在位置 {count} countを受け渡し位置として登録しました"
                )));
            }
            "bonus_select" => {
                let index = req.value.context("ボックス番号が必要です")? as usize;
                anyhow::ensure!(index < profile.boxes.len(), "ボックス番号が不正です");
                self.bonus.selected_box = index;
            }
            "bonus_receive" => {
                let added = req
                    .value
                    .unwrap_or(3.0)
                    .round()
                    .clamp(1.0, f32::from(profile.capacity)) as u8;
                self.bonus.loaded = self
                    .bonus
                    .loaded
                    .saturating_add(added)
                    .min(profile.capacity);
                if self.bonus.semi_auto && self.bonus.loaded >= profile.capacity {
                    self.begin_bonus_cycle(self.bonus.selected_box)?;
                }
            }
            "bonus_reset" => self.bonus.loaded = 0,
            "bonus_shoot" => self
                .begin_bonus_cycle(req.value.unwrap_or(self.bonus.selected_box as f32) as usize)?,
            "bonus_jog" => {
                anyhow::ensure!(
                    !self.bonus.active() && !self.bonus.semi_auto,
                    "手動モードで停止中のみ操作できます"
                );
                let value = req.value.context("方向が必要です")?.clamp(-1.0, 1.0);
                let duty = (value * f32::from(profile.selector_duty)).round() as i16;
                let now = Instant::now();
                let blocked = if let Some(limit) = profile.handoff_limit {
                    duty.signum() == i16::from(limit.direction)
                        && self.bonus_limit_reached(&profile, now)? == Some(true)
                } else {
                    false
                };
                if blocked {
                    self.bonus_dc(0, true)?;
                    self.send(&crate::protocol::dcmd::line(3, 0, 0))?;
                    self.bonus.manual_until = None;
                    return Ok(Reply::data("受け渡し側リミットで停止しました".into()));
                }
                if duty == 0 {
                    self.bonus_dc(0, true)?;
                    self.send(&crate::protocol::dcmd::line(3, 0, 0))?;
                    self.bonus.manual_until = None;
                } else {
                    self.send(&crate::protocol::dcmd::line(0, 0, 0))?;
                    self.bonus_dc(duty, true)?;
                    self.send(&crate::protocol::dcmd::line(2, 1, 0))?;
                    self.bonus.manual_until = Some(Instant::now() + Duration::from_millis(200));
                }
            }
            "bonus_lid" => {
                anyhow::ensure!(
                    !self.bonus.active() && !self.bonus.semi_auto,
                    "手動モードで停止中のみ操作できます"
                );
                let position = if req.flag.unwrap_or(false) {
                    profile.lid_open_position
                } else {
                    profile.lid_closed_position
                };
                self.bonus_servo(&profile.lid_servo, position)?;
            }
            "bonus_align" => {
                anyhow::ensure!(
                    !self.bonus.active() && !self.bonus.semi_auto,
                    "手動モードで停止中のみ操作できます"
                );
                let position = if req.flag.unwrap_or(false) {
                    profile.align_position
                } else {
                    profile.align_home_position
                };
                self.bonus_servo(&profile.align_servo, position)?;
            }
            _ => bail!("未対応のボーナスハンド操作です"),
        }
        Ok(Reply::accepted())
    }

    fn bonus_servo_arrived(
        &self,
        name: &str,
        target: i16,
        profile: &BonusProfile,
        now: Instant,
    ) -> bool {
        let Some(id) = self
            .cfg
            .machine
            .serial_svmd
            .as_ref()
            .and_then(|b| b.servos.iter().find(|s| s.name == name))
            .map(|s| s.id)
        else {
            return false;
        };
        let started = self.bonus.servo_started;
        self.servo_feedback.get(&id).is_some_and(|f| {
            now.saturating_duration_since(f.seen) < Duration::from_millis(300)
                && f.error == 0
                && started.is_some_and(|started| f.seen >= started)
                && (f.position - i32::from(target)).abs() <= profile.servo_tolerance_counts
        })
    }

    pub(super) fn tick_bonus(&mut self, now: Instant) -> Result<()> {
        if let Ok(profile) = self.bonus_profile() {
            self.poll_bonus_limit(&profile, now)?;
            if self.bonus.manual_until.is_some()
                && self.bonus.last_dc.is_some_and(|(duty, _)| {
                    profile
                        .handoff_limit
                        .is_some_and(|limit| duty.signum() == i16::from(limit.direction))
                })
                && self.bonus_limit_reached(&profile, now)? == Some(true)
            {
                self.bonus_dc(0, true)?;
                self.send(&crate::protocol::dcmd::line(3, 0, 0))?;
                self.bonus.manual_until = None;
            }
        }
        if self
            .bonus
            .manual_until
            .is_some_and(|deadline| now >= deadline)
        {
            self.bonus_dc(0, true)?;
            self.send(&crate::protocol::dcmd::line(3, 0, 0))?;
            self.bonus.manual_until = None;
        }
        if !self.bonus.active() {
            return Ok(());
        }
        let profile = self.bonus_profile()?;
        anyhow::ensure!(
            self.bonus
                .cycle_started
                .is_some_and(|started| now.saturating_duration_since(started)
                    <= Duration::from_millis(profile.cycle_timeout_ms)),
            "ボーナスハンドの自動動作が時間内に完了しませんでした"
        );
        let phase = std::mem::take(&mut self.bonus.phase);
        self.bonus.phase = match phase {
            Phase::Idle => Phase::Idle,
            Phase::ToBox { target } | Phase::Return { target } => {
                let returning = matches!(phase, Phase::Return { .. });
                let (position, seen) =
                    self.bonus.encoder.context("AMT102-Vの値を取得できません")?;
                anyhow::ensure!(
                    now.saturating_duration_since(seen) < Duration::from_millis(300),
                    "AMT102-Vの更新が停止しました"
                );
                let error = target - position;
                let limit_reached = self.bonus_limit_reached(&profile, now)?;
                if returning && limit_reached == Some(true) {
                    self.bonus_dc(0, true)?;
                    self.bonus.handoff = Some(position);
                    self.bonus.loaded = 0;
                    self.bonus.cycle_started = None;
                    self.send(&crate::protocol::dcmd::line(3, 0, 0))?;
                    Phase::Idle
                } else if error.abs() <= profile.selector_tolerance_counts
                    && (!returning || profile.handoff_limit.is_none())
                {
                    self.bonus_dc(0, true)?;
                    if returning {
                        self.bonus.loaded = 0;
                        self.bonus.cycle_started = None;
                        self.send(&crate::protocol::dcmd::line(3, 0, 0))?;
                        Phase::Idle
                    } else {
                        self.bonus_servo(&profile.align_servo, profile.align_position)?;
                        Phase::AlignOut
                    }
                } else {
                    let passed_reference = returning
                        && profile.handoff_limit.is_some_and(|limit| {
                            error.signum() as i16 != i16::from(limit.direction)
                        });
                    let magnitude =
                        if passed_reference || error.abs() <= profile.selector_slow_zone_counts {
                            profile.selector_slow_duty
                        } else {
                            profile.selector_duty
                        };
                    let direction = if returning {
                        profile
                            .handoff_limit
                            .map_or(error.signum() as i16, |limit| i16::from(limit.direction))
                    } else {
                        error.signum() as i16
                    };
                    if !returning
                        && limit_reached == Some(true)
                        && profile
                            .handoff_limit
                            .is_some_and(|limit| direction == i16::from(limit.direction))
                    {
                        bail!("ボックスへ移動中に受け渡し側リミットへ到達しました");
                    }
                    self.bonus_dc(direction * magnitude as i16, false)?;
                    if returning {
                        Phase::Return { target }
                    } else {
                        Phase::ToBox { target }
                    }
                }
            }
            Phase::AlignOut => {
                self.poll_bonus_servo(&profile.align_servo)?;
                if self.bonus_servo_arrived(
                    &profile.align_servo,
                    profile.align_position,
                    &profile,
                    now,
                ) {
                    self.bonus_servo(&profile.align_servo, profile.align_home_position)?;
                    Phase::AlignHome
                } else {
                    Phase::AlignOut
                }
            }
            Phase::AlignHome => {
                self.poll_bonus_servo(&profile.align_servo)?;
                if self.bonus_servo_arrived(
                    &profile.align_servo,
                    profile.align_home_position,
                    &profile,
                    now,
                ) {
                    self.bonus_servo(&profile.lid_servo, profile.lid_open_position)?;
                    Phase::LidOpen
                } else {
                    Phase::AlignHome
                }
            }
            Phase::LidOpen => {
                self.poll_bonus_servo(&profile.lid_servo)?;
                let arrived = self.bonus_servo_arrived(
                    &profile.lid_servo,
                    profile.lid_open_position,
                    &profile,
                    now,
                );
                if arrived
                    && self.bonus.servo_started.is_some_and(|at| {
                        now.saturating_duration_since(at) >= Duration::from_millis(profile.dwell_ms)
                    })
                {
                    self.bonus_servo(&profile.lid_servo, profile.lid_closed_position)?;
                    Phase::LidClose
                } else {
                    Phase::LidOpen
                }
            }
            Phase::LidClose => {
                self.poll_bonus_servo(&profile.lid_servo)?;
                if self.bonus_servo_arrived(
                    &profile.lid_servo,
                    profile.lid_closed_position,
                    &profile,
                    now,
                ) {
                    Phase::Return {
                        target: self.bonus.handoff.unwrap(),
                    }
                } else {
                    Phase::LidClose
                }
            }
        };
        Ok(())
    }

    fn poll_bonus_servo(&mut self, name: &str) -> Result<()> {
        let id = self
            .cfg
            .machine
            .serial_svmd
            .as_ref()
            .and_then(|b| b.servos.iter().find(|s| s.name == name))
            .map(|s| s.id)
            .context("STS設定がありません")?;
        self.send(&crate::protocol::serial_svmd::Command::Read { id }.to_cctl_line())
    }

    pub(super) fn publish_bonus(&self, status: &mut crate::application::app_state::BonusStatus) {
        let profile = self.cfg.machine.bonus.as_ref();
        status.configured = profile.is_some_and(|p| p.enabled);
        status.semi_auto = self.bonus.semi_auto;
        status.handoff_captured = self.bonus.handoff.is_some();
        status.encoder_count = self.bonus.encoder.map(|v| v.0);
        status.position_counts = self
            .bonus
            .encoder
            .zip(self.bonus.handoff)
            .map(|((count, _), origin)| count - origin);
        status.handoff_limit_configured = profile.is_some_and(|p| p.handoff_limit.is_some());
        status.handoff_limit = profile.and_then(|p| p.handoff_limit).and_then(|limit| {
            self.bonus.contacts.and_then(|(contacts, seen)| {
                (seen.elapsed() < Duration::from_millis(300)).then(|| limit.reached(contacts))
            })
        });
        status.selected_box = self.bonus.selected_box;
        status.box_names = profile
            .map(|p| p.boxes.iter().map(|b| b.name.clone()).collect())
            .unwrap_or_default();
        status.loaded = self.bonus.loaded;
        status.capacity = profile.map_or(0, |p| p.capacity);
        status.active = self.bonus.active();
        status.phase = self.bonus.label().into();
    }
}

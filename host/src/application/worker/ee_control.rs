use super::*;
use crate::{diagnostics::individual::Target, machine::ee};
use std::collections::BTreeMap;

fn pwm_step(
    previous: f32,
    goal: f32,
    mut velocity: f32,
    max_speed: f32,
    acceleration: f32,
    dt: f32,
) -> (f32, f32) {
    let error = goal - previous;
    if error.abs() <= 0.5 {
        return (goal, 0.0);
    }
    if acceleration == 0.0 {
        velocity = error.signum() * max_speed;
    } else {
        let desired = error.signum() * max_speed.min((2.0 * acceleration * error.abs()).sqrt());
        velocity += (desired - velocity).clamp(-acceleration * dt, acceleration * dt);
    }
    let candidate = previous + velocity * dt;
    if error * (goal - candidate) <= 0.0 {
        (goal, 0.0)
    } else {
        (candidate, velocity)
    }
}

#[derive(Default)]
pub(super) struct Control {
    pub targets: BTreeMap<String, f32>,
    pub(super) goals: BTreeMap<String, f32>,
    velocities: BTreeMap<String, f32>,
    sent: BTreeMap<String, (f32, Instant)>,
    started: Option<Instant>,
    tick: Option<Instant>,
    poll: Option<Instant>,
    pwm_arming_since: BTreeMap<String, Instant>,
    /// 先端回転のフィールド基準角[deg]。運転開始時は実測姿勢を採用し、△で180°反転する。
    pub(super) rotation_field: f32,
}
impl Runtime {
    /// θ正面合わせ時のSTS実測から、現在のEEフィールド角を求める。
    /// EE回転が未設定の機体では保存対象なしとして扱う。
    pub(super) fn measured_rotation_field(&self, now: Instant) -> Result<Option<f32>> {
        let Some(axis) = ee::axes(&self.cfg.machine)
            .into_iter()
            .find(|axis| axis.name == "ee_rotation" && axis.enabled)
        else {
            return Ok(None);
        };
        let Target::Sts(id) = axis.target else {
            return Ok(None);
        };
        anyhow::ensure!(
            self.test
                .peers
                .get(axis.target.board())
                .is_some_and(
                    |seen| now.saturating_duration_since(*seen) < Duration::from_millis(500)
                ),
            "正面合わせ時のSTS基板応答を確認できません"
        );
        let feedback = self
            .servo_feedback
            .get(&id)
            .filter(|feedback| {
                now.saturating_duration_since(feedback.seen) < Duration::from_millis(500)
                    && feedback.error == 0
            })
            .context("正面合わせ後のEE回転位置を取得できません")?;
        anyhow::ensure!(
            axis.counts_per_deg.is_finite() && axis.counts_per_deg.abs() > f32::EPSILON,
            "先端回転の角度換算が設定されていません"
        );
        let count = i16::try_from(feedback.position).context("EEサーボ位置が指令範囲外です")?;
        let theta = self
            .machine
            .axis_position("theta", self.telemetry.as_ref())
            .context("θ原点を確認してください")?;
        Ok(Some(
            theta + (f32::from(count) - axis.zero0_count) / axis.counts_per_deg,
        ))
    }

    /// PWMサーボは位置フィードバックを持たないため、基板が最後に保持している
    /// パルス幅を出力OFFのまま取得し、そこから補間を開始する。
    fn start_pwm_outputs(&mut self, now: Instant) -> Result<()> {
        let axes = ee::axes(&self.cfg.machine);
        let mut ready = Vec::new();
        let mut retry = Vec::new();
        for axis in axes.iter().filter(|axis| {
            matches!(axis.target, Target::Pwm(_))
                && self.ee.goals.contains_key(&axis.name)
                && !self.ee.targets.contains_key(&axis.name)
        }) {
            let Target::Pwm(channel) = axis.target else {
                unreachable!()
            };
            let Some(arming_since) = self.ee.pwm_arming_since.get(&axis.name).copied() else {
                continue;
            };
            if let Some(feedback) = self.pwm_feedback.get(&channel).filter(|feedback| {
                feedback.seen > arming_since
                    && feedback.state.result == 0
                    && feedback.state.enabled_mask & (1 << channel) == 0
            }) {
                let current = f32::from(feedback.state.pulse_us).clamp(axis.min, axis.max);
                ready.push((axis.clone(), current));
            } else if arming_since.elapsed() >= Duration::from_millis(200) {
                retry.push((axis.name.clone(), channel));
            }
        }
        for (name, channel) in retry {
            self.ee.pwm_arming_since.insert(name, Instant::now());
            self.send(
                &crate::protocol::svmd::Command::Enable {
                    channel,
                    enabled: false,
                }
                .to_cctl_line(),
            )?;
        }
        for (axis, current) in ready {
            self.ee.pwm_arming_since.remove(&axis.name);
            self.ee.targets.insert(axis.name.clone(), current);
            self.ee.velocities.insert(axis.name.clone(), 0.0);
            self.ee.sent.insert(axis.name.clone(), (current, now));
            self.ee.started = Some(now);
            for line in axis.commands(current)? {
                self.send(&line)?;
            }
        }
        Ok(())
    }

    /// 通常運転へ入った時点の実測姿勢をそのままフィールド基準として採用し、
    /// 最初からθ追従を有効にする。実測と同じカウントを初回目標にするため急動作しない。
    fn start_rotation_tracking(&mut self, now: Instant) -> Result<()> {
        if !self.drive.running()
            || self.sequence.is_some()
            || self.ee.targets.contains_key("ee_rotation")
        {
            return Ok(());
        }
        let Some(axis) = ee::axes(&self.cfg.machine)
            .into_iter()
            .find(|axis| axis.name == "ee_rotation" && axis.enabled)
        else {
            return Ok(());
        };
        let Target::Sts(id) = axis.target else {
            return Ok(());
        };
        if !self
            .test
            .peers
            .get(axis.target.board())
            .is_some_and(|seen| seen.elapsed() < Duration::from_millis(500))
        {
            return Ok(());
        }
        let Some(feedback) = self.servo_feedback.get(&id).filter(|feedback| {
            now.saturating_duration_since(feedback.seen) < Duration::from_millis(500)
                && feedback.error == 0
        }) else {
            return Ok(());
        };
        anyhow::ensure!(
            axis.counts_per_deg.is_finite() && axis.counts_per_deg.abs() > f32::EPSILON,
            "先端回転の角度換算が設定されていません"
        );
        let count = i16::try_from(feedback.position).context("EEサーボ位置が指令範囲外です")?;
        let theta = self
            .machine
            .axis_position("theta", self.telemetry.as_ref())
            .context("θ原点を確認してください")?;
        let count = if let Some(field) = self.prepared_rotation_field {
            self.ee.rotation_field = field;
            axis.rotation_count(field, theta)
        } else {
            self.ee.rotation_field =
                theta + (f32::from(count) - axis.zero0_count) / axis.counts_per_deg;
            count
        };
        self.prepared_rotation_field = None;
        self.ee.targets.insert(axis.name.clone(), f32::from(count));
        self.ee
            .sent
            .insert(axis.name.clone(), (f32::from(count), now));
        self.ee.started = Some(now);
        for line in axis.rotation_command(count, true) {
            self.send(&line)?;
        }
        Ok(())
    }

    pub(super) fn ee_request(&mut self, req: &Request) -> Result<Reply> {
        anyhow::ensure!(
            self.drive.running()
                && !self.test.enabled
                && !self.sts.active
                && !self.sts.busy()
                && !self.emergency,
            "通常運転を再開してからEEを操作してください"
        );
        self.ready()?;
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Input {
            targets: BTreeMap<String, f32>,
        }
        let input: Input = toml::from_str(req.text.as_deref().context("targetsが必要です")?)?;
        anyhow::ensure!(!input.targets.is_empty(), "対象を指定してください");
        let mut targets = input.targets;
        // 先端回転はフィールド基準角として専用経路で扱う。
        if let Some(field) = targets.remove("ee_rotation") {
            self.rotation_request(field)?;
        }
        if targets.is_empty() {
            return Ok(Reply::accepted());
        }
        let axes = ee::axes(&self.cfg.machine);
        let mut lines = Vec::new();
        let mut accepted = Vec::new();
        for (name, value) in &targets {
            let axis = axes
                .iter()
                .find(|a| a.name == *name)
                .context("EE軸が未割当です")?;
            anyhow::ensure!(
                self.test
                    .peers
                    .get(axis.target.board())
                    .is_some_and(|t| t.elapsed() < Duration::from_millis(500)),
                "{}の基板応答を待ってください",
                axis.label
            );
            axis.commands(*value)?;
            let current = self.ee.targets.get(name).copied();
            if matches!(axis.target, Target::Pwm(_)) {
                if current.is_none() {
                    let Target::Pwm(channel) = axis.target else {
                        unreachable!()
                    };
                    lines.push(
                        crate::protocol::svmd::Command::Enable {
                            channel,
                            enabled: false,
                        }
                        .to_cctl_line(),
                    );
                }
            } else {
                let commands = axis.commands(*value)?;
                lines.extend(commands.into_iter().take(if current.is_some() {
                    1
                } else {
                    usize::MAX
                }));
            }
            accepted.push((name.clone(), *value, current, axis.target));
        }
        // 全対象の検証が完了するまで送信しない。送信途中失敗も停止対象に含める。
        let requested_at = Instant::now();
        for (name, value, current, target) in accepted {
            if matches!(target, Target::Pwm(_)) {
                self.ee.goals.insert(name.clone(), value);
                if let Some(current) = current {
                    self.ee.targets.insert(name.clone(), current);
                    self.ee.velocities.entry(name.clone()).or_insert(0.0);
                    self.ee.sent.insert(name, (current, requested_at));
                } else {
                    self.ee.pwm_arming_since.insert(name, requested_at);
                }
            } else {
                self.ee.targets.insert(name.clone(), value);
                self.ee.sent.insert(name, (value, requested_at));
            }
        }
        self.ee.started = Some(requested_at);
        for line in lines {
            if let Err(error) = self.send(&line) {
                self.fault(error.to_string());
                return Err(error);
            }
        }
        Ok(Reply::accepted())
    }
    /// 先端回転をフィールド基準角[deg]へ向ける。θを打ち消して連続追従できるよう、
    /// 初回はトルク有効化とRUNも送り、以降はtickが追従を続ける。
    pub(super) fn rotation_request(&mut self, field_deg: f32) -> Result<()> {
        anyhow::ensure!(
            self.drive.running()
                && !self.test.enabled
                && !self.sts.active
                && !self.sts.busy()
                && !self.emergency,
            "通常運転を再開してからEEを操作してください"
        );
        anyhow::ensure!(field_deg.is_finite(), "先端回転角が不正です");
        self.ready()?;
        let axes = ee::axes(&self.cfg.machine);
        let axis = axes
            .iter()
            .find(|a| a.name == "ee_rotation")
            .context("先端回転のEE割当が未設定です")?;
        anyhow::ensure!(
            self.test
                .peers
                .get(axis.target.board())
                .is_some_and(|t| t.elapsed() < Duration::from_millis(500)),
            "{}の基板応答を待ってください",
            axis.label
        );
        self.ee.rotation_field = field_deg;
        let theta = self
            .machine
            .axis_position("theta", self.telemetry.as_ref())
            .unwrap_or(0.0);
        let count = axis.rotation_count(field_deg, theta);
        let first = !self.ee.targets.contains_key(&axis.name);
        self.ee.targets.insert(axis.name.clone(), f32::from(count));
        self.ee
            .sent
            .insert(axis.name.clone(), (f32::from(count), Instant::now()));
        self.ee.started = Some(Instant::now());
        for line in axis.rotation_command(count, first) {
            if let Err(error) = self.send(&line) {
                self.fault(error.to_string());
                return Err(error);
            }
        }
        Ok(())
    }

    pub(super) fn observe_ee(&mut self, line: &str) -> Result<()> {
        if self.ee.targets.is_empty() {
            return Ok(());
        }
        let axes: Vec<_> = ee::axes(&self.cfg.machine)
            .into_iter()
            .filter(|a| self.ee.targets.contains_key(&a.name))
            .collect();
        let settled = self
            .ee
            .started
            .is_some_and(|t| t.elapsed() > Duration::from_millis(500));
        for (id, board) in [(769, "pwm"), (801, "sts")] {
            if let Some(p) = crate::application::sts::frame(line, id) {
                for axis in axes.iter().filter(|a| a.target.board() == board) {
                    let disabled = match axis.target {
                        Target::Pwm(ch) => p[4] & (1 << ch) == 0,
                        Target::Sts(_) => p[2] != 1,
                        _ => false,
                    };
                    anyhow::ensure!(
                        p[1] == 0 && !(settled && disabled),
                        "{}の出力拒否または解除を検出しました",
                        axis.label
                    );
                }
            }
        }
        if let Some(s) = crate::protocol::serial_svmd::parse_state(line)
            && axes.iter().any(|a| a.target == Target::Sts(s.id))
        {
            anyhow::ensure!(
                s.error == 0 && (!settled || s.enabled),
                "EEサーボ{}の応答が異常です",
                s.id
            );
        }
        if let Some((id, detail)) = crate::protocol::serial_svmd::parse_diagnostic(line)
            && axes.iter().any(|a| a.target == Target::Sts(id))
        {
            anyhow::bail!("EEサーボ{id}: {detail}");
        }
        Ok(())
    }
    pub(super) fn tick_ee(&mut self, now: Instant) -> Result<()> {
        let dt = self
            .ee
            .tick
            .replace(now)
            .map_or(0.0, |t| now.duration_since(t).as_secs_f32().min(0.1));
        self.start_rotation_tracking(now)?;
        self.start_pwm_outputs(now)?;
        if self.ee.targets.is_empty() {
            return Ok(());
        }
        anyhow::ensure!(
            self.drive.running() && self.fresh(),
            "EE操作中に通常運転または通信を失いました"
        );
        let axes = ee::axes(&self.cfg.machine);
        let input = self.authority.input().unwrap_or(&self.manual_input).clone();
        let slow = self.adjustment || self.screen_control || input.buttons[9] != 0;
        for axis in axes
            .iter()
            .filter(|a| self.ee.targets.contains_key(&a.name))
        {
            anyhow::ensure!(
                self.test
                    .peers
                    .get(axis.target.board())
                    .is_some_and(|t| t.elapsed() < Duration::from_millis(500)),
                "{}の状態応答が途絶えました",
                axis.label
            );
        }
        for axis in &axes {
            if matches!(axis.target, Target::Pwm(_)) {
                let Some(goal) = self.ee.goals.get(&axis.name).copied() else {
                    continue;
                };
                let Some(previous) = self.ee.targets.get(&axis.name).copied() else {
                    continue;
                };
                let velocity = self.ee.velocities.get(&axis.name).copied().unwrap_or(0.0);
                let speed_scale = if self.sequence.is_none() && slow {
                    self.cfg.machine.slow_speed_percent * 0.01
                } else {
                    1.0
                };
                let max_speed = axis.speed * speed_scale;
                let acceleration = self
                    .cfg
                    .machine
                    .pwm_servos
                    .iter()
                    .find(|servo| servo.name == axis.name)
                    .expect("PWMのEE軸にはPWMサーボ設定がある")
                    .acceleration_us_per_second2
                    * speed_scale;
                let (next, velocity) =
                    pwm_step(previous, goal, velocity, max_speed, acceleration, dt);
                if self.ee.sent.get(&axis.name).is_none_or(|(value, sent)| {
                    *value != next.round() && now.duration_since(*sent) >= Duration::from_millis(50)
                }) {
                    for line in axis.commands(next.round())?.into_iter().take(1) {
                        self.send(&line)?;
                    }
                    self.ee.sent.insert(axis.name.clone(), (next.round(), now));
                }
                self.ee.targets.insert(axis.name.clone(), next);
                self.ee.velocities.insert(axis.name.clone(), velocity);
                continue;
            }
            if self.sequence.is_some() || !self.ee.targets.contains_key(&axis.name) {
                continue;
            }
            // 先端回転: θ回転を打ち消してフィールド基準の向きを保つよう連続追従する。
            let theta = self
                .machine
                .axis_position("theta", self.telemetry.as_ref())
                .unwrap_or(0.0);
            let count = axis.rotation_count(self.ee.rotation_field, theta);
            if self.ee.sent.get(&axis.name).is_none_or(|(value, sent)| {
                *value != f32::from(count) && now.duration_since(*sent) >= Duration::from_millis(50)
            }) {
                // 初回以外は目標だけを更新し、ENABLE/RUNを繰り返さない。
                for line in axis.rotation_command(count, false) {
                    self.send(&line)?;
                }
                self.ee
                    .sent
                    .insert(axis.name.clone(), (f32::from(count), now));
            }
            self.ee.targets.insert(axis.name.clone(), f32::from(count));
        }
        if self
            .ee
            .poll
            .is_none_or(|t| t.elapsed() > Duration::from_millis(200))
        {
            self.ee.poll = Some(now);
            for axis in &axes {
                if !self.ee.targets.contains_key(&axis.name) {
                    continue;
                }
                if let Target::Sts(id) = axis.target {
                    self.send(&crate::protocol::serial_svmd::Command::Read { id }.to_cctl_line())?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn ee_targets_reached(&self, targets: &BTreeMap<String, f32>) -> bool {
        targets.iter().all(|(name, target)| {
            let is_pwm = ee::axes(&self.cfg.machine)
                .iter()
                .any(|axis| axis.name == *name && matches!(axis.target, Target::Pwm(_)));
            !is_pwm
                || (self
                    .ee
                    .targets
                    .get(name)
                    .is_some_and(|value| (target - value).abs() <= 0.5)
                    && self
                        .ee
                        .velocities
                        .get(name)
                        .is_none_or(|value| value.abs() <= f32::EPSILON))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_run_starts_rotation_tracking_from_measured_position_without_triangle() {
        let mut r = crate::application::worker::tests::screen_runtime();
        r.cfg.machine.serial_svmd = MachineProfile::embedded().unwrap().serial_svmd;
        r.drive = DriveState::Running;
        let now = Instant::now();
        r.test.peers.insert("sts", now);
        r.servo_feedback.insert(
            1,
            ServoFeedback {
                seen: now,
                position: 1500,
                error: 0,
                detail: String::new(),
            },
        );

        r.tick_ee(now).unwrap();

        assert_eq!(r.ee.targets["ee_rotation"], 1500.0);
        let logs = r.shared.status_snapshot().logs;
        assert!(
            logs.iter()
                .any(|line| line.starts_with("TX CAN 2 800 010401"))
        );
        assert!(
            logs.iter()
                .any(|line| line == "TX CAN 2 800 0106010100000000")
        );
        assert!(
            logs.iter()
                .any(|line| line == "TX CAN 2 800 0102000000000000")
        );

        let theta = r
            .cfg
            .machine
            .axes
            .iter()
            .find(|axis| axis.name == "theta")
            .unwrap();
        r.telemetry.as_mut().unwrap().slots[usize::from(theta.slot)].measured +=
            theta.native_per_unit * 10.0;
        r.tick_ee(now + Duration::from_millis(60)).unwrap();
        assert!((r.ee.targets["ee_rotation"] - 1159.0).abs() <= 1.0);
    }

    #[test]
    fn stop_and_restart_keeps_the_unwrapped_rotation_position() {
        let mut r = crate::application::worker::tests::screen_runtime();
        r.cfg.machine.serial_svmd = MachineProfile::embedded().unwrap().serial_svmd;
        let now = Instant::now();
        r.test.peers.insert("sts", now);

        assert_eq!(r.sts.unwrap_position(1, 4085), 4085);
        r.ee.targets.insert("ee_rotation".into(), 4085.0);
        r.stop(false).unwrap();

        let position = r.sts.unwrap_position(1, 0);
        assert_eq!(position, 4096);
        r.servo_feedback.insert(
            1,
            ServoFeedback {
                seen: now,
                position,
                error: 0,
                detail: String::new(),
            },
        );
        r.drive = DriveState::Running;

        r.tick_ee(now).unwrap();

        assert_eq!(r.ee.targets["ee_rotation"], 4096.0);
        assert!(
            r.shared
                .status_snapshot()
                .logs
                .iter()
                .any(|line| { line.starts_with("TX CAN 2 800 010401") && line.contains("1000") })
        );
    }

    #[test]
    fn first_run_after_homing_restores_the_field_angle_captured_at_front() {
        let mut r = crate::application::worker::tests::screen_runtime();
        r.cfg.machine.serial_svmd = MachineProfile::embedded().unwrap().serial_svmd;
        r.drive = DriveState::Running;
        let now = Instant::now();
        r.test.peers.insert("sts", now);
        r.servo_feedback.insert(
            1,
            ServoFeedback {
                seen: now,
                position: 1500,
                error: 0,
                detail: String::new(),
            },
        );
        let field = 0.0;
        r.prepared_rotation_field = Some(field);
        let theta = r
            .machine
            .axis_position("theta", r.telemetry.as_ref())
            .unwrap();
        let axis = ee::axes(&r.cfg.machine)
            .into_iter()
            .find(|axis| axis.name == "ee_rotation")
            .unwrap();
        let expected = axis.rotation_count(field, theta);

        r.tick_ee(now).unwrap();

        assert_eq!(r.ee.rotation_field, field);
        assert_eq!(r.ee.targets["ee_rotation"], f32::from(expected));
        assert!(r.prepared_rotation_field.is_none());
    }

    #[test]
    fn zero_pwm_acceleration_changes_to_constant_speed_immediately() {
        assert_eq!(
            pwm_step(1000.0, 1500.0, 0.0, 1000.0, 0.0, 0.02),
            (1020.0, 1000.0)
        );
        assert_eq!(
            pwm_step(1490.0, 1500.0, 1000.0, 1000.0, 0.0, 0.02),
            (1500.0, 0.0)
        );
    }

    #[test]
    fn normal_ee_requires_permission_validates_group_and_stops() {
        let mut r = crate::application::worker::tests::screen_runtime();
        r.cfg
            .machine
            .pwm_servos
            .push(crate::machine::PwmServoProfile {
                name: "ee_fold".into(),
                channel: 0,
                input_axis: None,
                input_sign: 1.0,
                speed_us_per_second: 100.0,
                acceleration_us_per_second2: 2500.0,
                minimum_us: 1400,
                maximum_us: 1600,
                initial_us: 1500,
                enabled: false,
            });
        r.test.peers.insert("pwm", Instant::now());
        let req = Request {
            text: Some("[targets]\nee_fold=1500".into()),
            ..Request::new("ee")
        };
        assert!(r.request(&req, true).is_err());
        r.drive = DriveState::Running;
        assert!(r.request(&req, true).is_err());
        r.cfg.machine.pwm_servos[0].enabled = true;
        let bad = Request {
            text: Some("[targets]\nee_fold=1500\nee_grip_1=1500".into()),
            ..Request::new("ee")
        };
        assert!(r.request(&bad, true).is_err());
        assert!(r.ee.targets.is_empty());
        r.request(&req, true).unwrap();
        assert!(!r.ee.targets.contains_key("ee_fold"));
        let armed = r.ee.pwm_arming_since["ee_fold"];
        assert!(
            r.shared
                .status_snapshot()
                .logs
                .iter()
                .any(|line| line == "TX CAN 2 768 0102000000000000")
        );
        r.pwm_feedback.insert(
            0,
            PwmFeedback {
                seen: armed + Duration::from_millis(1),
                state: crate::protocol::svmd::State {
                    result: 0,
                    channel: 0,
                    enabled_mask: 0,
                    pulse_us: 1450,
                },
            },
        );
        r.tick_ee(armed + Duration::from_millis(2)).unwrap();
        assert!((r.ee.targets["ee_fold"] - 1450.0).abs() < 0.01);
        let pwm: Vec<_> = r
            .shared
            .status_snapshot()
            .logs
            .into_iter()
            .filter(|l| l.contains("CAN 2 768"))
            .collect();
        assert!(pwm[pwm.len() - 2].contains("0101000005AA0000"));
        assert!(pwm[pwm.len() - 1].contains("0102000100000000"));
        r.observe_ee("CAN_RX bus=2 id=769 data=0101000000000000")
            .unwrap_err();
        r.stop(false).unwrap();
        assert!(r.ee.targets.is_empty());
        assert!(!r.drive.running());
    }

    #[test]
    fn ee_fault_stops_only_ee_and_keeps_main_axes_running() {
        let mut r = crate::application::worker::tests::screen_runtime();
        r.drive = DriveState::Running;
        r.ee.targets.insert("ee_rotation".into(), 2048.0);
        let before = r.shared.status_snapshot().logs.len();

        r.fault_ee("EEサーボ1: 応答なし".into());

        assert!(r.drive.running());
        assert!(r.ee.targets.is_empty());
        let logs = r.shared.status_snapshot().logs;
        assert!(!logs.iter().skip(before).any(|line| line == "TX STOP"));
        assert!(r.reason.contains("r・θ・zの運転は継続"));
    }
}

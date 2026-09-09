use super::*;
use crate::application::sequence::{self, ResolvedStep, Start};
use std::collections::BTreeMap;

pub(super) struct Execution {
    previous_screen_control: bool,
    steps: Vec<ResolvedStep>,
    index: usize,
    started: Option<Instant>,
    from: BTreeMap<String, f32>,
    duration: f32,
    speed_percent: f32,
    tolerance_mm: f32,
    tolerance_deg: f32,
    timeout: f32,
}
impl Runtime {
    pub(super) fn sequence_request(&mut self, req: &Request, manual: bool) -> Result<Reply> {
        match req.action.as_str() {
            "sequence_apply" => {
                anyhow::ensure!(
                    self.sequence.is_none(),
                    "シーケンスを中断してから設定を適用してください"
                );
                let config: sequence::Config =
                    toml::from_str(req.text.as_deref().context("設定本文が必要です")?)?;
                config.validate()?;
                self.shared.set_sequence_config(config);
                self.shared.update_status(|s| {
                    s.sequence_saved = false;
                    s.sequence.message = "設定を適用しました".into();
                });
                Ok(Reply::data("シーケンス設定を適用しました".into()))
            }
            "sequence_load" => {
                anyhow::ensure!(self.sequence.is_none(), "実行完了後に再読込してください");
                let path = sequence::config_path(&self.cfg.profile_path);
                self.shared.set_sequence_config(sequence::load(&path)?);
                self.shared.update_status(|s| {
                    s.sequence_saved = true;
                    if !s.sequence.active {
                        s.sequence.message = "設定を保存・読込み済み".into();
                    }
                });
                Ok(Reply::data("シーケンス設定を読み込みました".into()))
            }
            "sequence_save" => {
                let path = sequence::config_path(&self.cfg.profile_path);
                sequence::save(&path, &self.shared.sequence_config())?;
                self.shared.update_status(|s| {
                    s.sequence_saved = true;
                    if !s.sequence.active {
                        s.sequence.message = "設定を保存・読込み済み".into();
                    }
                });
                Ok(Reply::data(format!("保存しました: {}", path.display())))
            }
            "sequence_start" => {
                anyhow::ensure!(self.sequence.is_none(), "シーケンス実行中です");
                let start: Start = toml::from_str(
                    req.text
                        .as_deref()
                        .context("取得グループと操作が必要です")?,
                )?;
                let config = self.shared.sequence_config();
                config.validate()?;
                let steps = config.resolve(&start)?;
                anyhow::ensure!(!steps.is_empty(), "実行する工程がありません");
                self.ready()?;
                // 実行する動作だけ検証する。アームだけの工程にEE割当を要求しない。
                let ee_axes = crate::machine::ee::axes(&self.cfg.machine);
                for step in &steps {
                    for (name, value) in &step.axes {
                        let axis = self
                            .cfg
                            .machine
                            .axes
                            .iter()
                            .find(|a| &a.name == name)
                            .context("アーム軸が未設定です")?;
                        anyhow::ensure!(
                            (*value >= axis.minimum) && (*value <= axis.maximum),
                            "{}: {name}={value} は設定可動域 {}〜{} の外です",
                            step.name,
                            axis.minimum,
                            axis.maximum
                        );
                        self.machine
                            .native_position(axis.slot, *value)
                            .context("対象軸の原点を採用してください")?;
                    }
                    for (name, value) in &step.ee {
                        ee_axes
                            .iter()
                            .find(|a| &a.name == name)
                            .with_context(|| format!("{name}が未割当です"))?
                            .commands(*value)?;
                    }
                }
                anyhow::ensure!(
                    !self.test.enabled && !self.sts.active && !self.sts.control_busy(),
                    "個別操作を終了してください"
                );
                // GUIからの開始は画面操作として扱い、接続していないパッドを要求しない。
                self.sts.cancel();
                self.sts.stop_monitoring();
                let previous_screen_control = self.screen_control;
                if manual {
                    self.screen_control = true;
                }
                self.manual_input = ControllerState::default();
                self.screen_input_times = [None; 6];
                self.authority.clear_input();
                self.pad.ee_armed = false;
                if !self.drive.running() && let Err(error) = self.start() {
                    self.screen_control = previous_screen_control;
                    return Err(error);
                }
                if let Some(telemetry) = self.telemetry.clone() {
                    for line in self.machine.hold_lines(&telemetry, telemetry.enabled_slots) {
                        self.send(&line)?;
                    }
                }
                self.sequence = Some(Execution {
                    previous_screen_control,
                    steps,
                    index: 0,
                    started: None,
                    from: BTreeMap::new(),
                    duration: 0.0,
                    speed_percent: config.speed_percent,
                    tolerance_mm: config.tolerance_mm,
                    tolerance_deg: config.tolerance_deg,
                    timeout: config.timeout_seconds,
                });
                self.shared.update_status(|s| {
                    s.sequence = sequence::Status {
                        active: true,
                        group: config.groups[start.group].name.clone(),
                        side: start.side.label().into(),
                        total: self.sequence.as_ref().unwrap().steps.len(),
                        step: 0,
                        message: "開始待ち".into(),
                    }
                });
                self.error.clear();
                Ok(Reply::accepted())
            }
            _ => bail!("未対応のシーケンス操作です"),
        }
    }
    pub(super) fn cancel_sequence(&mut self, message: &str) {
        if let Some(execution) = self.sequence.take() {
            self.screen_control = execution.previous_screen_control;
            self.shared.update_status(|s| {
                s.sequence.active = false;
                s.sequence.message = message.into();
            });
        }
    }
    pub(super) fn tick_sequence(&mut self, now: Instant) -> Result<()> {
        if !self.drive.running() {
            return Ok(());
        }
        let Some(mut execution) = self.sequence.take() else {
            return Ok(());
        };
        let result = self.advance_sequence(&mut execution, now);
        self.sequence = Some(execution);
        match result {
            Ok(true) => {
                self.cancel_sequence("完了・位置と把持を保持");
                self.machine.reset_jog();
                self.manual_input = ControllerState::default();
                self.authority.clear_input();
                self.reason = "シーケンス完了・位置と把持を保持".into();
                Ok(())
            }
            Ok(false) => Ok(()),
            Err(error) => {
                let message = format!("シーケンス中断: {error}");
                self.cancel_sequence(&message);
                self.stop(false)?;
                self.reason = message.clone();
                self.error = message;
                Ok(())
            }
        }
    }
    fn advance_sequence(&mut self, execution: &mut Execution, now: Instant) -> Result<bool> {
        let step = &execution.steps[execution.index];
        let positions = self.machine.origin_states(self.telemetry.as_ref());
        if execution.started.is_none() {
            execution.from.clear();
            execution.duration = 0.0;
            for (name, target) in &step.axes {
                let current = positions
                    .iter()
                    .find(|p| &p.name == name)
                    .context("軸位置を取得できません")?;
                let axis = self
                    .cfg
                    .machine
                    .axes
                    .iter()
                    .find(|a| &a.name == name)
                    .unwrap();
                execution.from.insert(name.clone(), current.position);
                // smoothstepの最大速度が設定速度を超えない時間で補間する。
                let speed = axis.speed_per_second * execution.speed_percent * 0.01;
                execution.duration = execution
                    .duration
                    .max(1.5 * (target - current.position).abs() / speed);
            }
            if !step.ee.is_empty() {
                #[derive(serde::Serialize)]
                struct Input<'a> {
                    targets: &'a BTreeMap<String, f32>,
                }
                self.ee_request(&Request {
                    text: Some(toml::to_string(&Input { targets: &step.ee })?),
                    ..Request::new("ee")
                })?;
            }
            execution.started = Some(now);
            self.shared.log(format!("SEQUENCE {}", step.name));
            self.shared.update_status(|s| {
                s.sequence.step = execution.index + 1;
                s.sequence.message = step.name.clone();
            });
        }
        let elapsed = now
            .saturating_duration_since(execution.started.unwrap())
            .as_secs_f32();
        anyhow::ensure!(
            elapsed <= execution.timeout,
            "{}が設定時間内に完了しませんでした",
            step.name
        );
        let t = if execution.duration > 0.0 {
            (elapsed / execution.duration).min(1.0)
        } else {
            1.0
        };
        let progress = t * t * (3.0 - 2.0 * t);
        let mut arrived = t >= 1.0;
        for (name, target) in &step.axes {
            let axis = self
                .cfg
                .machine
                .axes
                .iter()
                .find(|a| &a.name == name)
                .unwrap()
                .clone();
            let current = positions.iter().find(|p| &p.name == name).unwrap();
            let tolerance = if axis.unit == "deg" {
                execution.tolerance_deg
            } else {
                execution.tolerance_mm
            };
            if let Some(limit) = axis.limit {
                anyhow::ensure!(
                    !(current.at_limit == Some(true)
                        && (*target - current.position) * limit.direction > tolerance),
                    "{name}のリミットスイッチに到達しました"
                );
            }
            let from = execution.from[name];
            let position = from + (target - from) * progress;
            let native = self
                .machine
                .set_position_target(axis.slot, position)
                .context("軸の原点を確認してください")?;
            self.send(&format!("TARGET {} {native:.5}", axis.slot))?;
            arrived &= (target - current.position).abs() <= tolerance;
        }
        arrived &= self.ee_targets_reached(&step.ee);
        if arrived && elapsed >= step.wait_seconds {
            execution.index += 1;
            execution.started = None;
        }
        Ok(execution.index == execution.steps.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sequence::{Action, Side, Stage, Step};

    fn runtime() -> Runtime {
        let mut machine = MachineProfile::embedded().unwrap();
        for axis in &mut machine.axes {
            axis.origin_position = 0.0;
            axis.limit = None;
        }
        for (channel, name) in ["ee_fold", "ee_grip_1", "ee_grip_2", "ee_grip_3"]
            .iter()
            .enumerate()
        {
            machine.pwm_servos.push(crate::machine::PwmServoProfile {
                name: (*name).into(),
                channel: channel as u8,
                input_axis: None,
                input_sign: 1.0,
                speed_us_per_second: 100.0,
                acceleration_us_per_second2: 2500.0,
                minimum_us: 500,
                maximum_us: 2500,
                initial_us: 1500,
                enabled: true,
            });
        }
        for servo in &mut machine.pwm_servos {
            servo.enabled = true;
        }
        for servo in &mut machine.serial_svmd.as_mut().unwrap().servos {
            servo.enabled = true;
        }
        let shared = Arc::new(Shared::new(BridgeConfig {
            serial_device: "unused".into(),
            baud_rate: 115200,
            rate_hz: 20.0,
            machine,
            profile_path: "/dev/null".into(),
            simulate: true,
        }));
        let mut r = Runtime::new(shared);
        for _ in 0..200 {
            r.tick().unwrap();
        }
        for index in 0..r.cfg.machine.axes.len() {
            assert!(r.machine.capture_origin(index, r.telemetry.as_ref()));
        }
        let mut config = r.shared.sequence_config();
        config.groups[0].r = 25.0;
        config.groups[0].theta = 5.0;
        config.groups[0].approach_z = 15.0;
        config.groups[0].grab_z = 10.0;
        config.travel_z = 30.0;
        config.lift_mm = 5.0;
        config.left.r = 40.0;
        config.left.theta = 10.0;
        config.left.z = 25.0;
        config.right.r = 50.0;
        config.right.theta = -10.0;
        config.right.z = 28.0;
        for step in config
            .prepare
            .iter_mut()
            .chain(&mut config.pick)
            .chain(&mut config.transfer)
        {
            step.wait_seconds = 0.1;
        }
        // 取得上空への３軸移動とEE回転を同一工程で実行。
        config.prepare[0].actions.push(Action::WorkRotation);
        r.shared.set_sequence_config(config);
        r
    }
    fn start(r: &mut Runtime, stage: Stage, side: Side) {
        r.request(
            &Request {
                text: Some(
                    toml::to_string(&Start {
                        group: 0,
                        side,
                        stage,
                    })
                    .unwrap(),
                ),
                ..Request::new("sequence_start")
            },
            true,
        )
        .unwrap();
    }
    fn finish(r: &mut Runtime, now: &mut Instant) {
        for _ in 0..600 {
            *now += Duration::from_millis(50);
            r.tick_at(*now).unwrap();
            r.publish();
            if r.sequence.is_none() {
                assert!(
                    r.shared
                        .status_snapshot()
                        .sequence
                        .message
                        .starts_with("完了"),
                    "{}",
                    r.error
                );
                *now += Duration::from_millis(50);
                r.tick_at(*now).unwrap();
                return;
            }
        }
        panic!(
            "sequence did not finish: {}",
            r.shared.status_snapshot().sequence.message
        );
    }
    fn position(r: &Runtime, name: &str) -> f32 {
        r.machine
            .origin_states(r.telemetry.as_ref())
            .iter()
            .find(|p| p.name == name)
            .unwrap()
            .position
    }
    #[test]
    fn three_gui_operations_move_pick_lift_transfer_and_hold_without_stop() {
        let mut r = runtime();
        let mut now = Instant::now();
        start(&mut r, Stage::Prepare, Side::Left);
        finish(&mut r, &mut now);
        assert!((position(&r, "r") - 25.0).abs() < 0.1);
        assert!((position(&r, "z") - 15.0).abs() < 0.1);
        assert!(r.drive.running());
        assert_eq!(r.ee.targets.len(), 5);
        let offset = r.shared.status_snapshot().logs.len();
        // 工程境界で新しい選択に切替可能。取得完了では把持を保持する。
        start(&mut r, Stage::Pick, Side::Right);
        finish(&mut r, &mut now);
        assert!((position(&r, "z") - 15.0).abs() < 0.1);
        assert!(r.ee.targets.contains_key("ee_grip_3"));
        assert!(
            !r.shared
                .status_snapshot()
                .logs
                .iter()
                .skip(offset)
                .any(|l| l == "TX STOP")
        );
        start(&mut r, Stage::Transfer, Side::Right);
        finish(&mut r, &mut now);
        assert!((position(&r, "r") - 50.0).abs() < 0.1);
        assert!((position(&r, "theta") + 10.0).abs() < 0.1);
        assert!((position(&r, "z") - 28.0).abs() < 0.1);
        assert!(r.drive.running() && r.ee.targets.len() == 5);
        let held = position(&r, "r");
        for _ in 0..5 {
            r.tick().unwrap();
        }
        assert!((position(&r, "r") - held).abs() < 0.1);
    }
    #[test]
    fn continuous_plan_is_snapshot_and_manual_input_cannot_overwrite_it() {
        let mut r = runtime();
        let mut now = Instant::now();
        start(&mut r, Stage::All, Side::Left);
        let mut next = r.shared.sequence_config();
        next.left.r = 80.0;
        r.shared.set_sequence_config(next);
        assert!(
            r.request(
                &Request {
                    axis: Some("r".into()),
                    value: Some(1.0),
                    ..Request::new("input")
                },
                true
            )
            .is_err()
        );
        assert!(r.request(&Request::new("home"), true).is_err());
        finish(&mut r, &mut now);
        assert!((position(&r, "r") - 40.0).abs() < 0.1);
    }
    #[test]
    fn interruption_and_emergency_discard_remaining_steps() {
        for emergency in [false, true] {
            let mut r = runtime();
            start(&mut r, Stage::All, Side::Left);
            for _ in 0..3 {
                r.tick().unwrap();
            }
            r.request(
                &Request::new(if emergency { "estop" } else { "stop" }),
                true,
            )
            .unwrap();
            assert!(r.sequence.is_none());
            assert!(!r.drive.running());
            let count = r
                .shared
                .status_snapshot()
                .logs
                .iter()
                .filter(|l| l.starts_with("SEQUENCE"))
                .count();
            for _ in 0..5 {
                r.tick().unwrap();
            }
            assert_eq!(
                count,
                r.shared
                    .status_snapshot()
                    .logs
                    .iter()
                    .filter(|l| l.starts_with("SEQUENCE"))
                    .count()
            );
        }
    }
    #[test]
    fn arm_only_recipe_does_not_require_ee_and_timeout_cancels() {
        let mut r = super::super::tests::screen_runtime();
        let mut config = r.shared.sequence_config();
        config.prepare = vec![Step {
            name: "待機".into(),
            actions: vec![],
            wait_seconds: 1.0,
        }];
        config.timeout_seconds = 0.1;
        r.shared.set_sequence_config(config);
        start(&mut r, Stage::Prepare, Side::Left);
        let now = Instant::now();
        r.tick_at(now).unwrap();
        r.tick_at(now + Duration::from_secs(1)).unwrap();
        assert!(r.sequence.is_none());
        assert!(r.error.contains("設定時間内"));
    }
    #[test]
    fn ee_fault_cancels_sequence_and_holds_arm_at_current_position() {
        let mut r = runtime();
        start(&mut r, Stage::All, Side::Left);
        r.tick().unwrap();
        r.fault_ee("test error".into());
        assert!(r.sequence.is_none());
        assert!(r.drive.running());
        assert!(r.ee.targets.is_empty());
        assert!(
            r.shared
                .status_snapshot()
                .sequence
                .message
                .contains("EE異常")
        );
    }
    #[test]
    fn completion_restores_manual_mode_and_accepts_manual_arm_and_ee() {
        let mut r = runtime();
        let mut now = Instant::now();
        start(&mut r, Stage::Pick, Side::Left);
        finish(&mut r, &mut now);
        assert!(r.screen_control);
        r.request(
            &Request {
                axis: Some("r".into()),
                value: Some(0.5),
                ..Request::new("input")
            },
            true,
        )
        .unwrap();
        r.tick().unwrap();
        r.request(
            &Request {
                text: Some("[targets]\nee_grip_1=1500".into()),
                ..Request::new("ee")
            },
            true,
        )
        .unwrap();
        assert_eq!(r.ee.targets["ee_grip_1"], 611.0);
        assert!(!r.ee_targets_reached(&BTreeMap::from([("ee_grip_1".into(), 1500.0)])));
        r.screen_control = false;
        r.gamepad_name = "test pad".into();
        start(&mut r, Stage::Prepare, Side::Left);
        assert!(r.screen_control);
        finish(&mut r, &mut now);
        assert!(!r.screen_control);
        let mut input = ControllerState::default();
        input.axes[r.cfg.machine.axes[0].input_axis.unwrap()] = 0.5;
        r.read_pad(input.clone(), Instant::now()).unwrap();
        assert_eq!(r.manual_input.axes, input.axes);
    }
}

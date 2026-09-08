use super::*;
use crate::{diagnostics::individual::Target, machine::ee};
use std::collections::BTreeMap;
#[derive(Default)]
pub(super) struct Control {
    pub targets: BTreeMap<String, f32>,
    sent: BTreeMap<String, (f32, Instant)>,
    started: Option<Instant>,
    tick: Option<Instant>,
    poll: Option<Instant>,
}
impl Runtime {
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
        let axes = ee::axes(&self.cfg.machine);
        let mut lines = Vec::new();
        for (name, value) in &input.targets {
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
            lines.extend(axis.commands(*value)?.into_iter().take(
                if self.ee.targets.contains_key(name) {
                    1
                } else {
                    usize::MAX
                },
            ));
        }
        // 全対象の検証が完了するまで送信しない。送信途中失敗も停止対象に含める。
        for (name, value) in input.targets {
            self.ee.targets.insert(name.clone(), value);
            self.ee.sent.insert(name, (value, Instant::now()));
        }
        self.ee.started = Some(Instant::now());
        for line in lines {
            if let Err(error) = self.send(&line) {
                self.fault(error.to_string());
                return Err(error);
            }
        }
        Ok(Reply::accepted())
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
            let Some(previous) = self.ee.targets.get(&axis.name).copied() else {
                continue;
            };
            let value = if !self.authority.active() && !self.screen_control {
                if !self.pad.ee_armed {
                    continue;
                }
                axis.pad_value(&input)
            } else {
                axis.input_axis.map_or(0.0, |index| input.axes[index])
            };
            if value.abs() < 0.1 {
                continue;
            }
            let next = (previous
                + value
                    * axis.sign
                    * axis.speed
                    * dt
                    * if slow {
                        self.cfg.machine.slow_speed_percent * 0.01
                    } else {
                        1.0
                    })
            .clamp(axis.min, axis.max);
            if self.ee.sent.get(&axis.name).is_none_or(|(value, sent)| {
                *value != next.round() && now.duration_since(*sent) >= Duration::from_millis(50)
            }) {
                // 初回以外は目標だけを更新し、ENABLE/RUNを繰り返さない。
                for line in axis.commands(next.round())?.into_iter().take(1) {
                    self.send(&line)?;
                }
                self.ee.sent.insert(axis.name.clone(), (next.round(), now));
            }
            self.ee.targets.insert(axis.name.clone(), next);
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
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert_eq!(r.ee.targets["ee_fold"], 1500.0);
        let pwm: Vec<_> = r
            .shared
            .status_snapshot()
            .logs
            .into_iter()
            .filter(|l| l.contains("CAN 2 768"))
            .collect();
        assert!(pwm[pwm.len() - 2].contains("0101000005DC0000"));
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

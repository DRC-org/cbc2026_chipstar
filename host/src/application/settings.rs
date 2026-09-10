use crate::{
    machine::{MachineProfile, PARAMETER_NAMES, ParameterMap},
    protocol::{
        dcmd,
        parameters::{ParameterBoard, ParameterKey, ParameterValue},
        serial_svmd, svmd,
    },
};
use anyhow::{Result, bail};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

pub struct Settings {
    pending: VecDeque<ParameterValue>,
    sent: Option<Instant>,
    pub confirmed: usize,
    pub expected: usize,
}
impl Settings {
    pub fn new(profile: &MachineProfile) -> Self {
        let pending: VecDeque<_> = parameter_plan(profile).into();
        Self {
            expected: pending.len(),
            pending,
            sent: None,
            confirmed: 0,
        }
    }
    pub fn ready(&self) -> bool {
        self.pending.is_empty()
    }
    pub fn poll_command(&mut self) -> Result<Option<String>> {
        if self
            .sent
            .is_some_and(|t| t.elapsed() > Duration::from_secs(1))
        {
            bail!("設定の反映応答がありません。接続とFWを確認して再適用してください");
        }
        if self.sent.is_some() {
            return Ok(None);
        }
        if let Some(parameter) = self.pending.front() {
            self.sent = Some(Instant::now());
            return Ok(Some(parameter.command()));
        }
        Ok(None)
    }
    pub fn receive(&mut self, line: &str) -> Result<()> {
        let Some(ParameterValue { key, value }) = ParameterValue::parse_reply(line) else {
            return Ok(());
        };
        let Some(expected) = self.pending.front() else {
            return Ok(());
        };
        if self.sent.is_none() || key != expected.key {
            return Ok(());
        }
        if !value.is_finite()
            || (value - expected.value).abs() > 0.0001_f32.max(expected.value.abs() * 0.00001)
        {
            let names: &[&str] = match key.board {
                ParameterBoard::Cctl => &PARAMETER_NAMES,
                ParameterBoard::Svmd => &svmd::PARAMETER_NAMES,
                ParameterBoard::Dcmd => &dcmd::PARAMETER_NAMES,
                ParameterBoard::SerialSvmd => &serial_svmd::PARAMETER_NAMES,
            };
            let name = names.get(usize::from(key.id)).copied().unwrap_or("unknown");
            bail!(
                "設定の応答値がPCと不一致です: {:?} / {} (ID {})、送信値 {}、応答値 {}",
                key.board,
                name,
                key.id,
                expected.value,
                value
            );
        }
        self.pending.pop_front();
        self.sent = None;
        self.confirmed += 1;
        Ok(())
    }
}
/// 検証済みプロファイルを送信順の設定値へ変換する。
/// 基板とIDを保持し、指令文字列の逆解析で応答先を推測しない。
pub fn parameter_plan(profile: &MachineProfile) -> Vec<ParameterValue> {
    let cctl_parameters = profile.cctl_parameters();
    let svmd_parameters = profile.effective_svmd_parameters();
    let boards: [(ParameterBoard, &ParameterMap, &[&str]); 4] = [
        (ParameterBoard::Cctl, &cctl_parameters, &PARAMETER_NAMES),
        (
            ParameterBoard::Svmd,
            &svmd_parameters,
            &svmd::PARAMETER_NAMES,
        ),
        (
            ParameterBoard::Dcmd,
            &profile.dcmd_parameters,
            &dcmd::PARAMETER_NAMES,
        ),
        (
            ParameterBoard::SerialSvmd,
            &profile.serial_svmd_parameters,
            &serial_svmd::PARAMETER_NAMES,
        ),
    ];
    boards
        .into_iter()
        .flat_map(|(board, values, names)| {
            values.iter().map(move |(name, value)| {
                let id = names
                    .iter()
                    .position(|entry| entry == name)
                    .expect("MachineProfile must be validated before synchronization")
                    as u8;
                // ASCII基板の小数5桁という送信精度を、照合値にも適用する。
                let value = if board == ParameterBoard::Cctl {
                    format!("{value:.5}").parse().expect("formatted float")
                } else {
                    *value
                };
                ParameterValue {
                    key: ParameterKey { board, id },
                    value,
                }
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn small_gain_requires_precise_reply_and_reports_both_values() {
        let mut profile = MachineProfile::embedded().unwrap();
        profile.parameters.clear();
        profile.axes.clear();
        profile.pwm_servos.clear();
        profile.parameters.insert("m3508_vel_ki".into(), 0.0005);
        let mut sync = Settings::new(&profile);
        assert_eq!(sync.poll_command().unwrap().unwrap(), "PARAM 5 0.00050");
        let error = sync.receive("PARAM 5 0.001").unwrap_err().to_string();
        for detail in [
            "Cctl",
            "m3508_vel_ki",
            "ID 5",
            "送信値 0.0005",
            "応答値 0.001",
        ] {
            assert!(error.contains(detail), "{error}");
        }
        assert_eq!(sync.confirmed, 0);
        assert!(!sync.ready());
        sync.receive("PARAM 5 0.00050").unwrap();
        assert!(sync.ready());
    }

    #[test]
    fn unrelated_ack_cannot_confirm_a_setting() {
        let profile = MachineProfile::parse("protocol_version=1\n[parameters]\nel05_limit_spd=1.0\n[[axes]]\nname='r'\nunit='mm'\nslot=0\nspeed_per_second=1.0\nnative_per_unit=1.0\nminimum=0.0\nmaximum=10.0\ninitial=0.0").unwrap();
        let mut sync = Settings::new(&profile);
        sync.receive("PARAM 9 1.0").unwrap();
        assert!(!sync.ready());
        assert!(sync.poll_command().unwrap().is_some());
        sync.receive("OK").unwrap();
        assert!(!sync.ready());
        assert!(sync.receive("PARAM 9 2.0").is_err());
        sync.receive("PARAM 9 1.0").unwrap();
        assert!(sync.ready());
    }

    #[test]
    fn sends_named_parameters_as_numeric_ids() {
        let mut profile = MachineProfile::embedded().unwrap();
        profile.parameters.insert("m3508_vel_kp".into(), 0.9);
        profile.parameters.insert("watchdog_ms".into(), 300.0);
        let lines = parameter_plan(&profile)
            .iter()
            .map(ParameterValue::command)
            .collect::<Vec<_>>();
        assert!(lines.contains(&"PARAM 4 0.90000".to_owned()));
        assert!(lines.contains(&"PARAM 30 300.00000".to_owned()));
    }

    #[test]
    fn derives_motor_speed_limits_from_axis_speed() {
        let mut profile = MachineProfile::embedded().unwrap();
        for (slot, speed, native) in [(0, 100.0, -0.04), (1, 40.0, 132.0), (2, 100.0, -96.0)] {
            let axis = profile
                .axes
                .iter_mut()
                .find(|axis| axis.slot == slot)
                .unwrap();
            axis.speed_per_second = speed;
            axis.native_per_unit = native;
        }
        let lines = parameter_plan(&profile)
            .iter()
            .map(ParameterValue::command)
            .collect::<Vec<_>>();
        assert!(lines.contains(&"PARAM 9 4.00000".to_owned()));
        assert!(lines.contains(&"PARAM 3 880.00000".to_owned()));
        assert!(lines.contains(&"PARAM 36 1600.00000".to_owned()));
    }

    #[test]
    fn sends_can_board_parameters_through_the_gateway() {
        let mut profile = MachineProfile::embedded().unwrap();
        profile.dcmd_parameters.insert("max_duty".into(), 1000.0);
        profile
            .serial_svmd_parameters
            .insert("servo_baud".into(), 1000000.0);
        let lines = parameter_plan(&profile)
            .iter()
            .map(ParameterValue::command)
            .collect::<Vec<_>>();
        // DCMD id 0 (max_duty) = 1000.0f = 0x447A0000
        assert!(lines.contains(&"CAN 2 784 01070000447A0000".to_owned()));
        // serial_svmd id 0 (servo_baud) = 1000000.0f = 0x49742400
        assert!(lines.contains(&"CAN 2 800 0109000049742400".to_owned()));
    }

    #[test]
    fn identical_parameter_ids_on_different_boards_do_not_cross_confirm() {
        let mut profile = MachineProfile::embedded().unwrap();
        profile.parameters.clear();
        profile.axes.clear();
        profile.pwm_servos.clear();
        profile.svmd_parameters.insert("min_pulse_us".into(), 500.0);
        profile.dcmd_parameters.insert("max_duty".into(), 500.0);
        profile
            .serial_svmd_parameters
            .insert("servo_baud".into(), 1_000_000.0);
        profile.validate().unwrap();
        let mut sync = Settings::new(&profile);
        assert_eq!(sync.expected, 3);
        assert_eq!(
            sync.poll_command().unwrap().unwrap(),
            "CAN 2 768 0104000043FA0000"
        );
        sync.receive("CAN_RX bus=2 id=788 data=0100000043FA0000")
            .unwrap();
        assert_eq!(sync.confirmed, 0);
        assert!(sync.poll_command().unwrap().is_none());
        sync.receive("CAN_RX bus=2 id=772 data=0100000043FA0000")
            .unwrap();
        assert_eq!(
            sync.poll_command().unwrap().unwrap(),
            "CAN 2 784 0107000043FA0000"
        );
        sync.receive("CAN_RX bus=2 id=772 data=0100000043FA0000")
            .unwrap();
        assert_eq!(sync.confirmed, 1);
        sync.receive("CAN_RX bus=2 id=788 data=0100000043FA0000")
            .unwrap();
        assert_eq!(
            sync.poll_command().unwrap().unwrap(),
            "CAN 2 800 0109000049742400"
        );
        assert!(
            sync.receive("CAN_RX bus=2 id=804 data=010000007FC00000")
                .is_err()
        );
        sync.receive("CAN_RX bus=2 id=804 data=0100000049742400")
            .unwrap();
        assert!(sync.ready());
        assert_eq!(sync.confirmed, 3);
    }

    #[test]
    fn missing_readback_times_out_without_sending_the_next_setting() {
        let mut sync = Settings::new(&MachineProfile::embedded().unwrap());
        assert!(sync.poll_command().unwrap().is_some());
        sync.sent = Some(Instant::now() - Duration::from_secs(2));
        assert!(sync.poll_command().is_err());
        assert_eq!(sync.confirmed, 0);
        assert!(!sync.ready());
    }
}

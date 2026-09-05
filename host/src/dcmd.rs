//! DCMD v1: signed duty in permille through cctl FDCAN2.
use anyhow::{Result, bail};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct MotorProfile {
    pub name: String,
    pub channel: u8,
    pub input_axis: usize,
    pub input_sign: f32,
    pub maximum_duty: u16,
}

pub fn validate(motors: &[MotorProfile]) -> Result<()> {
    let mut mask = 0u8;
    for motor in motors {
        if motor.channel > 1
            || mask & (1 << motor.channel) != 0
            || motor.input_axis >= 6
            || motor.input_sign.abs() != 1.0
            || motor.maximum_duty > 900
            || motor.name.is_empty()
        {
            bail!("DCMDのchannel、入力、Duty上限が不正です");
        }
        mask |= 1 << motor.channel;
    }
    Ok(())
}

pub fn line(op: u8, channel: u8, duty: i16) -> String {
    let bytes = duty.to_be_bytes();
    format!(
        "CAN 2 784 01{op:02X}{channel:02X}00{:02X}{:02X}0000",
        bytes[0], bytes[1]
    )
}

pub fn targets(motors: &[MotorProfile], axes: &[f32; 6]) -> Vec<String> {
    motors
        .iter()
        .map(|motor| {
            let input = axes[motor.input_axis];
            let input = if !input.is_finite() || input.abs() < 0.1 {
                0.0
            } else {
                input.clamp(-1.0, 1.0)
            };
            line(
                4,
                motor.channel,
                (input * motor.input_sign * f32::from(motor.maximum_duty)).round() as i16,
            )
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct Status {
    pub result: u8,
    pub mode: u8,
    pub enabled: u8,
    pub duty: [i16; 2],
}
pub fn parse_status(line: &str) -> Option<Status> {
    let mut fields = line.split_whitespace();
    if fields.next()? != "CAN_RX" {
        return None;
    }
    let mut bus = None;
    let mut id = None;
    let mut data = None;
    for field in fields {
        let (key, value) = field.split_once('=')?;
        match key {
            "bus" => bus = Some(value),
            "id" => id = Some(value),
            "data" => data = Some(value),
            _ => {}
        }
    }
    if bus != Some("2") || id != Some("785") {
        return None;
    }
    let data = data?;
    if data.len() != 16 || !data.is_ascii() {
        return None;
    }
    let mut bytes = [0u8; 8];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&data[i * 2..i * 2 + 2], 16).ok()?;
    }
    if bytes[0] != 1 || bytes[1] > 2 || bytes[2] > 2 || bytes[3] > 3 {
        return None;
    }
    Some(Status {
        result: bytes[1],
        mode: bytes[2],
        enabled: bytes[3],
        duty: [
            i16::from_be_bytes([bytes[4], bytes[5]]),
            i16::from_be_bytes([bytes[6], bytes[7]]),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encodes_negative_duty_and_decodes_status() {
        assert_eq!(line(4, 1, -900), "CAN 2 784 01040100FC7C0000");
        let status = parse_status("CAN_RX bus=2 id=785 data=010001030064FC7C").unwrap();
        assert_eq!(status.duty, [100, -900]);
        assert!(parse_status("CAN_RX bus=2 id=769 data=010001030064FC7C").is_none());
    }

    #[test]
    fn profile_limits_input_and_rejects_duplicate_channels() {
        let mut profile =
            crate::machine::MachineProfile::parse(include_str!("../config/dcmd.toml")).unwrap();
        assert_eq!(
            targets(&profile.dc_motors, &[0.0, -2.0, 0.0, 0.05, 0.0, 0.0]),
            vec![line(4, 0, -100), line(4, 1, 0)]
        );
        profile.dc_motors[1].channel = 0;
        assert!(validate(&profile.dc_motors).is_err());
    }
}

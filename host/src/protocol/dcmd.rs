//! DCMD v1: signed duty in permille through cctl FDCAN2.

pub fn line(op: u8, channel: u8, duty: i16) -> String {
    let bytes = duty.to_be_bytes();
    format!(
        "CAN 2 784 01{op:02X}{channel:02X}00{:02X}{:02X}0000",
        bytes[0], bytes[1]
    )
}

#[derive(Clone, Debug)]
pub struct Status {
    pub result: u8,
    pub mode: u8,
    pub enabled: u8,
    pub duty: [i16; 2],
}
fn payload(line: &str, expected_id: &str) -> Option<[u8; 8]> {
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
    if bus != Some("2") || id != Some(expected_id) {
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
    if bytes[0] != 1 {
        return None;
    }
    Some(bytes)
}

#[derive(Clone, Debug)]
pub struct EncoderStatus {
    pub count: i32,
    pub index_count: u16,
}

pub fn parse_encoder(line: &str) -> Option<EncoderStatus> {
    let bytes = payload(line, "786")?;
    if bytes[1] != 1 {
        return None;
    }
    Some(EncoderStatus {
        count: i32::from_be_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]),
        index_count: u16::from_be_bytes([bytes[6], bytes[7]]),
    })
}

pub fn parse_status(line: &str) -> Option<Status> {
    let bytes = payload(line, "785")?;
    if bytes[1] > 2 || bytes[2] > 2 || bytes[3] > 1 || bytes[6] != 0 || bytes[7] != 0 {
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

/// DCMDの実行時パラメータ。名前とidの対応は device_protocol.md の表に従う。
pub const PARAMETER_NAMES: [&str; 6] = [
    "max_duty",
    "ramp_interval_ms",
    "ramp_step",
    "reverse_brake_ms",
    "watchdog_ms",
    "pwm_frequency_hz",
];

// dcmd/src/domain/parameters.hpp の起動時既定値と受理範囲。
pub const PARAMETER_DEFAULTS: [f32; 6] = [900.0, 10.0, 1.0, 2000.0, 250.0, 20000.0];
pub const PARAMETER_RANGES: [(f32, f32); 6] = [
    (0.0, 1000.0),
    (1.0, 10000.0),
    (1.0, 1000.0),
    (0.0, 60000.0),
    (1.0, 60000.0),
    (500.0, 100000.0),
];

pub fn max_duty(parameters: &std::collections::BTreeMap<String, f32>) -> f32 {
    parameters
        .get("max_duty")
        .copied()
        .unwrap_or(PARAMETER_DEFAULTS[0])
        .trunc()
}

/// `PARAM SET` のCANフレーム行。byte 4..7 に float32 を big endian で載せる。
pub fn parameter_line(id: u8, value: f32) -> String {
    let bytes = value.to_be_bytes();
    format!(
        "CAN 2 784 0107{id:02X}00{:02X}{:02X}{:02X}{:02X}",
        bytes[0], bytes[1], bytes[2], bytes[3]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encodes_negative_duty_and_decodes_status() {
        assert_eq!(line(4, 1, -900), "CAN 2 784 01040100FC7C0000");
        let status = parse_status("CAN_RX bus=2 id=785 data=0100010100640000").unwrap();
        assert_eq!(status.duty, [100, 0]);
        assert!(parse_status("CAN_RX bus=2 id=769 data=010001030064FC7C").is_none());
    }

    #[test]
    fn decodes_signed_encoder_and_index() {
        let encoder = parse_encoder("CAN_RX bus=2 id=786 data=0101FFFFFFFE0003").unwrap();
        assert_eq!(encoder.count, -2);
        assert_eq!(encoder.index_count, 3);
        assert!(parse_encoder("CAN_RX bus=2 id=786 data=0100FFFFFFFE0003").is_none());
    }
}

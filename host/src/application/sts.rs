//! STS管理のGUI/API共通要求と、CAN拡張操作の符号化。
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub id: u8,
    pub mode: u8,
    pub value: i16,
    pub speed: u16,
    pub acceleration: u8,
}
impl Default for Target {
    fn default() -> Self {
        Self {
            id: 1,
            mode: 0,
            value: 2048,
            speed: 100,
            acceleration: 20,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub wait_ms: u32,
    pub targets: Vec<Target>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Scan {
        first: u8,
        last: u8,
    },
    Read {
        id: u8,
        address: u8,
        width: u8,
    },
    Configure {
        id: u8,
        address: u8,
        width: u8,
        value: u16,
        single_servo: bool,
    },
    Monitor {
        ids: Vec<u8>,
    },
    Move {
        targets: Vec<Target>,
    },
    Sequence {
        steps: Vec<Step>,
    },
    Renew,
}
pub fn validate_targets(targets: &[Target]) -> Result<()> {
    ensure!(
        !targets.is_empty() && targets.len() <= 16,
        "同時指令は1〜16台です"
    );
    let mut seen = std::collections::BTreeSet::new();
    for t in targets {
        ensure!(
            (1..=253).contains(&t.id) && seen.insert(t.id),
            "IDは1〜253、重複不可です"
        );
        ensure!(
            t.acceleration <= 254 && t.speed <= 1000,
            "速度・加速度が範囲外です"
        );
        ensure!(
            match t.mode {
                0 => (-28672..=28672).contains(&t.value),
                1 => (-1000..=1000).contains(&t.value),
                3 => (-28672..=28672).contains(&t.value),
                _ => false,
            },
            "位置・速度・相対ステップの値またはモードが不正です"
        );
    }
    Ok(())
}
pub fn signed(value: i16) -> u16 {
    value.unsigned_abs() | if value < 0 { 0x8000 } else { 0 }
}
pub fn decode_signed(value: u16, bit: u8) -> i32 {
    let magnitude = i32::from(value & ((1 << bit) - 1));
    if value & (1 << bit) != 0 {
        -magnitude
    } else {
        magnitude
    }
}
pub fn line(data: [u8; 8]) -> String {
    format!(
        "CAN 2 800 {}",
        data.iter().map(|b| format!("{b:02X}")).collect::<String>()
    )
}
pub fn frame(text: &str, id: u16) -> Option<[u8; 8]> {
    let data = text.strip_prefix(&format!("CAN_RX bus=2 id={id} data="))?;
    if data.len() != 16 || !data.is_ascii() {
        return None;
    }
    let mut bytes = [0; 8];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&data[i * 2..i * 2 + 2], 16).ok()?;
    }
    (bytes[0] == 1).then_some(bytes)
}
#[derive(Clone, Default, Serialize)]
pub struct Sample {
    pub elapsed_ms: u64,
    pub id: u8,
    pub position: i32,
    pub speed: i32,
    pub load: i32,
    pub voltage: f32,
    pub temperature: u8,
    pub current_raw: i32,
    pub current_ma: f32,
    pub moving: bool,
}
#[derive(Clone, Default, Serialize)]
pub struct Status {
    pub elapsed_ms: u64,
    pub active: bool,
    pub busy: bool,
    pub message: String,
    pub discovered: Vec<u8>,
    pub samples: std::collections::VecDeque<Sample>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wheel_and_step_use_sign_magnitude_and_reject_duplicates() {
        assert_eq!(signed(-100), 0x8064);
        assert_eq!(decode_signed(signed(-100), 15), -100);
        let t = Target::default();
        assert!(validate_targets(&[t.clone(), t]).is_err());
        assert!(
            validate_targets(&[Target {
                mode: 3,
                value: -28672,
                ..Target::default()
            }])
            .is_ok()
        );
        assert!(
            validate_targets(&[Target {
                mode: 0,
                value: -28673,
                ..Target::default()
            }])
            .is_err()
        );
    }
}

//! ボーナスハンドの機体固有設定。

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

pub const AMT102_PPR_VALUES: [u16; 16] = [
    48, 96, 100, 125, 192, 200, 250, 256, 384, 400, 500, 512, 800, 1000, 1024, 2048,
];

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct BoxProfile {
    pub name: String,
    /// セッティングで登録した受け渡し位置からのAMT102-V相対カウント。
    pub offset_counts: i32,
}

/// ボックス選択軸の受け渡し位置側に置く任意のリミットスイッチ。
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
pub struct HandoffLimit {
    /// DCMDの接点bit位置。SW1=0, SW2=1, SW3=2。
    pub input: u8,
    /// 受け渡し位置へ近づくエンコーダ方向。`1`または`-1`。
    pub direction: i8,
    /// B接点（常閉）配線ならtrue。断線時も到達として扱う。
    #[serde(default = "default_normally_closed")]
    pub normally_closed: bool,
}

impl HandoffLimit {
    pub fn reached(&self, contacts: u8) -> bool {
        let closed = contacts & (1 << self.input) != 0;
        closed != self.normally_closed
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct BonusProfile {
    #[serde(default)]
    pub enabled: bool,
    pub selector_motor: String,
    pub lid_servo: String,
    pub align_servo: String,
    pub selector_duty: u16,
    pub selector_slow_duty: u16,
    pub selector_slow_zone_counts: i32,
    pub selector_tolerance_counts: i32,
    /// AMT102-VのDIPで選んだ分解能。仕様書上のPPRで、DCMDカウントはこの4倍。
    #[serde(default = "default_encoder_ppr")]
    pub encoder_ppr: u16,
    /// ラックのモジュール。M1なら1.0。
    #[serde(default = "default_pinion_module_mm")]
    pub pinion_module_mm: f32,
    /// ラックを駆動するピニオンの歯数。
    #[serde(default = "default_pinion_teeth")]
    pub pinion_teeth: u16,
    /// 未指定なら、従来どおりAMT102-Vの登録位置だけで復帰する。
    #[serde(default)]
    pub handoff_limit: Option<HandoffLimit>,
    pub lid_closed_position: i16,
    pub lid_open_position: i16,
    pub align_home_position: i16,
    pub align_position: i16,
    #[serde(default = "default_servo_tolerance")]
    pub servo_tolerance_counts: i32,
    #[serde(default = "default_dwell_ms")]
    pub dwell_ms: u64,
    #[serde(default = "default_cycle_timeout_ms")]
    pub cycle_timeout_ms: u64,
    #[serde(default = "default_capacity")]
    pub capacity: u8,
    #[serde(default)]
    pub boxes: Vec<BoxProfile>,
}

fn default_servo_tolerance() -> i32 {
    20
}
fn default_dwell_ms() -> u64 {
    300
}
fn default_cycle_timeout_ms() -> u64 {
    20_000
}
fn default_capacity() -> u8 {
    6
}
fn default_normally_closed() -> bool {
    true
}
fn default_encoder_ppr() -> u16 {
    2048
}
fn default_pinion_module_mm() -> f32 {
    1.0
}
fn default_pinion_teeth() -> u16 {
    20
}

impl BonusProfile {
    pub fn encoder_counts_per_revolution(&self) -> i32 {
        i32::from(self.encoder_ppr) * 4
    }

    pub fn travel_mm_per_revolution(&self) -> f32 {
        std::f32::consts::PI * self.pinion_module_mm * f32::from(self.pinion_teeth)
    }

    pub fn counts_to_mm(&self, counts: i32) -> f32 {
        counts as f32 * self.travel_mm_per_revolution()
            / self.encoder_counts_per_revolution() as f32
    }

    pub fn mm_to_counts(&self, mm: f32) -> Option<i32> {
        let counts =
            mm * self.encoder_counts_per_revolution() as f32 / self.travel_mm_per_revolution();
        (counts.is_finite() && counts >= i32::MIN as f32 && counts <= i32::MAX as f32)
            .then(|| counts.round() as i32)
    }

    pub fn validate(&self) -> Result<()> {
        if self.selector_motor.is_empty()
            || self.lid_servo.is_empty()
            || self.align_servo.is_empty()
            || self.lid_servo == self.align_servo
            || self.selector_duty == 0
            || self.selector_duty > 1000
            || self.selector_slow_duty == 0
            || self.selector_slow_duty > self.selector_duty
            || self.selector_slow_zone_counts <= self.selector_tolerance_counts
            || self.selector_tolerance_counts <= 0
            || !AMT102_PPR_VALUES.contains(&self.encoder_ppr)
            || !self.pinion_module_mm.is_finite()
            || self.pinion_module_mm <= 0.0
            || self.pinion_teeth == 0
            || self.servo_tolerance_counts <= 0
            || self.capacity == 0
            || self.cycle_timeout_ms < 1000
            || self.boxes.is_empty()
            || self.boxes.iter().any(|b| b.name.is_empty())
        {
            bail!(
                "ボーナスハンド設定の名前、Duty、エンコーダ、ラック、許容差、本数またはボックスが不正です"
            );
        }
        if self
            .handoff_limit
            .is_some_and(|limit| limit.input >= 3 || !matches!(limit.direction, -1 | 1))
        {
            bail!("bonus.handoff_limitはinput=0..2、direction=-1または1で指定してください");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_handoff_limit_validates_input_and_direction() {
        let mut profile = crate::machine::MachineProfile::embedded()
            .unwrap()
            .bonus
            .unwrap();
        assert!(profile.validate().is_ok());
        profile.handoff_limit = Some(HandoffLimit {
            input: 2,
            direction: -1,
            normally_closed: false,
        });
        assert!(profile.validate().is_ok());
        profile.handoff_limit.as_mut().unwrap().input = 3;
        assert!(profile.validate().is_err());
        profile.handoff_limit.as_mut().unwrap().input = 0;
        profile.handoff_limit.as_mut().unwrap().direction = 0;
        assert!(profile.validate().is_err());
    }

    #[test]
    fn handoff_limit_supports_no_and_nc_contacts() {
        let no = HandoffLimit {
            input: 1,
            direction: 1,
            normally_closed: false,
        };
        assert!(!no.reached(0));
        assert!(no.reached(0b10));
        let nc = HandoffLimit {
            normally_closed: true,
            ..no
        };
        assert!(!nc.reached(0b10));
        assert!(nc.reached(0));
    }

    #[test]
    fn m1_twenty_tooth_pinion_converts_amt102_counts_to_mm() {
        let mut profile = crate::machine::MachineProfile::embedded()
            .unwrap()
            .bonus
            .unwrap();
        assert_eq!(profile.encoder_ppr, 2048);
        assert_eq!(profile.encoder_counts_per_revolution(), 8192);
        assert!((profile.travel_mm_per_revolution() - 20.0 * std::f32::consts::PI).abs() < 1e-5);
        assert!((profile.counts_to_mm(8192) - 20.0 * std::f32::consts::PI).abs() < 1e-5);
        assert_eq!(
            profile.mm_to_counts(20.0 * std::f32::consts::PI),
            Some(8192)
        );

        profile.encoder_ppr = 123;
        assert!(profile.validate().is_err());
    }
}

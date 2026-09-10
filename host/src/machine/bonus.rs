//! ボーナスハンドの機体固有設定。

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

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
    /// 受け渡し位置へ近づくDutyの符号。`1`または`-1`。
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

impl BonusProfile {
    pub fn validate(&self) -> Result<()> {
        if self.selector_motor.is_empty()
            || self.lid_servo.is_empty()
            || self.align_servo.is_empty()
            || self.lid_servo == self.align_servo
            || self.selector_duty == 0
            || self.selector_duty > 900
            || self.selector_slow_duty == 0
            || self.selector_slow_duty > self.selector_duty
            || self.selector_slow_zone_counts <= self.selector_tolerance_counts
            || self.selector_tolerance_counts <= 0
            || self.servo_tolerance_counts <= 0
            || self.capacity == 0
            || self.cycle_timeout_ms < 1000
            || self.boxes.is_empty()
            || self.boxes.iter().any(|b| b.name.is_empty())
        {
            bail!("ボーナスハンド設定の名前、Duty、許容差、本数またはボックスが不正です");
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
}

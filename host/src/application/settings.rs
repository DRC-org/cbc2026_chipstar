use crate::machine::MachineProfile;
use anyhow::{Result, bail};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

pub struct Settings {
    pending: VecDeque<(String, String, f32)>,
    sent: Option<Instant>,
    pub confirmed: usize,
    pub expected: usize,
}
impl Settings {
    pub fn new(profile: &MachineProfile) -> Self {
        let pending: VecDeque<_> = profile
            .parameter_lines()
            .into_iter()
            .filter_map(|line| {
                if let Some((key, value)) = parameter(&line) {
                    Some((line, key, value))
                } else {
                    let words: Vec<_> = line.split_whitespace().collect();
                    let id: u16 = words.get(2)?.parse().ok()?;
                    let data = *words.get(3)?;
                    let key = format!("{}:{}", id + 4, u8::from_str_radix(&data[4..6], 16).ok()?);
                    let value = f32::from_bits(u32::from_str_radix(&data[8..16], 16).ok()?);
                    Some((line, key, value))
                }
            })
            .collect();
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
    pub fn next(&mut self) -> Result<Option<String>> {
        if self
            .sent
            .is_some_and(|t| t.elapsed() > Duration::from_secs(1))
        {
            bail!("設定の反映応答がありません。接続とFWを確認して再適用してください");
        }
        if self.sent.is_some() {
            return Ok(None);
        }
        if let Some((line, _, _)) = self.pending.front() {
            self.sent = Some(Instant::now());
            return Ok(Some(line.clone()));
        }
        Ok(None)
    }
    pub fn receive(&mut self, line: &str) -> Result<()> {
        let Some((key, value)) = parameter(line) else {
            return Ok(());
        };
        let Some((_, expected_key, expected_value)) = self.pending.front() else {
            return Ok(());
        };
        if self.sent.is_none() || &key != expected_key {
            return Ok(());
        }
        if !value.is_finite()
            || (value - expected_value).abs() > 0.0001_f32.max(expected_value.abs() * 0.00001)
        {
            bail!("設定の応答値がPCと不一致です: {key}");
        }
        self.pending.pop_front();
        self.sent = None;
        self.confirmed += 1;
        Ok(())
    }
}
fn parameter(line: &str) -> Option<(String, f32)> {
    if let Some(body) = line.strip_prefix("PARAM ") {
        let mut words = body.split_whitespace();
        let id: u8 = words.next()?.parse().ok()?;
        let value = words.next()?.parse().ok()?;
        if words.next().is_some() {
            return None;
        }
        return Some((format!("cctl:{id}"), value));
    }
    let body = line.strip_prefix("CAN_RX bus=2 id=")?;
    let (id, data) = body.split_once(" data=")?;
    if !["772", "788", "804"].contains(&id)
        || data.len() != 16
        || !data.is_ascii()
        || !data.starts_with("01")
    {
        return None;
    }
    Some((
        format!("{id}:{}", u8::from_str_radix(&data[2..4], 16).ok()?),
        f32::from_bits(u32::from_str_radix(&data[8..16], 16).ok()?),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unrelated_ack_cannot_confirm_a_setting() {
        let profile = MachineProfile::parse("protocol_version=1\n[parameters]\nel05_limit_spd=1.0\n[[axes]]\nname='r'\nunit='mm'\nslot=0\nspeed_per_second=1.0\nnative_per_unit=1.0\nminimum=0.0\nmaximum=10.0\ninitial=0.0").unwrap();
        let mut sync = Settings::new(&profile);
        sync.receive("PARAM 9 1.0").unwrap();
        assert!(!sync.ready());
        assert!(sync.next().unwrap().is_some());
        sync.receive("OK").unwrap();
        assert!(!sync.ready());
        assert!(sync.receive("PARAM 9 2.0").is_err());
        sync.receive("PARAM 9 1.0").unwrap();
        assert!(sync.ready());
    }
}

//! cctlが送る汎用slotテレメトリの解釈。

/// `Telemetry::error_bits` のうち cctl 自身が立てるもの。
/// 下位bitは各モータのドライバが返す値がそのまま入る。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunMode {
    Safe,
    Run,
    Stop,
}

impl RunMode {
    pub fn label(self) -> &'static str {
        match self {
            RunMode::Safe => "SAFE（待機）",
            RunMode::Run => "RUN（運転）",
            RunMode::Stop => "STOP（出力停止）",
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct SlotState {
    pub target: f32,
    pub measured: f32,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Telemetry {
    pub uptime_ms: u32,
    pub slots: [SlotState; 3],
    pub enabled_slots: u8,
    pub mode: RunMode,
    /// slotごとの異常bit。bit7=フィードバック途絶、bit6=過熱。
    pub error_bits: [u8; 3],
    /// SW1..SW3の10ms安定値。閉で1。`sw=`を持たないFWではNone。
    pub contacts: Option<u8>,
    /// モータのフィードバックが途絶えたslotのbit mask。
    pub stale_slots: u8,
    /// 使えるCANバス。bit0=FDCAN1（モータ）、bit1=FDCAN2（周辺基板）。
    /// バスオフや初期化失敗でbitが落ちる。
    pub buses: u8,
}

impl Telemetry {
    #[cfg(test)]
    pub fn slot_enabled(&self, slot: u8) -> bool {
        self.enabled_slots & (1 << slot) != 0
    }
}

fn parse_slot(text: &str) -> Option<SlotState> {
    let (target, measured) = text.split_once('/')?;
    let target: f32 = target.parse().ok()?;
    let measured: f32 = measured.parse().ok()?;
    if !target.is_finite() || !measured.is_finite() {
        return None;
    }
    Some(SlotState { target, measured })
}

fn parse_mode(text: &str) -> Option<RunMode> {
    match text {
        "SAFE" => Some(RunMode::Safe),
        "RUN" => Some(RunMode::Run),
        "STOP" => Some(RunMode::Stop),
        _ => None,
    }
}

pub fn parse_telemetry(line: &str) -> Option<Telemetry> {
    let mut tokens = line.split_whitespace();
    if tokens.next()? != "STATE" {
        return None;
    }

    let mut uptime_ms = None;
    let mut slots = [None; 3];
    let mut enabled_slots = None;
    let mut mode = None;
    let mut error_bits = None;
    let mut contacts = None;
    let mut stale_slots = 0;
    let mut buses = 3;
    for token in tokens {
        let (key, value) = token.split_once('=')?;
        match key {
            "t" => uptime_ms = Some(value.parse().ok()?),
            "mode" => mode = Some(parse_mode(value)?),
            "en" => enabled_slots = Some(value.parse().ok()?),
            "a0" => slots[0] = Some(parse_slot(value)?),
            "a1" => slots[1] = Some(parse_slot(value)?),
            "a2" => slots[2] = Some(parse_slot(value)?),
            "err" => {
                let mut bits = [0u8; 3];
                let mut fields = value.split(',');
                for slot in &mut bits {
                    *slot = u8::from_str_radix(fields.next()?, 16).ok()?;
                }
                if fields.next().is_some() {
                    return None;
                }
                error_bits = Some(bits);
            }
            "sw" => contacts = Some(value.parse().ok()?),
            "stale" => stale_slots = value.parse().ok()?,
            "can" => buses = value.parse().ok()?,
            _ => {}
        }
    }

    Some(Telemetry {
        uptime_ms: uptime_ms?,
        slots: [slots[0]?, slots[1]?, slots[2]?],
        enabled_slots: enabled_slots?,
        mode: mode?,
        error_bits: error_bits?,
        contacts,
        stale_slots,
        buses,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "STATE t=12345 mode=RUN en=7 a0=1.200/1.100 a1=-45.000/-44.200 a2=0.500/0.400 err=0A,00,03 sw=5 stale=2 can=3";

    #[test]
    fn parses_all_slots() {
        let telemetry = parse_telemetry(SAMPLE).unwrap();
        assert_eq!(telemetry.uptime_ms, 12345);
        assert_eq!(telemetry.slots[0].target, 1.2);
        assert_eq!(telemetry.slots[1].measured, -44.2);
        assert_eq!(telemetry.mode, RunMode::Run);
        assert_eq!(telemetry.error_bits, [0x0A, 0x00, 0x03]);
        assert_eq!(telemetry.contacts, Some(5));
        assert_eq!(telemetry.stale_slots, 2);
        assert_eq!(telemetry.buses, 3);
        assert!(telemetry.slot_enabled(2));
    }

    #[test]
    fn treats_missing_contacts_as_unknown_not_as_asserted() {
        let line = SAMPLE.strip_suffix(" sw=5 stale=2 can=3").unwrap();
        assert_eq!(parse_telemetry(line).unwrap().contacts, None);
    }

    #[test]
    fn rejects_incomplete_or_old_telemetry() {
        assert!(parse_telemetry("STATE t=1 mode=SAFE").is_none());
        assert!(parse_telemetry("ST t=1 r=0/0").is_none());
    }

    #[test]
    fn ignores_future_fields() {
        assert!(parse_telemetry(&format!("{SAMPLE} extra=1")).is_some());
    }
}

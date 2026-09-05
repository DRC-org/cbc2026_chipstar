//! cctlが送る汎用slotテレメトリの解釈。

pub mod slot_bit {
    pub const SLOT0: u8 = 1 << 0;
    pub const SLOT1: u8 = 1 << 1;
    pub const SLOT2: u8 = 1 << 2;
    pub const ALL: u8 = SLOT0 | SLOT1 | SLOT2;
}

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
            RunMode::Stop => "STOP（非常停止）",
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
    pub error_bits: u8,
    /// SW1..SW3の10ms安定値。閉で1。`sw=`を持たないFWではNone。
    pub contacts: Option<u8>,
}

impl Telemetry {
    pub fn slot_enabled(&self, slot: u8) -> bool {
        self.enabled_slots & (1 << slot) != 0
    }
}

fn parse_slot(text: &str) -> Option<SlotState> {
    let (target, measured) = text.split_once('/')?;
    Some(SlotState {
        target: target.parse().ok()?,
        measured: measured.parse().ok()?,
    })
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
    for token in tokens {
        let (key, value) = token.split_once('=')?;
        match key {
            "t" => uptime_ms = Some(value.parse().ok()?),
            "mode" => mode = Some(parse_mode(value)?),
            "en" => enabled_slots = Some(value.parse().ok()?),
            "a0" => slots[0] = Some(parse_slot(value)?),
            "a1" => slots[1] = Some(parse_slot(value)?),
            "a2" => slots[2] = Some(parse_slot(value)?),
            "err" => error_bits = Some(u8::from_str_radix(value, 16).ok()?),
            "sw" => contacts = Some(value.parse().ok()?),
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str =
        "STATE t=12345 mode=RUN en=7 a0=1.200/1.100 a1=-45.000/-44.200 a2=0.500/0.400 err=0A sw=5";

    #[test]
    fn parses_all_slots() {
        let telemetry = parse_telemetry(SAMPLE).unwrap();
        assert_eq!(telemetry.uptime_ms, 12345);
        assert_eq!(telemetry.slots[0].target, 1.2);
        assert_eq!(telemetry.slots[1].measured, -44.2);
        assert_eq!(telemetry.mode, RunMode::Run);
        assert_eq!(telemetry.error_bits, 0x0A);
        assert_eq!(telemetry.contacts, Some(5));
        assert!(telemetry.slot_enabled(2));
    }

    #[test]
    fn treats_missing_contacts_as_unknown_not_as_asserted() {
        let line = SAMPLE.strip_suffix(" sw=5").unwrap();
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

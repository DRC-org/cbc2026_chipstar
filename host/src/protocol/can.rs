//! cctlが通知するFDCANコントローラ診断の解釈。

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Diagnostics {
    pub bus: u8,
    pub started: bool,
    pub bus_off: bool,
    pub lec: u8,
    pub tec: u8,
    pub rec: u8,
    pub cel: u8,
    pub tx_failed: u32,
}

pub fn parse_diagnostics(line: &str) -> Option<Diagnostics> {
    let mut tokens = line.split_whitespace();
    if tokens.next()? != "CANSTAT" {
        return None;
    }
    let mut bus = None;
    let mut started = None;
    let mut bus_off = None;
    let mut lec = None;
    let mut tec = None;
    let mut rec = None;
    let mut cel = None;
    let mut tx_failed = None;
    for token in tokens {
        let (key, value) = token.split_once('=')?;
        match key {
            "bus" => bus = Some(value.parse().ok()?),
            "started" => started = Some(parse_bool(value)?),
            "busoff" => bus_off = Some(parse_bool(value)?),
            "lec" => lec = Some(value.parse().ok()?),
            "tec" => tec = Some(value.parse().ok()?),
            "rec" => rec = Some(value.parse().ok()?),
            "cel" => cel = Some(value.parse().ok()?),
            "tx_failed" => tx_failed = Some(value.parse().ok()?),
            _ => {}
        }
    }
    let bus = bus?;
    if !(1..=2).contains(&bus) {
        return None;
    }
    Some(Diagnostics {
        bus,
        started: started?,
        bus_off: bus_off?,
        lec: lec?,
        tec: tec?,
        rec: rec?,
        cel: cel?,
        tx_failed: tx_failed?,
    })
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_complete_can_diagnostics() {
        assert_eq!(
            parse_diagnostics(
                "CANSTAT bus=1 started=1 busoff=1 lec=7 tec=248 rec=0 cel=255 tx_failed=96458"
            ),
            Some(Diagnostics {
                bus: 1,
                started: true,
                bus_off: true,
                lec: 7,
                tec: 248,
                rec: 0,
                cel: 255,
                tx_failed: 96458,
            })
        );
    }

    #[test]
    fn rejects_incomplete_and_unknown_bus_diagnostics() {
        assert!(parse_diagnostics("CANSTAT bus=1 started=1").is_none());
        assert!(
            parse_diagnostics(
                "CANSTAT bus=3 started=1 busoff=0 lec=0 tec=0 rec=0 cel=0 tx_failed=0"
            )
            .is_none()
        );
        assert!(
            parse_diagnostics("STATE bus=1 started=1 busoff=0 lec=0 tec=0 rec=0 cel=0 tx_failed=0")
                .is_none()
        );
    }
}

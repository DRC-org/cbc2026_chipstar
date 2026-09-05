//! 汎用FWが返す能力通知の解釈。

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DeviceInfo {
    pub protocol: u8,
    pub board: String,
    pub slots: u8,
    pub watchdog_ms: u32,
}

pub fn parse_device_info(line: &str) -> Option<DeviceInfo> {
    let mut tokens = line.split_whitespace();
    if tokens.next()? != "DEVICE" {
        return None;
    }
    let mut protocol = None;
    let mut board = None;
    let mut slots = None;
    let mut watchdog_ms = None;
    for token in tokens {
        let (key, value) = token.split_once('=')?;
        match key {
            "protocol" => protocol = Some(value.parse().ok()?),
            "board" => board = Some(value.to_owned()),
            "slots" => slots = Some(value.parse().ok()?),
            "watchdog_ms" => watchdog_ms = Some(value.parse().ok()?),
            _ => {}
        }
    }
    Some(DeviceInfo {
        protocol: protocol?,
        board: board?,
        slots: slots?,
        watchdog_ms: watchdog_ms?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cctl_capabilities() {
        let info =
            parse_device_info("DEVICE protocol=1 board=cctl slots=3 watchdog_ms=250").unwrap();
        assert_eq!(info.protocol, 1);
        assert_eq!(info.board, "cctl");
        assert_eq!(info.slots, 3);
        assert_eq!(info.watchdog_ms, 250);
    }

    #[test]
    fn rejects_incomplete_lines() {
        assert!(parse_device_info("DEVICE protocol=1 board=cctl").is_none());
        assert!(parse_device_info("STATE protocol=1 board=cctl slots=3 watchdog_ms=250").is_none());
    }
}

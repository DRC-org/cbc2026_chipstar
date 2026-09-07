//! 汎用FWが返す能力通知の解釈。

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DeviceInfo {
    pub protocol: u8,
    pub board: String,
    pub slots: u8,
    pub can_buses: Vec<u8>,
    pub watchdog_ms: u32,
    /// 基板が保存済みの調整値で動いているか。旧FWは通知しないので `false`。
    pub parameters_stored: bool,
    pub jog: bool,
    pub motor_layout: String,
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
    let mut can_buses = Vec::new();
    let mut parameters_stored = false;
    let mut jog = false;
    let mut motor_layout = String::new();
    for token in tokens {
        let (key, value) = token.split_once('=')?;
        match key {
            "protocol" => protocol = Some(value.parse().ok()?),
            "board" => board = Some(value.to_owned()),
            "slots" => slots = Some(value.parse().ok()?),
            "can" => {
                can_buses = value
                    .split(',')
                    .map(str::parse)
                    .collect::<Result<Vec<_>, _>>()
                    .ok()?;
            }
            "watchdog_ms" => watchdog_ms = Some(value.parse().ok()?),
            "motors" => motor_layout = value.to_owned(),
            "jog" => jog = value == "1",
            "params" => parameters_stored = value == "stored",
            _ => {}
        }
    }
    Some(DeviceInfo {
        protocol: protocol?,
        board: board?,
        slots: slots?,
        can_buses,
        watchdog_ms: watchdog_ms?,
        parameters_stored,
        motor_layout,
        jog,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cctl_capabilities() {
        let info = parse_device_info("DEVICE protocol=1 board=cctl slots=3 can=2 watchdog_ms=250")
            .unwrap();
        assert_eq!(info.protocol, 1);
        assert_eq!(info.board, "cctl");
        assert_eq!(info.slots, 3);
        assert_eq!(info.can_buses, vec![2]);
        assert_eq!(info.watchdog_ms, 250);
        assert!(!info.parameters_stored);
    }

    #[test]
    fn reads_the_stored_parameter_flag() {
        let info = parse_device_info(
            "DEVICE protocol=1 board=cctl slots=3 can=2 watchdog_ms=250 params=stored",
        )
        .unwrap();
        assert!(info.parameters_stored);
    }

    #[test]
    fn rejects_incomplete_lines() {
        assert!(parse_device_info("DEVICE protocol=1 board=cctl").is_none());
        assert!(parse_device_info("STATE protocol=1 board=cctl slots=3 watchdog_ms=250").is_none());
    }
}

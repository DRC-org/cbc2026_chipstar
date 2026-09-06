//! serial_svmdのCAN指令をcctlゲートウェイ用の行へ変換する。
//!
//! 基板のUSART2にもASCIIの口があるが、機体としてはcctlのFDCAN2経由に一本化する。
//! PCへのUSBはcctlの1本だけになる。

const CAN_BUS: u8 = 2;
const COMMAND_CAN_ID: u16 = 0x320;
const PROTOCOL_VERSION: u8 = 1;

// 基板側はDIPで選ぶアドレスぶんCAN IDをずらせる（0x100刻み、0..3）。
// hostは後から書き換えられるので、2台目が必要になった時点でここへ足す。
// いまは address 0 のIDだけを送る。

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Hello,
    Safe,
    Run,
    Stop,
    Enable {
        id: u8,
        enabled: bool,
    },
    Read {
        id: u8,
    },
    Target {
        id: u8,
        position: u16,
        speed: u16,
        acceleration: u8,
    },
}

impl Command {
    pub fn to_cctl_line(self) -> String {
        let mut data = [0u8; 8];
        data[0] = PROTOCOL_VERSION;
        match self {
            Self::Hello => {}
            Self::Safe => data[1] = 1,
            Self::Run => data[1] = 2,
            Self::Stop => data[1] = 3,
            Self::Target {
                id,
                position,
                speed,
                acceleration,
            } => {
                data[1] = 4;
                data[2] = id;
                data[3] = acceleration;
                data[4] = (position >> 8) as u8;
                data[5] = position as u8;
                data[6] = (speed >> 8) as u8;
                data[7] = speed as u8;
            }
            Self::Enable { id, enabled } => {
                data[1] = 6;
                data[2] = id;
                data[3] = u8::from(enabled);
            }
            Self::Read { id } => {
                data[1] = 7;
                data[2] = id;
            }
        }

        let payload: String = data.iter().map(|byte| format!("{byte:02X}")).collect();
        format!("CAN {CAN_BUS} {COMMAND_CAN_ID} {payload}")
    }
}

/// serial_svmdの実行時パラメータ。
pub const PARAMETER_NAMES: [&str; 4] = [
    "servo_baud",
    "servo_timeout_ms",
    "wait_for_write_status",
    "watchdog_ms",
];

pub fn parameter_line(id: u8, value: f32) -> String {
    let bytes = value.to_be_bytes();
    format!(
        "CAN 2 800 0109{id:02X}00{:02X}{:02X}{:02X}{:02X}",
        bytes[0], bytes[1], bytes[2], bytes[3]
    )
}

/// サーボの実測位置。`0x322`（802）のフレームから取り出す。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServoState {
    pub id: u8,
    pub position: u16,
    pub enabled: bool,
    pub error: u8,
}

pub fn parse_state(line: &str) -> Option<ServoState> {
    let data = line.strip_prefix("CAN_RX bus=2 id=802 data=")?;
    if data.len() != 16 || !data.is_ascii() {
        return None;
    }
    let byte = |index: usize| u8::from_str_radix(&data[index * 2..index * 2 + 2], 16).ok();
    if byte(0)? != PROTOCOL_VERSION {
        return None;
    }
    Some(ServoState {
        id: byte(1)?,
        position: u16::from(byte(2)?) << 8 | u16::from(byte(3)?),
        enabled: byte(4)? != 0,
        error: byte(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_commands_for_cctl_gateway() {
        assert_eq!(Command::Hello.to_cctl_line(), "CAN 2 800 0100000000000000");
        assert_eq!(Command::Stop.to_cctl_line(), "CAN 2 800 0103000000000000");
        assert_eq!(
            Command::Target {
                id: 12,
                position: 2048,
                speed: 500,
                acceleration: 30,
            }
            .to_cctl_line(),
            "CAN 2 800 01040C1E080001F4"
        );
        assert_eq!(
            Command::Enable {
                id: 12,
                enabled: true,
            }
            .to_cctl_line(),
            "CAN 2 800 01060C0100000000"
        );
        assert_eq!(
            Command::Safe.to_cctl_line(),
            "CAN 2 800 0101000000000000"
        );
        assert_eq!(Command::Run.to_cctl_line(), "CAN 2 800 0102000000000000");
        assert_eq!(
            Command::Read { id: 9 }.to_cctl_line(),
            "CAN 2 800 0107090000000000"
        );
    }

    #[test]
    fn parses_servo_state_and_rejects_other_frames() {
        assert_eq!(
            parse_state("CAN_RX bus=2 id=802 data=010C080001040000"),
            Some(ServoState {
                id: 12,
                position: 2048,
                enabled: true,
                error: 4,
            })
        );
        assert!(parse_state("CAN_RX bus=2 id=801 data=0100000000000000").is_none());
        assert!(parse_state("STATE t=1 mode=RUN").is_none());
    }
}

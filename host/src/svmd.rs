//! svmdのCAN指令をcctlゲートウェイ用の行へ変換する。

const CAN_BUS: u8 = 2;
const COMMAND_CAN_ID: u16 = 0x300;
const PROTOCOL_VERSION: u8 = 1;

// 基板側はDIPで選ぶアドレスぶんCAN IDをずらせる（0x100刻み、0..3）。
// hostは後から書き換えられるので、2台目が必要になった時点でここへ足す。
// いまは address 0 のIDだけを送る。

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Stop,
    Set { channel: u8, pulse_us: u16 },
    Enable { channel: u8, enabled: bool },
}

impl Command {
    pub fn to_cctl_line(self) -> String {
        let mut data = [0u8; 8];
        data[0] = PROTOCOL_VERSION;
        match self {
            Self::Stop => {}
            Self::Set { channel, pulse_us } => {
                data[1] = 1;
                data[2] = channel;
                data[4] = (pulse_us >> 8) as u8;
                data[5] = pulse_us as u8;
            }
            Self::Enable { channel, enabled } => {
                data[1] = 2;
                data[2] = channel;
                data[3] = u8::from(enabled);
            }
        }

        let payload: String = data.iter().map(|byte| format!("{byte:02X}")).collect();
        format!("CAN {CAN_BUS} {COMMAND_CAN_ID} {payload}")
    }
}

/// svmdの実行時パラメータ。
pub const PARAMETER_NAMES: [&str; 3] = ["min_pulse_us", "max_pulse_us", "watchdog_ms"];

pub fn parameter_line(id: u8, value: f32) -> String {
    let bytes = value.to_be_bytes();
    format!(
        "CAN 2 768 0104{id:02X}00{:02X}{:02X}{:02X}{:02X}",
        bytes[0], bytes[1], bytes[2], bytes[3]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_commands_for_cctl_gateway() {
        assert_eq!(Command::Stop.to_cctl_line(), "CAN 2 768 0100000000000000");
        assert_eq!(
            Command::Set {
                channel: 2,
                pulse_us: 1500,
            }
            .to_cctl_line(),
            "CAN 2 768 0101020005DC0000"
        );
        assert_eq!(
            Command::Enable {
                channel: 2,
                enabled: true,
            }
            .to_cctl_line(),
            "CAN 2 768 0102020100000000"
        );
    }
}

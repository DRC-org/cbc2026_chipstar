//! cctlの汎用slot指令を生成する。

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Command {
    Stop,
    Run,
    Safe,
    Home { slots: u8 },
    Enable { slots: u8, enabled: bool },
}

impl Command {
    pub fn to_line(self) -> String {
        match self {
            Command::Stop => "STOP".to_owned(),
            Command::Run => "RUN".to_owned(),
            Command::Safe => "SAFE".to_owned(),
            Command::Home { slots } => format!("HOME {slots}"),
            Command::Enable { slots, enabled } => {
                format!("ENABLE {slots} {}", u8::from(enabled))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_state_commands() {
        assert_eq!(Command::Stop.to_line(), "STOP");
        assert_eq!(Command::Run.to_line(), "RUN");
        assert_eq!(Command::Safe.to_line(), "SAFE");
    }

    #[test]
    fn formats_slot_commands() {
        assert_eq!(Command::Home { slots: 7 }.to_line(), "HOME 7");
        assert_eq!(
            Command::Enable {
                slots: 2,
                enabled: true
            }
            .to_line(),
            "ENABLE 2 1"
        );
    }
}

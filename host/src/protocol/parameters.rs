//! パラメータ指令と適用値応答の共通表現。
use super::{dcmd, serial_svmd, svmd};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParameterBoard {
    Cctl,
    Svmd,
    Dcmd,
    SerialSvmd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParameterKey {
    pub board: ParameterBoard,
    pub id: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct ParameterValue {
    pub key: ParameterKey,
    pub value: f32,
}

impl ParameterValue {
    pub fn command(&self) -> String {
        match self.key.board {
            ParameterBoard::Cctl => format!("PARAM {} {:.5}", self.key.id, self.value),
            ParameterBoard::Svmd => svmd::parameter_line(self.key.id, self.value),
            ParameterBoard::Dcmd => dcmd::parameter_line(self.key.id, self.value),
            ParameterBoard::SerialSvmd => serial_svmd::parameter_line(self.key.id, self.value),
        }
    }

    pub fn parse_reply(line: &str) -> Option<Self> {
        if let Some(body) = line.strip_prefix("PARAM ") {
            let mut words = body.split_whitespace();
            let id = words.next()?.parse().ok()?;
            let value = words.next()?.parse().ok()?;
            if words.next().is_some() {
                return None;
            }
            return Some(Self {
                key: ParameterKey {
                    board: ParameterBoard::Cctl,
                    id,
                },
                value,
            });
        }
        let body = line.strip_prefix("CAN_RX bus=2 id=")?;
        let (board, data) = body.split_once(" data=")?;
        let board = match board {
            "772" => ParameterBoard::Svmd,
            "788" => ParameterBoard::Dcmd,
            "804" => ParameterBoard::SerialSvmd,
            _ => return None,
        };
        if data.len() != 16 || !data.is_ascii() || !data.starts_with("01") {
            return None;
        }
        Some(Self {
            key: ParameterKey {
                board,
                id: u8::from_str_radix(&data[2..4], 16).ok()?,
            },
            value: f32::from_bits(u32::from_str_radix(&data[8..16], 16).ok()?),
        })
    }
}

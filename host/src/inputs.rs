//! スイッチの接点状態。1はGNDへの閉接点、highは開接点を停止条件とするbit。
use crate::fw_test::Board;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputState {
    pub raw: u8,
    pub stable: u8,
    pub dip: u8,
    pub guard: u8,
    pub high: u8,
    pub trip: bool,
    pub available: u8,
}

pub fn parse(line: &str, board: Board) -> Option<InputState> {
    let (available, dip_mask) = match board {
        Board::Cctl => (7, 15),
        Board::SerialSvmd => (63, 15),
        Board::Dcmd => (7, 3),
        Board::Svmd => (0, 15),
    };
    let values = if matches!(board, Board::Cctl | Board::SerialSvmd) {
        let fields: Vec<_> = line
            .strip_prefix("INPUT_STATE ")?
            .split_whitespace()
            .collect();
        let mut values = vec![];
        for key in ["raw", "stable", "dip", "guard", "high", "trip", "available"] {
            values.push(
                fields
                    .iter()
                    .find_map(|field| {
                        field
                            .split_once('=')
                            .filter(|(name, _)| *name == key)
                            .map(|(_, value)| value)
                    })?
                    .parse::<u8>()
                    .ok()?,
            );
        }
        values
    } else {
        let prefix = if board == Board::Dcmd {
            "CAN_RX bus=2 id=787 data="
        } else {
            "CAN_RX bus=2 id=770 data="
        };
        let data = line.strip_prefix(prefix)?;
        if data.len() != 16 || !data.is_ascii() || !data.starts_with("01") {
            return None;
        }
        (1..8)
            .map(|i| u8::from_str_radix(&data[i * 2..i * 2 + 2], 16).ok())
            .collect::<Option<Vec<_>>>()?
    };
    if values[6] != available
        || values[0] & !available != 0
        || values[1] & !available != 0
        || values[2] & !dip_mask != 0
        || values[3] & !available != 0
        || values[4] & !values[3] != 0
        || values[5] > 1
    {
        return None;
    }
    Some(InputState {
        raw: values[0],
        stable: values[1],
        dip: values[2],
        guard: values[3],
        high: values[4],
        trip: values[5] != 0,
        available,
    })
}

pub fn describe(state: &InputState) -> String {
    format!(
        "INPUT raw={:06b} stable={:06b} DIP={:04b} guard={:06b} high={:06b} trip={}",
        state.raw, state.stable, state.dip, state.guard, state.high, state.trip
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_masks_and_rejects_other_board_or_invalid_payload() {
        let value = parse(
            "INPUT_STATE raw=1 stable=0 dip=2 guard=3 high=2 trip=1 available=7",
            Board::Cctl,
        )
        .unwrap();
        assert!(value.trip);
        assert_eq!(value.raw, 1);
        assert_eq!(
            parse("CAN_RX bus=2 id=787 data=0101000203020107", Board::Dcmd),
            Some(value)
        );
        assert!(parse("CAN_RX bus=2 id=787 data=0101000203020107", Board::Svmd).is_none());
        assert!(parse("CAN_RX bus=2 id=787 data=0101000203040107", Board::Dcmd).is_none());
        assert!(parse("CAN_RX bus=2 id=770 data=0100000F00000000", Board::Svmd).is_some());
    }
}

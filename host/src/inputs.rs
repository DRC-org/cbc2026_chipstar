//! 基板が報告する接点とDIPの状態。接点はGNDへ閉じたとき1。
//!
//! cctlの接点は`STATE`の`sw=`で常時届くため、ここでは要求応答型の基板だけを扱う。
use crate::fw_test::Board;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputState {
    pub raw: u8,
    pub stable: u8,
    pub dip: u8,
    pub available: u8,
}

/// 基板が備える接点bitとDIP bit。
fn capability(board: Board) -> Option<(u8, u8)> {
    match board {
        Board::SerialSvmd => Some((63, 15)),
        Board::Dcmd => Some((7, 3)),
        Board::Cctl | Board::Svmd => None,
    }
}

/// 要求応答で接点を読める基板か。cctlは`STATE`で常時届くので対象外。
pub fn supported(board: Board) -> bool {
    capability(board).is_some()
}

pub fn parse(line: &str, board: Board) -> Option<InputState> {
    let (available, dip_mask) = capability(board)?;
    let values = match board {
        Board::SerialSvmd => {
            let fields: Vec<_> = line
                .strip_prefix("INPUT_STATE ")?
                .split_whitespace()
                .collect();
            let mut values = Vec::new();
            for key in ["raw", "stable", "dip", "available"] {
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
        }
        _ => {
            let data = line.strip_prefix("CAN_RX bus=2 id=787 data=")?;
            if data.len() != 16 || !data.is_ascii() || !data.starts_with("01") {
                return None;
            }
            (1..5)
                .map(|index| u8::from_str_radix(&data[index * 2..index * 2 + 2], 16).ok())
                .collect::<Option<Vec<_>>>()?
        }
    };
    if values[3] != available
        || values[0] & !available != 0
        || values[1] & !available != 0
        || values[2] & !dip_mask != 0
    {
        return None;
    }
    Some(InputState {
        raw: values[0],
        stable: values[1],
        dip: values[2],
        available,
    })
}

pub fn describe(state: &InputState) -> String {
    format!(
        "INPUT raw={:06b} stable={:06b} DIP={:04b}",
        state.raw, state.stable, state.dip
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_report_forms_and_rejects_other_boards() {
        let value = parse(
            "INPUT_STATE raw=1 stable=0 dip=2 available=63",
            Board::SerialSvmd,
        )
        .unwrap();
        assert_eq!(value.raw, 1);
        assert_eq!(value.dip, 2);

        let dcmd = parse("CAN_RX bus=2 id=787 data=0101000207000000", Board::Dcmd).unwrap();
        assert_eq!(dcmd.stable, 0);
        assert_eq!(dcmd.available, 7);

        // 他基板の報告や、能力と食い違う値は受け付けない。
        assert!(parse("CAN_RX bus=2 id=787 data=0101000207000000", Board::Cctl).is_none());
        assert!(parse("CAN_RX bus=2 id=787 data=010100020F000000", Board::Dcmd).is_none());
        assert!(parse("INPUT_STATE raw=64 stable=0 dip=0 available=63", Board::SerialSvmd).is_none());
    }
}

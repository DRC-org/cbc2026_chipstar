//! 基板種別とゲートウェイ接続方式。
use clap::ValueEnum;

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Board {
    Cctl,
    Svmd,
    SerialSvmd,
    Dcmd,
    /// cctlのUSB1本で、cctl・svmd・DCMDを繋ぎ替えずにまとめて扱う。
    Network,
}

impl Board {
    pub fn key(self) -> &'static str {
        match self {
            Board::Cctl => "cctl",
            Board::Svmd => "svmd",
            Board::SerialSvmd => "serial_svmd",
            Board::Dcmd => "dcmd",
            Board::Network => "network",
        }
    }

    /// この対象が実際に触る基板。
    pub fn members(self) -> Vec<Board> {
        match self {
            Board::Network => vec![Board::Cctl, Board::Svmd, Board::Dcmd],
            other => vec![other],
        }
    }

    /// cctlのUSB CDCへ繋ぐ対象か。
    pub fn via_cctl(self) -> bool {
        !matches!(self, Board::SerialSvmd)
    }
}

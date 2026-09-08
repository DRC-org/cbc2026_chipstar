//! 入力スナップショットとゲームパッドの読み取り。
pub mod controller;

pub const BUTTON_NAMES: [(usize, &str); 15] = [
    (0, "×"),
    (1, "○"),
    (2, "□"),
    (3, "△"),
    (4, "Create"),
    (5, "PS"),
    (6, "Options"),
    (7, "L3"),
    (8, "R3"),
    (9, "L1"),
    (10, "R1"),
    (11, "↑"),
    (12, "↓"),
    (13, "←"),
    (14, "→"),
];

/// 1 フレーム分のコントローラ状態。
#[derive(Clone, Default, serde::Serialize)]
pub struct ControllerState {
    /// LX, LY, RX, RY, L2, R2 の順（各 -1.0..=1.0）。
    pub axes: [f32; 6],
    /// 17 個のボタン（押下で 1、非押下で 0）。
    pub buttons: [u8; 17],
}

//! 入力スナップショットとゲームパッドの読み取り。
pub mod controller;

/// 1 フレーム分のコントローラ状態。
#[derive(Clone, Default)]
pub struct ControllerState {
    /// LX, LY, RX, RY, L2, R2 の順（各 -1.0..=1.0）。
    pub axes: [f32; 6],
    /// 17 個のボタン（押下で 1、非押下で 0）。
    pub buttons: [u8; 17],
}

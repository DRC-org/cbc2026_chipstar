//! コントローラ入力の読み取り。
//!
//! gilrs の名前付きコントロールを、cctl が期待する軸/ボタンのインデックス順
//! （ROS `joy` 由来の SDL GameController 準拠）に対応付ける。
//! 軸の符号やボタン番号は実機に合わせて調整できるよう、この対応表に集約する。

use gilrs::{Axis, Button, Gamepad};

/// 1 フレーム分のコントローラ状態。
pub struct ControllerState {
    /// LX, LY, RX, RY, L2, R2 の順（各 -1.0..=1.0）。
    pub axes: [f32; 6],
    /// 17 個のボタン（押下で 1、非押下で 0）。
    pub buttons: [u8; 17],
}

/// 接続中のゲームパッドから現在の状態を読み取る。
pub fn read(gamepad: &Gamepad) -> ControllerState {
    let axis = |a: Axis| gamepad.value(a);
    let trigger = |b: Button| gamepad.button_data(b).map(|d| d.value()).unwrap_or(0.0);
    let button = |b: Button| u8::from(gamepad.is_pressed(b));

    ControllerState {
        axes: [
            axis(Axis::LeftStickX),
            axis(Axis::LeftStickY),
            axis(Axis::RightStickX),
            axis(Axis::RightStickY),
            trigger(Button::LeftTrigger2),
            trigger(Button::RightTrigger2),
        ],
        buttons: [
            button(Button::South),
            button(Button::East),
            button(Button::West),
            button(Button::North),
            button(Button::Select),
            button(Button::Mode),
            button(Button::Start),
            button(Button::LeftThumb),
            button(Button::RightThumb),
            button(Button::LeftTrigger),
            button(Button::RightTrigger),
            button(Button::DPadUp),
            button(Button::DPadDown),
            button(Button::DPadLeft),
            button(Button::DPadRight),
            // タッチパッド / 追加ボタンは gilrs の共通マッピング外のため未対応。
            0,
            0,
        ],
    }
}

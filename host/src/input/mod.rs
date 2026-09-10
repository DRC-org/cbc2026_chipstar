//! 入力スナップショットとゲームパッドの読み取り。
pub mod controller;

/// 機体軸へ割り当てられる入力数。0..5は生入力、6はR2-L2の合成入力。
pub const MACHINE_INPUT_COUNT: usize = 7;

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

impl ControllerState {
    pub fn machine_axis(&self, index: usize) -> Option<f32> {
        match index {
            0..=5 => Some(self.axes[index]),
            6 => Some(self.axes[5] - self.axes[4]),
            _ => None,
        }
    }

    pub fn set_machine_axis(&mut self, index: usize, value: f32) -> bool {
        match index {
            0..=5 => self.axes[index] = value,
            6 if value >= 0.0 => {
                self.axes[4] = 0.0;
                self.axes[5] = value;
            }
            6 => {
                self.axes[4] = -value;
                self.axes[5] = 0.0;
            }
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combined_triggers_are_a_signed_machine_axis() {
        let mut input = ControllerState::default();
        input.axes[4] = 0.7;
        input.axes[5] = 0.2;
        assert!((input.machine_axis(6).unwrap() + 0.5).abs() < 1e-6);
        assert!(input.set_machine_axis(6, 0.4));
        assert_eq!((input.axes[4], input.axes[5]), (0.0, 0.4));
        assert!(input.set_machine_axis(6, -0.3));
        assert_eq!((input.axes[4], input.axes[5]), (0.3, 0.0));
    }
}

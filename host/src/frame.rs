//! cctl が解釈するシリアル行フォーマットの生成。
//!
//! 出力例: `LX+012 LY-034 RX+000 RY+000 L2+000 R2+100 B00100000000000000`
//! - 軸値は `value * 100` を四捨五入し、符号付きゼロ詰め 4 桁で表現する。
//! - ボタンは 17 個を 0/1 で連結する。
//! - 行末の改行は付けない（送信側で付与する）。

use crate::controller::ControllerState;

/// 軸値（-1.0..=1.0）を百分率の整数に変換する。
fn percent(value: f32) -> i32 {
    (value * 100.0).round() as i32
}

/// コントローラ状態を cctl 向けの 1 行にフォーマットする。
pub fn format_controller_input(state: &ControllerState) -> String {
    let mut line = format!(
        "LX{:+04} LY{:+04} RX{:+04} RY{:+04} L2{:+04} R2{:+04} B",
        percent(state.axes[0]),
        percent(state.axes[1]),
        percent(state.axes[2]),
        percent(state.axes[3]),
        percent(state.axes[4]),
        percent(state.axes[5]),
    );

    for &button in &state.buttons {
        line.push(if button != 0 { '1' } else { '0' });
    }

    line
}

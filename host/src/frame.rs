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

/// cctl へ送る立ち上げ・調整用の指令。
///
/// 操作入力（`LX...`）と同じリンクを流れるが、cctl 側は先に指令として
/// 解釈を試みるため取り違えは起きない。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    /// 全軸トルクオフ。
    Stop,
    /// 通常運転へ。
    Run,
    /// 指令を出さない待機状態へ。
    Safe,
    /// 現在位置を原点に取り直す。
    Home,
    /// 軸ごとの有効・無効。`axes` は `telemetry::axis_bit` の論理和。
    Enable { axes: u8, enabled: bool },
    /// 内蔵テストシーケンスの切替。
    Test(bool),
}

impl Command {
    /// cctl が解釈する 1 行に変換する（改行は付けない）。
    pub fn to_line(self) -> String {
        match self {
            Command::Stop => "STOP".to_owned(),
            Command::Run => "RUN".to_owned(),
            Command::Safe => "SAFE".to_owned(),
            Command::Home => "HOME".to_owned(),
            Command::Enable { axes, enabled } => {
                let mut letters = String::new();
                if axes & crate::telemetry::axis_bit::R != 0 {
                    letters.push('R');
                }
                if axes & crate::telemetry::axis_bit::THETA != 0 {
                    letters.push('T');
                }
                if axes & crate::telemetry::axis_bit::Z != 0 {
                    letters.push('Z');
                }
                format!("EN {letters} {}", u8::from(enabled))
            }
            Command::Test(enabled) => format!("TEST {}", u8::from(enabled)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::axis_bit;

    #[test]
    fn formats_simple_commands() {
        assert_eq!(Command::Stop.to_line(), "STOP");
        assert_eq!(Command::Run.to_line(), "RUN");
        assert_eq!(Command::Safe.to_line(), "SAFE");
        assert_eq!(Command::Home.to_line(), "HOME");
    }

    #[test]
    fn formats_test_toggle() {
        assert_eq!(Command::Test(true).to_line(), "TEST 1");
        assert_eq!(Command::Test(false).to_line(), "TEST 0");
    }

    #[test]
    fn formats_axis_letters_in_fixed_order() {
        // cctl 側は重複を弾くため、順序と綴りを固定しておく。
        let all = Command::Enable {
            axes: axis_bit::R | axis_bit::THETA | axis_bit::Z,
            enabled: true,
        };
        assert_eq!(all.to_line(), "EN RTZ 1");

        let theta = Command::Enable {
            axes: axis_bit::THETA,
            enabled: false,
        };
        assert_eq!(theta.to_line(), "EN T 0");

        let rz = Command::Enable {
            axes: axis_bit::R | axis_bit::Z,
            enabled: true,
        };
        assert_eq!(rz.to_line(), "EN RZ 1");
    }

    #[test]
    fn command_lines_have_no_newline() {
        // 送信側が改行を付けるので、ここでは含めない。
        for command in [Command::Stop, Command::Home, Command::Test(true)] {
            assert!(!command.to_line().contains('\n'));
        }
    }
}

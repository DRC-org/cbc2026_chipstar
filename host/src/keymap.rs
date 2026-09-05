//! vim 風のキー操作。
//!
//! GUI から切り離した純粋なモジュールにしてある。egui の型に依存しないので、
//! キー割当の意図をテストで固定できる。
//!
//! - Normal: 既定。キーがそのまま操作になる
//! - Insert: テキスト欄の編集中。キー操作は解釈しない
//! - Command: `:` で開く行入力

/// 解釈対象のキー。egui の型からはここへ変換して渡す。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Char(char),
    Enter,
    Escape,
    Space,
    Up,
    Down,
}

/// 入力モード。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Normal,
    Insert,
    Command,
}

impl Mode {
    /// 画面下部に出す表示。
    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "-- NORMAL --",
            Mode::Insert => "-- INSERT --",
            Mode::Command => "-- COMMAND --",
        }
    }
}

/// Normal モードのキーが意味する操作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// 何もしない（未割当、または多打鍵の途中）。
    None,
    NextScreen,
    PrevScreen,
    /// 0 始まりの画面番号。
    GotoScreen(usize),
    MoveDown,
    MoveUp,
    MoveTop,
    MoveBottom,
    /// 選択中の項目を実行する。
    Activate,
    /// 非常停止。押した時点で送る。
    Stop,
    EnterCommand,
    EnterInsert,
    ShowHelp,
    /// 選択の解除など、その場の状態を戻す。
    Cancel,
}

/// Normal モードのキー解釈。`g` のような前置キーの途中状態を持つ。
#[derive(Default)]
pub struct Normal {
    pending: Option<char>,
}

impl Normal {
    /// 多打鍵の途中かどうか。画面下部の表示に使う。
    pub fn pending(&self) -> Option<char> {
        self.pending
    }

    pub fn press(&mut self, key: Key) -> Action {
        if self.pending == Some('g') {
            self.pending = None;
            return match key {
                Key::Char('t') => Action::NextScreen,
                Key::Char('T') => Action::PrevScreen,
                Key::Char('g') => Action::MoveTop,
                _ => Action::None,
            };
        }

        match key {
            Key::Char('g') => {
                self.pending = Some('g');
                Action::None
            }
            Key::Char('j') | Key::Down => Action::MoveDown,
            Key::Char('k') | Key::Up => Action::MoveUp,
            Key::Char('G') => Action::MoveBottom,
            Key::Char(c @ '1'..='9') => Action::GotoScreen(c as usize - '1' as usize),
            Key::Char(':') => Action::EnterCommand,
            Key::Char('i') => Action::EnterInsert,
            Key::Char('?') => Action::ShowHelp,
            // 非常停止は 1 打鍵で届く場所に置く。
            // 誤って止まるのは安全側なので、押しやすさを優先する。
            Key::Space => Action::Stop,
            Key::Enter => Action::Activate,
            Key::Escape => {
                self.pending = None;
                Action::Cancel
            }
            _ => Action::None,
        }
    }
}

/// `:` で入力された行の意味。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ExCommand {
    /// 何も入力されていない。
    Empty,
    Quit,
    Help,
    /// cctl へそのまま送る行。
    ///
    /// GUI が知らない語は全て cctl 側へ渡す。`STOP` や `ENABLE 2 1` はもちろん、
    /// ファームに指令が増えても host を直さずに使える。
    Send(String),
}

/// `:` の行を解釈する。先頭の `:` は付いていてもいなくてもよい。
pub fn parse_ex(input: &str) -> ExCommand {
    let body = input.trim().trim_start_matches(':').trim();
    if body.is_empty() {
        return ExCommand::Empty;
    }

    match body.to_ascii_lowercase().as_str() {
        "q" | "quit" => ExCommand::Quit,
        "h" | "help" => ExCommand::Help,
        _ => ExCommand::Send(body.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_selection() {
        let mut normal = Normal::default();
        assert_eq!(normal.press(Key::Char('j')), Action::MoveDown);
        assert_eq!(normal.press(Key::Char('k')), Action::MoveUp);
        assert_eq!(normal.press(Key::Down), Action::MoveDown);
        assert_eq!(normal.press(Key::Up), Action::MoveUp);
    }

    #[test]
    fn switches_screens_with_gt() {
        let mut normal = Normal::default();

        // g は単独では何も起こさず、次のキーと合わせて意味を持つ。
        assert_eq!(normal.press(Key::Char('g')), Action::None);
        assert_eq!(normal.pending(), Some('g'));
        assert_eq!(normal.press(Key::Char('t')), Action::NextScreen);
        assert_eq!(normal.pending(), None);

        normal.press(Key::Char('g'));
        assert_eq!(normal.press(Key::Char('T')), Action::PrevScreen);
    }

    #[test]
    fn gg_moves_to_top() {
        let mut normal = Normal::default();
        normal.press(Key::Char('g'));
        assert_eq!(normal.press(Key::Char('g')), Action::MoveTop);
        assert_eq!(normal.press(Key::Char('G')), Action::MoveBottom);
    }

    #[test]
    fn unknown_key_after_g_is_discarded() {
        let mut normal = Normal::default();
        normal.press(Key::Char('g'));

        assert_eq!(normal.press(Key::Char('x')), Action::None);
        // 途中状態を残さない。次の j が飲まれない。
        assert_eq!(normal.pending(), None);
        assert_eq!(normal.press(Key::Char('j')), Action::MoveDown);
    }

    #[test]
    fn escape_after_prefix_only_cancels_the_prefix() {
        // vim と同じで、前置キーの後の Esc は打ち消すだけで他に何もしない。
        let mut normal = Normal::default();
        normal.press(Key::Char('g'));

        assert_eq!(normal.press(Key::Escape), Action::None);
        assert_eq!(normal.pending(), None);
    }

    #[test]
    fn escape_without_prefix_cancels_selection() {
        let mut normal = Normal::default();
        assert_eq!(normal.press(Key::Escape), Action::Cancel);
    }

    #[test]
    fn digits_select_screens() {
        let mut normal = Normal::default();
        assert_eq!(normal.press(Key::Char('1')), Action::GotoScreen(0));
        assert_eq!(normal.press(Key::Char('4')), Action::GotoScreen(3));
    }

    #[test]
    fn space_stops() {
        // 非常停止は前置キーなしで届く必要がある。
        let mut normal = Normal::default();
        assert_eq!(normal.press(Key::Space), Action::Stop);
    }

    #[test]
    fn enters_other_modes() {
        let mut normal = Normal::default();
        assert_eq!(normal.press(Key::Char(':')), Action::EnterCommand);
        assert_eq!(normal.press(Key::Char('i')), Action::EnterInsert);
        assert_eq!(normal.press(Key::Char('?')), Action::ShowHelp);
    }

    #[test]
    fn unassigned_keys_do_nothing() {
        let mut normal = Normal::default();
        assert_eq!(normal.press(Key::Char('z')), Action::None);
        assert_eq!(normal.press(Key::Char('/')), Action::None);
    }

    #[test]
    fn parses_gui_commands() {
        assert_eq!(parse_ex(":q"), ExCommand::Quit);
        assert_eq!(parse_ex("quit"), ExCommand::Quit);
        assert_eq!(parse_ex(":QUIT"), ExCommand::Quit);
        assert_eq!(parse_ex(":h"), ExCommand::Help);
        assert_eq!(parse_ex(":help"), ExCommand::Help);
    }

    #[test]
    fn passes_unknown_commands_to_cctl() {
        // ファームに指令が増えても host を直さずに使えるようにする。
        assert_eq!(parse_ex(":stop"), ExCommand::Send("stop".to_owned()));
        assert_eq!(
            parse_ex(":ENABLE 2 1"),
            ExCommand::Send("ENABLE 2 1".to_owned())
        );
        assert_eq!(parse_ex("home"), ExCommand::Send("home".to_owned()));
    }

    #[test]
    fn keeps_argument_case_and_spacing() {
        // cctl 側の綴りをそのまま届ける。
        assert_eq!(
            parse_ex(":  EN  RTZ  1  "),
            ExCommand::Send("EN  RTZ  1".to_owned())
        );
    }

    #[test]
    fn empty_input_does_nothing() {
        assert_eq!(parse_ex(""), ExCommand::Empty);
        assert_eq!(parse_ex(":"), ExCommand::Empty);
        assert_eq!(parse_ex("   "), ExCommand::Empty);
    }
}

//! 通常モードのキーとコロンコマンド。出力可否は共通の操作受付で判定する。
use super::Screen;
use eframe::egui::{Event, Key, Modifiers};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Action {
    Stop,
    Run,
    Recover,
    Emergency(bool),
    Escape,
    Command,
    Screen(Screen),
    Apply,
    Save,
    Help,
    Tab(i32),
    Scroll(f32),
    Page(f32),
    Edge(bool),
}
pub(super) struct CommandSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub action: Action,
}
pub(super) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "recover",
        description: "出力停止のまま設定とエラーを再確認",
        action: Action::Recover,
    },
    CommandSpec {
        name: "run",
        description: "運転再開",
        action: Action::Run,
    },
    CommandSpec {
        name: "stop",
        description: "停止・保持（個別テストは出力解除）",
        action: Action::Stop,
    },
    CommandSpec {
        name: "apply",
        description: "調整画面の編集内容を適用",
        action: Action::Apply,
    },
    CommandSpec {
        name: "w",
        description: "調整画面の適用済み設定を保存",
        action: Action::Save,
    },
];
pub(super) fn command(text: &str) -> Option<Action> {
    let text = text.trim();
    COMMANDS
        .iter()
        .find(|item| item.name == text)
        .map(|item| item.action)
}
#[derive(Default)]
pub(super) struct Vim {
    g_at: Option<f64>,
}
impl Vim {
    pub fn clear(&mut self) {
        self.g_at = None;
    }
    pub fn resolve(&mut self, events: &[Event], editing: bool, now: f64) -> Option<Action> {
        if editing {
            self.clear();
            return None;
        }
        for event in events {
            let Event::Key {
                key,
                pressed: true,
                repeat,
                modifiers,
                ..
            } = event
            else {
                continue;
            };
            let mut modifiers = *modifiers;
            modifiers.command = false;
            let g = self.g_at.take().is_some_and(|time| now - time <= 0.8);
            let action = match (*key, modifiers) {
                (Key::J, Modifiers::NONE) => Some(Action::Scroll(48.0)),
                (Key::K, Modifiers::NONE) => Some(Action::Scroll(-48.0)),
                (Key::D, Modifiers::CTRL) => Some(Action::Page(0.5)),
                (Key::U, Modifiers::CTRL) => Some(Action::Page(-0.5)),
                (Key::H, Modifiers::NONE) if !repeat => Some(Action::Tab(-1)),
                (Key::L, Modifiers::NONE) if !repeat => Some(Action::Tab(1)),
                (Key::T, Modifiers::NONE) if g && !repeat => Some(Action::Tab(1)),
                (Key::T, Modifiers::SHIFT) if g && !repeat => Some(Action::Tab(-1)),
                (Key::G, Modifiers::SHIFT) if !repeat => Some(Action::Edge(false)),
                (Key::G, Modifiers::NONE) if !repeat => {
                    if g {
                        Some(Action::Edge(true))
                    } else {
                        self.g_at = Some(now);
                        None
                    }
                }
                _ => None,
            };
            if action.is_some() {
                return action;
            }
        }
        None
    }
}
pub(super) fn resolve(
    events: &[Event],
    editing: bool,
    outputs_active: bool,
    emergency: bool,
) -> Option<Action> {
    let keys: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::Key {
                key,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } => {
                let mut modifiers = *modifiers;
                modifiers.command = false;
                Some((*key, modifiers))
            }
            _ => None,
        })
        .collect();
    // 出力中のSpaceは文字入力より優先。解除は編集中に行わない。
    if keys.contains(&(Key::Space, Modifiers::NONE)) && (!editing || (outputs_active && !emergency))
    {
        return Some(Action::Emergency(!emergency));
    }
    if keys.iter().any(|(key, _)| *key == Key::Escape) {
        return Some(Action::Escape);
    }
    if editing {
        return None;
    }
    if keys.contains(&(Key::S, Modifiers::NONE)) {
        return Some(Action::Stop);
    }
    if let Some(action) = events.iter().find_map(|event| match event {
        Event::Text(text) if text == ":" => Some(Action::Command),
        Event::Text(text) if text == "?" => Some(Action::Help),
        _ => None,
    }) {
        return Some(action);
    }
    keys.into_iter().find_map(|(key, modifiers)| {
        if modifiers != Modifiers::NONE {
            return None;
        }
        match key {
            Key::Num1 => Some(Action::Screen(Screen::Operate)),
            Key::Num2 => Some(Action::Screen(Screen::Tune)),
            Key::Num3 => Some(Action::Screen(Screen::Diagnose)),
            Key::Num4 => Some(Action::Screen(Screen::Documents)),
            Key::Num5 => Some(Action::Screen(Screen::Debug)),
            _ => None,
        }
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn key(key: Key, modifiers: Modifiers, repeat: bool) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat,
            modifiers,
        }
    }
    #[test]
    fn emergency_overrides_output_editing_but_reset_does_not() {
        let events = [
            key(Key::Space, Modifiers::NONE, false),
            key(Key::S, Modifiers::NONE, false),
        ];
        assert_eq!(
            resolve(&events, true, true, false),
            Some(Action::Emergency(true))
        );
        assert_eq!(resolve(&events, true, false, false), None);
        assert_eq!(resolve(&events, true, true, true), None);
        assert_eq!(
            resolve(&events, false, false, true),
            Some(Action::Emergency(false))
        );
        assert_eq!(
            resolve(&[key(Key::Space, Modifiers::NONE, true)], false, true, true),
            None
        );
    }
    #[test]
    fn escape_only_exits_editing_and_old_bindings_are_removed() {
        assert_eq!(
            resolve(
                &[key(Key::Escape, Modifiers::NONE, false)],
                true,
                true,
                false
            ),
            Some(Action::Escape)
        );
        for event in [
            key(Key::F1, Modifiers::NONE, false),
            key(Key::F12, Modifiers::NONE, false),
            key(Key::Enter, Modifiers::CTRL, false),
            key(Key::S, Modifiers::CTRL, false),
        ] {
            assert_eq!(resolve(&[event], false, false, false), None);
        }
        for event in [
            key(Key::S, Modifiers::NONE, false),
            key(Key::Num1, Modifiers::NONE, false),
            Event::Text(":".into()),
        ] {
            assert_eq!(resolve(&[event], true, false, false), None);
        }
    }
    #[test]
    fn vim_prefix_expires_and_is_cleared_by_editing() {
        let mut vim = Vim::default();
        let g = [key(Key::G, Modifiers::NONE, false)];
        assert_eq!(vim.resolve(&g, false, 0.0), None);
        assert_eq!(
            vim.resolve(&[key(Key::T, Modifiers::SHIFT, false)], false, 0.5),
            Some(Action::Tab(-1))
        );
        vim.resolve(&g, false, 1.0);
        assert_eq!(vim.resolve(&g, false, 2.0), None);
        vim.resolve(&[], true, 2.1);
        assert_eq!(vim.resolve(&g, false, 2.2), None);
        assert_eq!(vim.resolve(&g, false, 2.3), Some(Action::Edge(true)));
        let ctrl = Modifiers {
            command: true,
            ..Modifiers::CTRL
        };
        assert_eq!(
            vim.resolve(&[key(Key::D, ctrl, false)], false, 3.0),
            Some(Action::Page(0.5))
        );
    }
    #[test]
    fn command_registry_accepts_only_complete_known_commands() {
        assert_eq!(command(" w "), Some(Action::Save));
        assert_eq!(command("run"), Some(Action::Run));
        assert_eq!(command("recover"), Some(Action::Recover));
        assert_eq!(command("recover anything"), None);
        assert_eq!(command("run anything"), None);
        assert_eq!(command("!rm"), None);
    }
}

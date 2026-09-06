//! キー入力の解釈。操作可否はボタンと共通の実行入口で判定する。
use super::Screen;
use eframe::egui::{Event, Key, Modifiers};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Action {
    Stop,
    Run,
    Screen(Screen),
    Apply,
    Save,
    Help,
    Tab(i32),
    Scroll(f32),
    Edge(bool),
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
        let mut action = None;
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
            let previous_g = self.g_at.take();
            action = match (*key, modifiers) {
                (Key::J, Modifiers::NONE) => Some(Action::Scroll(48.0)),
                (Key::K, Modifiers::NONE) => Some(Action::Scroll(-48.0)),
                (Key::D, Modifiers::CTRL) => Some(Action::Scroll(f32::INFINITY)),
                (Key::U, Modifiers::CTRL) => Some(Action::Scroll(f32::NEG_INFINITY)),
                (Key::H, Modifiers::NONE) if !repeat => Some(Action::Tab(-1)),
                (Key::L, Modifiers::NONE) if !repeat => Some(Action::Tab(1)),
                (Key::G, Modifiers::SHIFT) if !repeat => Some(Action::Edge(false)),
                (Key::G, Modifiers::NONE) if !repeat => {
                    if previous_g.is_some_and(|time| now - time <= 0.8) {
                        Some(Action::Edge(true))
                    } else {
                        self.g_at = Some(now);
                        None
                    }
                }
                _ => None,
            };
            if action.is_some() {
                break;
            }
        }
        action
    }
}

pub(super) fn resolve(events: &[Event], editing: bool) -> Option<Action> {
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
                // eguiはLinux/WindowsのCtrlにもcommandフラグを付ける。
                let mut modifiers = *modifiers;
                modifiers.command = false;
                Some((*key, modifiers))
            }
            _ => None,
        })
        .collect();
    // 同じフレームに開始と停止が来ても停止だけを処理する。
    if keys.iter().any(|(key, modifiers)| {
        *key == Key::Escape || (!editing && *key == Key::Space && *modifiers == Modifiers::NONE)
    }) {
        return Some(Action::Stop);
    }
    if editing {
        return None;
    }
    keys.into_iter().find_map(|(key, modifiers)| {
        if modifiers == Modifiers::CTRL {
            return match key {
                Key::Enter => Some(Action::Run),
                Key::S => Some(Action::Save),
                _ => None,
            };
        }
        if modifiers == Modifiers::CTRL | Modifiers::SHIFT && key == Key::Enter {
            return Some(Action::Apply);
        }
        if modifiers != Modifiers::NONE {
            return None;
        }
        match key {
            Key::F1 => Some(Action::Screen(Screen::Operate)),
            Key::F2 => Some(Action::Screen(Screen::Tune)),
            Key::F3 => Some(Action::Screen(Screen::Diagnose)),
            Key::F4 => Some(Action::Screen(Screen::Documents)),
            Key::F12 => Some(Action::Help),
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vim_prefix_expires_and_never_survives_editing_or_other_keys() {
        let mut vim = Vim::default();
        let g = key(Key::G, Modifiers::NONE, false);
        assert_eq!(vim.resolve(std::slice::from_ref(&g), false, 0.0), None);
        assert_eq!(
            vim.resolve(std::slice::from_ref(&g), false, 0.5),
            Some(Action::Edge(true))
        );
        assert_eq!(vim.resolve(std::slice::from_ref(&g), false, 1.0), None);
        assert_eq!(vim.resolve(std::slice::from_ref(&g), false, 2.0), None);
        assert_eq!(vim.resolve(std::slice::from_ref(&g), true, 2.1), None);
        assert_eq!(vim.resolve(std::slice::from_ref(&g), false, 2.2), None);
        assert_eq!(
            vim.resolve(&[key(Key::J, Modifiers::NONE, true)], false, 2.3),
            Some(Action::Scroll(48.0))
        );
        assert_eq!(vim.resolve(&[g], false, 2.4), None);
        for key_code in [Key::J, Key::K, Key::H, Key::L, Key::D, Key::U] {
            assert_eq!(
                vim.resolve(&[key(key_code, Modifiers::NONE, false)], true, 3.0),
                None
            );
        }
    }
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
    fn editing_suppresses_shortcuts_but_not_stop_escape() {
        for event in [
            key(Key::Space, Modifiers::NONE, false),
            key(Key::S, Modifiers::CTRL, false),
            key(Key::Enter, Modifiers::CTRL, false),
            key(Key::F2, Modifiers::NONE, false),
        ] {
            assert_eq!(resolve(&[event], true), None);
        }
        assert_eq!(
            resolve(&[key(Key::Escape, Modifiers::NONE, false)], true),
            Some(Action::Stop)
        );
    }
    #[test]
    fn platform_command_alias_does_not_hide_control_shortcuts() {
        let modifiers = Modifiers {
            ctrl: true,
            command: true,
            ..Modifiers::NONE
        };
        assert_eq!(
            resolve(&[key(Key::Enter, modifiers, false)], false),
            Some(Action::Run)
        );
        assert_eq!(
            resolve(&[key(Key::S, modifiers, false)], false),
            Some(Action::Save)
        );
    }

    #[test]
    fn stop_takes_priority_and_key_repeat_never_restarts() {
        assert_eq!(
            resolve(
                &[
                    key(Key::Enter, Modifiers::CTRL, false),
                    key(Key::Space, Modifiers::NONE, false)
                ],
                false
            ),
            Some(Action::Stop)
        );
        assert_eq!(
            resolve(&[key(Key::Enter, Modifiers::CTRL, true)], false),
            None
        );
        assert_eq!(
            resolve(
                &[key(Key::Enter, Modifiers::CTRL | Modifiers::SHIFT, false)],
                false
            ),
            Some(Action::Apply)
        );
        assert_eq!(
            resolve(
                &[key(Key::Enter, Modifiers::CTRL | Modifiers::ALT, false)],
                false
            ),
            None
        );
    }
}

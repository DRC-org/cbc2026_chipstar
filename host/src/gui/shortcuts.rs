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
            Key::F12 => Some(Action::Help),
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

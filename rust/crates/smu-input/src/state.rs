//! Physical key-state and hotkey matching.

use smu_core::{Hotkey, KeyCode};

/// Read-only physical key state. Wheel directions remain transient and are not represented here.
pub trait KeyState: Send + Sync {
    fn is_pressed(&self, key: KeyCode) -> bool;

    fn current_pressed_key(&self) -> Option<KeyCode> {
        None
    }

    fn consume_next_transient_input(&self) -> Option<KeyCode> {
        None
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HotkeyMatch {
    Loose,
    Strict,
}

fn modifier_pressed(state: &dyn KeyState, key: KeyCode) -> bool {
    match key {
        KeyCode::SHIFT => {
            state.is_pressed(KeyCode::SHIFT)
                || state.is_pressed(KeyCode::LSHIFT)
                || state.is_pressed(KeyCode::RSHIFT)
        }
        KeyCode::CONTROL => {
            state.is_pressed(KeyCode::CONTROL)
                || state.is_pressed(KeyCode::LCONTROL)
                || state.is_pressed(KeyCode::RCONTROL)
        }
        KeyCode::MENU => {
            state.is_pressed(KeyCode::MENU)
                || state.is_pressed(KeyCode::LMENU)
                || state.is_pressed(KeyCode::RMENU)
        }
        KeyCode::LWIN => state.is_pressed(KeyCode::LWIN) || state.is_pressed(KeyCode::RWIN),
        _ => state.is_pressed(key),
    }
}

fn exact_modifier_state(state: &dyn KeyState, hotkey: Hotkey) -> bool {
    let key = hotkey.key();
    let wants_win = hotkey.has_win() || matches!(key, KeyCode::LWIN | KeyCode::RWIN);
    let wants_ctrl = hotkey.has_ctrl()
        || matches!(key, KeyCode::CONTROL | KeyCode::LCONTROL | KeyCode::RCONTROL);
    let wants_alt =
        hotkey.has_alt() || matches!(key, KeyCode::MENU | KeyCode::LMENU | KeyCode::RMENU);
    let wants_shift =
        hotkey.has_shift() || matches!(key, KeyCode::SHIFT | KeyCode::LSHIFT | KeyCode::RSHIFT);

    modifier_pressed(state, KeyCode::LWIN) == wants_win
        && modifier_pressed(state, KeyCode::CONTROL) == wants_ctrl
        && modifier_pressed(state, KeyCode::MENU) == wants_alt
        && modifier_pressed(state, KeyCode::SHIFT) == wants_shift
}

/// Match a packed SMU hotkey against physical state using the C++ loose/strict rules.
#[must_use]
pub fn is_hotkey_pressed(state: &dyn KeyState, hotkey: Hotkey, mode: HotkeyMatch) -> bool {
    let key = hotkey.key();
    if key == KeyCode::NONE {
        return false;
    }
    if hotkey.has_win() && !modifier_pressed(state, KeyCode::LWIN) {
        return false;
    }
    if hotkey.has_ctrl() && !modifier_pressed(state, KeyCode::CONTROL) {
        return false;
    }
    if hotkey.has_alt() && !modifier_pressed(state, KeyCode::MENU) {
        return false;
    }
    if hotkey.has_shift() && !modifier_pressed(state, KeyCode::SHIFT) {
        return false;
    }
    if mode == HotkeyMatch::Strict && !exact_modifier_state(state, hotkey) {
        return false;
    }
    modifier_pressed(state, key)
}

#[cfg(test)]
mod tests {
    use super::{HotkeyMatch, KeyState, is_hotkey_pressed};
    use smu_core::{Hotkey, KeyCode};
    use std::collections::BTreeSet;

    struct SetState(BTreeSet<KeyCode>);

    impl KeyState for SetState {
        fn is_pressed(&self, key: KeyCode) -> bool {
            self.0.contains(&key)
        }
    }

    fn state(keys: &[KeyCode]) -> SetState {
        SetState(keys.iter().copied().collect())
    }

    #[test]
    fn loose_mode_allows_extra_modifiers() {
        let pressed = state(&[KeyCode::A, KeyCode::CONTROL, KeyCode::SHIFT]);
        assert!(is_hotkey_pressed(&pressed, Hotkey::new(0x2_0041), HotkeyMatch::Loose));
        assert!(!is_hotkey_pressed(&pressed, Hotkey::new(0x2_0041), HotkeyMatch::Strict));
    }

    #[test]
    fn modifier_main_key_counts_as_requested_in_strict_mode() {
        let pressed = state(&[KeyCode::LCONTROL]);
        assert!(is_hotkey_pressed(
            &pressed,
            Hotkey::new(KeyCode::LCONTROL.raw()),
            HotkeyMatch::Strict
        ));
    }
}

use smu_core::{Hotkey, KeyCode};
use smu_input::{HotkeyMatch, KeyState, is_hotkey_pressed};

pub fn hotkey_pressed(input: &dyn KeyState, hotkey: Hotkey) -> bool {
    let key = hotkey.key();
    if matches!(key, KeyCode::MOUSE_WHEEL_UP | KeyCode::MOUSE_WHEEL_DOWN) {
        return input.consume_next_transient_input() == Some(key);
    }
    is_hotkey_pressed(input, hotkey, HotkeyMatch::Loose)
}

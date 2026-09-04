//! Input-domain services for SMU.
//!
//! This crate deliberately excludes screen bounds, cursor reads, and pixel sampling. Those lived on
//! the old C++ `InputBackend`, but belong to the display/capture service in the Rust architecture.

use smu_core::KeyCode;

pub mod state;
pub mod text;

pub use state::{HotkeyMatch, KeyState, is_hotkey_pressed};
pub use text::{
    FALLBACK_SCAN_CODE_NONE, TextKeyAction, TextPlanEvent, compatibility_key_action,
    fallback_text_plan,
};

pub const NO_KEY: KeyCode = KeyCode::NONE;
pub const MOUSE_WHEEL_UP: KeyCode = KeyCode::MOUSE_WHEEL_UP;
pub const MOUSE_WHEEL_DOWN: KeyCode = KeyCode::MOUSE_WHEEL_DOWN;

/// Physical/injected output operations only. Display queries intentionally do not appear here.
pub trait InputInjector: Send + Sync {
    fn hold_key(&self, key: KeyCode, extended: bool);
    fn release_key(&self, key: KeyCode, extended: bool);
    fn move_mouse(&self, dx: i32, dy: i32);

    fn move_mouse_raw(&self, dx: i32, dy: i32) {
        self.move_mouse(dx, dy);
    }

    fn mouse_wheel(&self, delta: i32);
}

/// Compatibility facade for consumers that need both physical state and injection.
pub trait InputDevice: KeyState + InputInjector {
    fn format_key_name(&self, key: KeyCode) -> String {
        key.legacy_label()
    }
}

impl<T: KeyState + InputInjector> InputDevice for T {}

/// Optional native-layout character injection capability.
pub trait CharacterInput: InputInjector {
    /// Returns `true` only when the native keyboard-layout path emitted the character.
    fn press_character(&self, _character: char, _delay_ms: i32) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{InputDevice, InputInjector, KeyCode, KeyState};

    struct MinimalDevice;

    impl KeyState for MinimalDevice {
        fn is_pressed(&self, _key: KeyCode) -> bool {
            false
        }
    }

    impl InputInjector for MinimalDevice {
        fn hold_key(&self, _key: KeyCode, _extended: bool) {}
        fn release_key(&self, _key: KeyCode, _extended: bool) {}
        fn move_mouse(&self, _dx: i32, _dy: i32) {}
        fn mouse_wheel(&self, _delta: i32) {}
    }

    #[test]
    fn default_key_name_uses_core_compatibility_label() {
        assert_eq!(MinimalDevice.format_key_name(KeyCode::new(0x74)), "F5");
        assert_eq!(MinimalDevice.format_key_name(KeyCode::new(999)), "0x999");
    }
}

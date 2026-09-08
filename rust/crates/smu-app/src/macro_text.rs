//! Shared macro chat/text compatibility path for Linux.
//!
//! C++ `OpenChatForMacro` first tries the backend's native-layout `pressCharacter('/', 50)` when
//! chat override is enabled. If that native path is unavailable, it falls back to the configured
//! bound chat key. Normal text tries native layout per byte first and then uses the compatibility
//! fallback key map for only the bytes the native backend cannot emit.

use smu_core::Hotkey;
use smu_input::{InputInjector, execute_fallback_text};
use std::time::Duration;

pub fn open_chat(
    input: &dyn InputInjector,
    chat_override: impl FnOnce() -> bool,
    chat_key_down: impl FnOnce() -> Hotkey,
    chat_key_up: impl FnOnce() -> Hotkey,
    mut sleep: impl FnMut(Duration),
) {
    if chat_override() && input.press_character(b'/', 50) {
        return;
    }

    input.hold_key_chord(chat_key_down());
    sleep(Duration::from_millis(50));
    input.release_key_chord(chat_key_up());
}

pub fn paste_text(
    input: &dyn InputInjector,
    text: &str,
    delay_ms: i32,
    use_unicode: bool,
    sleep: impl FnMut(Duration),
) -> bool {
    let safe_delay_ms = delay_ms.max(0);
    if use_unicode && input.type_unicode_text(text, safe_delay_ms) {
        return true;
    }
    type_text(input, text, safe_delay_ms, sleep)
}

pub fn type_text(
    input: &dyn InputInjector,
    text: &str,
    delay_ms: i32,
    mut sleep: impl FnMut(Duration),
) -> bool {
    let safe_delay_ms = delay_ms.max(0);
    let mut typed_any = false;
    for &character in text.as_bytes() {
        if input.press_character(character, safe_delay_ms) {
            typed_any = true;
            continue;
        }
        typed_any |= execute_fallback_text(input, &[character], safe_delay_ms, &mut sleep);
    }
    typed_any
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic)]

    use super::{open_chat, paste_text, type_text};
    use smu_core::{Hotkey, KeyCode};
    use smu_input::InputInjector;
    use std::sync::Mutex;
    use std::time::Duration;

    #[derive(Debug, Default)]
    struct FakeInput {
        native_success: bool,
        unicode_success: bool,
        events: Mutex<Vec<String>>,
    }

    impl InputInjector for FakeInput {
        fn hold_key(&self, key: KeyCode, _extended: bool) {
            self.events.lock().expect("events").push(format!("down:{}", key.raw()));
        }
        fn release_key(&self, key: KeyCode, _extended: bool) {
            self.events.lock().expect("events").push(format!("up:{}", key.raw()));
        }
        fn move_mouse(&self, _dx: i32, _dy: i32) {}
        fn mouse_wheel(&self, delta: i32) {
            self.events.lock().expect("events").push(format!("wheel:{delta}"));
        }
        fn press_character(&self, character: u8, delay_ms: i32) -> bool {
            self.events.lock().expect("events").push(format!("native:{character}:{delay_ms}"));
            self.native_success
        }
        fn type_unicode_text(&self, text: &str, delay_ms: i32) -> bool {
            self.events.lock().expect("events").push(format!("unicode:{text}:{delay_ms}"));
            self.unicode_success
        }
    }

    #[test]
    fn chat_override_uses_native_slash_without_reading_bound_chat_key() {
        let input = FakeInput { native_success: true, ..FakeInput::default() };
        let mut waits = Vec::new();
        open_chat(
            &input,
            || true,
            || panic!("chat key down should not be read"),
            || panic!("chat key up should not be read"),
            |duration| waits.push(duration),
        );
        assert_eq!(*input.events.lock().expect("events"), ["native:47:50"]);
        assert!(waits.is_empty());
    }

    #[test]
    fn failed_native_chat_override_falls_back_to_live_bound_chat_key() {
        let input = FakeInput::default();
        let mut waits = Vec::new();
        open_chat(
            &input,
            || true,
            || Hotkey::new(0x41),
            || Hotkey::new(0x42),
            |duration| waits.push(duration),
        );
        assert_eq!(*input.events.lock().expect("events"), ["native:47:50", "down:65", "up:66"]);
        assert_eq!(waits, [Duration::from_millis(50)]);
    }

    #[test]
    fn disabled_chat_override_never_attempts_native_character() {
        let input = FakeInput { native_success: true, ..FakeInput::default() };
        let mut waits = Vec::new();
        open_chat(
            &input,
            || false,
            || Hotkey::new(0x41),
            || Hotkey::new(0x42),
            |duration| waits.push(duration),
        );
        assert_eq!(*input.events.lock().expect("events"), ["down:65", "up:66"]);
        assert_eq!(waits, [Duration::from_millis(50)]);
    }

    #[test]
    fn paste_text_uses_unicode_path_only_when_requested_and_available() {
        let input = FakeInput { unicode_success: true, ..FakeInput::default() };
        let mut waits = Vec::new();
        assert!(paste_text(&input, "hé", 3, true, |duration| waits.push(duration)));
        assert_eq!(*input.events.lock().expect("events"), ["unicode:hé:3"]);
        assert!(waits.is_empty());
    }

    #[test]
    fn paste_text_falls_back_to_normal_typing_when_unicode_path_is_unavailable() {
        let input = FakeInput::default();
        let mut waits = Vec::new();
        assert!(paste_text(&input, "a", 4, true, |duration| waits.push(duration)));
        assert_eq!(
            *input.events.lock().expect("events"),
            ["unicode:a:4", "native:97:4", "down:65", "up:65"]
        );
        assert_eq!(waits, [Duration::from_millis(4)]);
    }

    #[test]
    fn paste_text_skips_unicode_path_when_setting_is_disabled() {
        let input =
            FakeInput { unicode_success: true, native_success: true, ..FakeInput::default() };
        let mut waits = Vec::new();
        assert!(paste_text(&input, "a", 2, false, |duration| waits.push(duration)));
        assert_eq!(*input.events.lock().expect("events"), ["native:97:2"]);
        assert!(waits.is_empty());
    }

    #[test]
    fn typed_text_falls_back_per_byte_when_native_layout_is_unavailable() {
        let input = FakeInput::default();
        let mut waits = Vec::new();
        assert!(type_text(&input, "a?", 2, |duration| waits.push(duration)));
        assert_eq!(
            *input.events.lock().expect("events"),
            [
                "native:97:2",
                "down:65",
                "up:65",
                "native:63:2",
                "down:16",
                "down:191",
                "up:191",
                "up:16"
            ]
        );
        assert_eq!(waits, [Duration::from_millis(2), Duration::from_millis(2)]);
    }

    #[test]
    fn typed_text_uses_native_layout_without_fallback_wait_callback() {
        let input = FakeInput { native_success: true, ..FakeInput::default() };
        let mut waits = Vec::new();
        assert!(type_text(&input, "a?", -2, |duration| waits.push(duration)));
        assert_eq!(*input.events.lock().expect("events"), ["native:97:0", "native:63:0"]);
        assert!(waits.is_empty());
    }
}

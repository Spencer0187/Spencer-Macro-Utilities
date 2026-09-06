//! Input-domain services for SMU.
//!
//! This crate deliberately excludes screen bounds, cursor reads, and pixel sampling. Those lived on
//! the old C++ `InputBackend`, but belong to the display/capture service in the Rust architecture.

use smu_core::{Hotkey, KeyCode};

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "linux")]
mod linux_permissions;
#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "linux")]
pub use linux_permissions::{LinuxInputPermissionStatus, linux_input_permission_status};
pub mod state;
pub mod text;

pub use state::{HotkeyMatch, KeyState, is_hotkey_pressed};
pub use text::{
    FALLBACK_SCAN_CODE_NONE, TextKeyAction, TextPlanEvent, compatibility_key_action,
    execute_fallback_text, fallback_text_plan,
};

pub const NO_KEY: KeyCode = KeyCode::NONE;
pub const MOUSE_WHEEL_UP: KeyCode = KeyCode::MOUSE_WHEEL_UP;
pub const MOUSE_WHEEL_DOWN: KeyCode = KeyCode::MOUSE_WHEEL_DOWN;

#[cfg(any(target_os = "windows", test))]
pub(crate) const fn windows_virtual_key(key: KeyCode) -> Option<i32> {
    let raw = key.raw();
    if matches!(key, KeyCode::MOUSE_WHEEL_UP | KeyCode::MOUSE_WHEEL_DOWN) || raw > 0xFF {
        None
    } else {
        Some(raw.cast_signed())
    }
}

#[cfg(any(target_os = "windows", test))]
pub(crate) const fn windows_is_mouse_button_virtual_key(vk: u16) -> bool {
    vk >= 0x01 && vk <= 0x06
}

#[cfg(any(target_os = "windows", test))]
pub(crate) const fn windows_is_extended_virtual_key(vk: u16) -> bool {
    matches!(
        vk,
        0xA3 | 0xA5 | 0x2D | 0x2E | 0x24 | 0x23 | 0x21 | 0x22 | 0x25 | 0x26 | 0x27 | 0x28 | 0x90
    )
}

#[cfg(any(target_os = "windows", test))]
pub(crate) const fn windows_modifier_scan_code(vk: u16) -> Option<u16> {
    match vk {
        0x10 | 0xA0 => Some(0x2A),
        0xA1 => Some(0x36),
        0x11 | 0xA2 | 0xA3 => Some(0x1D),
        0x12 | 0xA4 | 0xA5 => Some(0x38),
        _ => None,
    }
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn windows_scale_relative_delta(delta: i32, scale_percent: i32) -> i32 {
    let scaled = i64::from(delta) * i64::from(scale_percent.clamp(10, 500)) / 100;
    #[allow(clippy::cast_possible_truncation)]
    {
        scaled as i32
    }
}

#[cfg(any(target_os = "windows", test))]
pub(crate) const fn windows_wheel_direction(delta: i16) -> Option<KeyCode> {
    if delta > 0 {
        Some(KeyCode::MOUSE_WHEEL_UP)
    } else if delta < 0 {
        Some(KeyCode::MOUSE_WHEEL_DOWN)
    } else {
        None
    }
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn windows_normalize_absolute_coordinate(
    value: i32,
    origin: i32,
    extent: i32,
) -> Option<i32> {
    if extent <= 0 {
        return None;
    }
    let maximum = i64::from(origin) + i64::from(extent) - 1;
    let clamped = i64::from(value).clamp(i64::from(origin), maximum);
    let denominator = i64::from((extent - 1).max(1));
    let numerator = (clamped - i64::from(origin)) * 65_535;
    let normalized = (numerator + denominator / 2) / denominator;
    i32::try_from(normalized).ok()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputError(String);

impl InputError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for InputError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for InputError {}

/// Physical/injected output operations only. Display queries intentionally do not appear here.
pub trait InputInjector: Send + Sync {
    fn hold_key(&self, key: KeyCode, extended: bool);
    fn release_key(&self, key: KeyCode, extended: bool);
    fn move_mouse(&self, dx: i32, dy: i32);

    fn move_mouse_raw(&self, dx: i32, dy: i32) {
        self.move_mouse(dx, dy);
    }

    /// Move the pointer to one global screen coordinate.
    ///
    /// # Errors
    ///
    /// Returns the platform/session absolute-pointer failure when unsupported.
    fn move_mouse_absolute(&self, _x: i32, _y: i32) -> Result<(), InputError> {
        Err(InputError::new(self.absolute_pointer_unavailable_reason()))
    }

    fn prefers_direct_absolute_positioning(&self) -> bool {
        false
    }

    fn absolute_pointer_unavailable_reason(&self) -> String {
        "cursor/screen position is not available on this platform/session".to_owned()
    }

    fn mouse_wheel(&self, delta: i32);

    /// Try the platform's active keyboard-layout character path. Returns `false` when unavailable so
    /// callers can use the compatibility fallback mapping.
    fn press_character(&self, _character: u8, _delay_ms: i32) -> bool {
        false
    }

    /// Inject a packed SMU hotkey in the same order as C++ `holdKeyChord`.
    fn hold_key_chord(&self, hotkey: Hotkey) {
        let key = hotkey.key();
        if key == KeyCode::MOUSE_WHEEL_UP {
            self.mouse_wheel(1);
            return;
        }
        if key == KeyCode::MOUSE_WHEEL_DOWN {
            self.mouse_wheel(-1);
            return;
        }
        for key in hotkey.expanded_keys() {
            self.hold_key(key, false);
        }
    }

    /// Release a packed SMU hotkey in the reverse order used by C++ `releaseKeyChord`.
    fn release_key_chord(&self, hotkey: Hotkey) {
        let wheel = matches!(hotkey.key(), KeyCode::MOUSE_WHEEL_UP | KeyCode::MOUSE_WHEEL_DOWN);
        for key in hotkey.expanded_keys().into_iter().rev() {
            if wheel && matches!(key, KeyCode::MOUSE_WHEEL_UP | KeyCode::MOUSE_WHEEL_DOWN) {
                continue;
            }
            self.release_key(key, false);
        }
    }
}

/// Compatibility facade for consumers that need both physical state and injection.
pub trait InputDevice: KeyState + InputInjector {
    fn format_key_name(&self, key: KeyCode) -> String {
        key.legacy_label()
    }
}

impl<T: KeyState + InputInjector> InputDevice for T {}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{Hotkey, InputDevice, InputInjector, KeyCode, KeyState};
    use std::sync::Mutex;

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

    struct RecordingDevice {
        events: Mutex<Vec<String>>,
    }

    impl RecordingDevice {
        fn new() -> Self {
            Self { events: Mutex::new(Vec::new()) }
        }
    }

    impl KeyState for RecordingDevice {
        fn is_pressed(&self, _key: KeyCode) -> bool {
            false
        }
    }

    impl InputInjector for RecordingDevice {
        fn hold_key(&self, key: KeyCode, _extended: bool) {
            self.events.lock().expect("event mutex").push(format!("down:{}", key.raw()));
        }
        fn release_key(&self, key: KeyCode, _extended: bool) {
            self.events.lock().expect("event mutex").push(format!("up:{}", key.raw()));
        }
        fn move_mouse(&self, _dx: i32, _dy: i32) {}
        fn mouse_wheel(&self, delta: i32) {
            self.events.lock().expect("event mutex").push(format!("wheel:{delta}"));
        }
    }

    #[test]
    fn packed_chord_injection_matches_cpp_modifier_and_release_order() {
        let device = RecordingDevice::new();
        let hotkey = Hotkey::new(0xF_0000 | 0x41);
        device.hold_key_chord(hotkey);
        device.release_key_chord(hotkey);
        assert_eq!(
            *device.events.lock().expect("event mutex"),
            [
                "down:91", "down:17", "down:18", "down:16", "down:65", "up:65", "up:16", "up:18",
                "up:17", "up:91",
            ]
        );
    }

    #[test]
    fn wheel_chord_matches_cpp_hold_and_modifier_release_quirk() {
        let device = RecordingDevice::new();
        let up = Hotkey::new(KeyCode::MOUSE_WHEEL_UP.raw());
        device.hold_key_chord(up);
        device.release_key_chord(up);
        let down = Hotkey::new(KeyCode::MOUSE_WHEEL_DOWN.raw());
        device.hold_key_chord(down);
        device.release_key_chord(down);
        let shift_wheel = Hotkey::new(0x1_0000 | KeyCode::MOUSE_WHEEL_UP.raw());
        device.hold_key_chord(shift_wheel);
        device.release_key_chord(shift_wheel);
        assert_eq!(
            *device.events.lock().expect("event mutex"),
            ["wheel:1", "wheel:-1", "wheel:1", "up:16"]
        );
    }

    #[test]
    fn default_key_name_uses_core_compatibility_label() {
        assert_eq!(MinimalDevice.format_key_name(KeyCode::new(0x74)), "F5");
        assert_eq!(MinimalDevice.format_key_name(KeyCode::new(999)), "0x999");
    }

    #[test]
    fn windows_virtual_key_rejects_wheel_and_out_of_range_values() {
        assert_eq!(super::windows_virtual_key(KeyCode::A), Some(0x41));
        assert_eq!(super::windows_virtual_key(KeyCode::LBUTTON), Some(0x01));
        assert_eq!(super::windows_virtual_key(KeyCode::RBUTTON), Some(0x02));
        assert_eq!(super::windows_virtual_key(KeyCode::MOUSE_WHEEL_UP), None);
        assert_eq!(super::windows_virtual_key(KeyCode::MOUSE_WHEEL_DOWN), None);
        assert_eq!(super::windows_virtual_key(KeyCode::new(0x1FF)), None);
    }

    #[test]
    fn windows_sendinput_key_classification_matches_cpp_backend() {
        assert!(super::windows_is_mouse_button_virtual_key(0x01));
        assert!(super::windows_is_mouse_button_virtual_key(0x03));
        assert!(super::windows_is_mouse_button_virtual_key(0x06));
        assert!(!super::windows_is_mouse_button_virtual_key(0x07));

        assert!(super::windows_is_extended_virtual_key(0xA3));
        assert!(super::windows_is_extended_virtual_key(0x25));
        assert!(super::windows_is_extended_virtual_key(0x90));
        assert!(!super::windows_is_extended_virtual_key(0xA2));
        assert!(!super::windows_is_extended_virtual_key(0x41));

        assert_eq!(super::windows_modifier_scan_code(0x10), Some(0x2A));
        assert_eq!(super::windows_modifier_scan_code(0xA1), Some(0x36));
        assert_eq!(super::windows_modifier_scan_code(0xA3), Some(0x1D));
        assert_eq!(super::windows_modifier_scan_code(0xA5), Some(0x38));
        assert_eq!(super::windows_modifier_scan_code(0x41), None);
    }

    #[test]
    fn windows_relative_mouse_scaling_matches_cpp_display_scale_rule() {
        assert_eq!(super::windows_scale_relative_delta(100, 100), 100);
        assert_eq!(super::windows_scale_relative_delta(100, 250), 250);
        assert_eq!(super::windows_scale_relative_delta(-9, 150), -13);
        assert_eq!(super::windows_scale_relative_delta(100, 1), 10);
        assert_eq!(super::windows_scale_relative_delta(100, 999), 500);
    }

    #[test]
    fn windows_wheel_direction_matches_cpp_transient_binding_codes() {
        assert_eq!(super::windows_wheel_direction(120), Some(KeyCode::MOUSE_WHEEL_UP));
        assert_eq!(super::windows_wheel_direction(-120), Some(KeyCode::MOUSE_WHEEL_DOWN));
        assert_eq!(super::windows_wheel_direction(0), None);
    }

    #[test]
    fn windows_absolute_normalization_matches_virtual_desktop_sendinput_rule() {
        assert_eq!(super::windows_normalize_absolute_coordinate(-1920, -1920, 1920), Some(0));
        assert_eq!(super::windows_normalize_absolute_coordinate(-1, -1920, 1920), Some(65_535));
        assert_eq!(super::windows_normalize_absolute_coordinate(-9999, -1920, 1920), Some(0));
        assert_eq!(super::windows_normalize_absolute_coordinate(9999, -1920, 1920), Some(65_535));
        assert_eq!(super::windows_normalize_absolute_coordinate(50, 50, 1), Some(0));
        assert_eq!(super::windows_normalize_absolute_coordinate(0, 0, 0), None);
    }
}

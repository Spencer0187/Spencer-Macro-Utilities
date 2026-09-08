//! Input-domain services for SMU.
//!
//! This crate deliberately excludes screen bounds, cursor reads, and pixel sampling. Those lived on
//! the old C++ `InputBackend`, but belong to the display/capture service in the Rust architecture.

use smu_core::{Hotkey, KeyCode};

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "linux")]
mod linux_permissions;
#[cfg(target_os = "macos")]
pub mod macos;
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

#[cfg(any(target_os = "macos", test))]
pub(crate) const MACOS_MOUSE_LEFT: u16 = 0xFF;
#[cfg(any(target_os = "macos", test))]
pub(crate) const MACOS_MOUSE_RIGHT: u16 = 0xFE;
#[cfg(any(target_os = "macos", test))]
pub(crate) const MACOS_MOUSE_MIDDLE: u16 = 0xFD;
#[cfg(any(target_os = "macos", test))]
pub(crate) const MACOS_MOUSE_X1: u16 = 0xFC;
#[cfg(any(target_os = "macos", test))]
pub(crate) const MACOS_MOUSE_X2: u16 = 0xFB;

#[cfg(any(target_os = "macos", test))]
const MACOS_KEY_MAP: &[(u32, u16)] = &[
    (0x41, 0x00),
    (0x53, 0x01),
    (0x44, 0x02),
    (0x46, 0x03),
    (0x48, 0x04),
    (0x47, 0x05),
    (0x5A, 0x06),
    (0x58, 0x07),
    (0x43, 0x08),
    (0x56, 0x09),
    (0x42, 0x0B),
    (0x51, 0x0C),
    (0x57, 0x0D),
    (0x45, 0x0E),
    (0x52, 0x0F),
    (0x59, 0x10),
    (0x54, 0x11),
    (0x31, 0x12),
    (0x32, 0x13),
    (0x33, 0x14),
    (0x34, 0x15),
    (0x36, 0x16),
    (0x35, 0x17),
    (0xBB, 0x18),
    (0x39, 0x19),
    (0x37, 0x1A),
    (0xBD, 0x1B),
    (0x38, 0x1C),
    (0x30, 0x1D),
    (0xDD, 0x1E),
    (0x4F, 0x1F),
    (0x55, 0x20),
    (0xDB, 0x21),
    (0x49, 0x22),
    (0x50, 0x23),
    (0x0D, 0x24),
    (0x4C, 0x25),
    (0x4A, 0x26),
    (0xDE, 0x27),
    (0x4B, 0x28),
    (0xBA, 0x29),
    (0xDC, 0x2A),
    (0xBC, 0x2B),
    (0xBF, 0x2C),
    (0x4E, 0x2D),
    (0x4D, 0x2E),
    (0xBE, 0x2F),
    (0x09, 0x30),
    (0x20, 0x31),
    (0xC0, 0x32),
    (0x08, 0x33),
    (0x1B, 0x35),
    (0x5B, 0x37),
    (0x10, 0x38),
    (0x14, 0x39),
    (0x12, 0x3A),
    (0x11, 0x3B),
    (0x6E, 0x41),
    (0x6A, 0x43),
    (0x6B, 0x45),
    (0x6F, 0x4B),
    (0x6D, 0x4E),
    (0x60, 0x52),
    (0x61, 0x53),
    (0x62, 0x54),
    (0x63, 0x55),
    (0x64, 0x56),
    (0x65, 0x57),
    (0x66, 0x58),
    (0x67, 0x59),
    (0x68, 0x5B),
    (0x69, 0x5C),
    (0x74, 0x60),
    (0x75, 0x61),
    (0x76, 0x62),
    (0x72, 0x63),
    (0x77, 0x64),
    (0x78, 0x65),
    (0x7A, 0x67),
    (0x7C, 0x69),
    (0x7D, 0x6B),
    (0x79, 0x6D),
    (0x7B, 0x6F),
    (0x7E, 0x71),
    (0x2D, 0x72),
    (0x24, 0x73),
    (0x21, 0x74),
    (0x2E, 0x75),
    (0x73, 0x76),
    (0x23, 0x77),
    (0x71, 0x78),
    (0x22, 0x79),
    (0x70, 0x7A),
    (0x25, 0x7B),
    (0x27, 0x7C),
    (0x28, 0x7D),
    (0x26, 0x7E),
];

#[cfg(any(target_os = "macos", test))]
pub(crate) fn macos_key_code(key: KeyCode) -> Option<u16> {
    match key.raw() {
        0x01 => Some(MACOS_MOUSE_LEFT),
        0x02 => Some(MACOS_MOUSE_RIGHT),
        0x04 => Some(MACOS_MOUSE_MIDDLE),
        0x05 => Some(MACOS_MOUSE_X1),
        0x06 => Some(MACOS_MOUSE_X2),
        raw => {
            MACOS_KEY_MAP.iter().find_map(|(windows, macos)| (*windows == raw).then_some(*macos))
        }
    }
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn macos_windows_key_code(mac_key: u16) -> Option<KeyCode> {
    match mac_key {
        0x36 => Some(KeyCode::RWIN),
        0x3C => Some(KeyCode::SHIFT),
        0x3D => Some(KeyCode::MENU),
        0x3E => Some(KeyCode::CONTROL),
        0x4C => Some(KeyCode::RETURN),
        _ => MACOS_KEY_MAP
            .iter()
            .find_map(|(windows, macos)| (*macos == mac_key).then_some(KeyCode::new(*windows))),
    }
}

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

    /// Toggle the legacy macOS visible-cursor movement mode. Other platform injectors keep this unsupported.
    fn set_direct_cursor_movement(&self, _enabled: bool) -> bool {
        false
    }

    fn direct_cursor_movement(&self) -> bool {
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

    /// Try the platform's direct Unicode text-injection compatibility path. Windows implements this
    /// with `KEYEVENTF_UNICODE`; other platforms return `false` so callers keep their normal text path.
    fn type_unicode_text(&self, _text: &str, _delay_ms: i32) -> bool {
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
    fn macos_key_mapping_matches_cpp_virtual_key_table() {
        assert_eq!(super::macos_key_code(KeyCode::A), Some(0x00));
        assert_eq!(super::macos_key_code(KeyCode::new(0x5A)), Some(0x06));
        assert_eq!(super::macos_key_code(KeyCode::new(0x0D)), Some(0x24));
        assert_eq!(super::macos_key_code(KeyCode::new(0x10)), Some(0x38));
        assert_eq!(super::macos_key_code(KeyCode::new(0x11)), Some(0x3B));
        assert_eq!(super::macos_key_code(KeyCode::new(0x12)), Some(0x3A));
        assert_eq!(super::macos_key_code(KeyCode::LBUTTON), Some(super::MACOS_MOUSE_LEFT));
        assert_eq!(super::macos_key_code(KeyCode::RBUTTON), Some(super::MACOS_MOUSE_RIGHT));
        assert_eq!(super::macos_key_code(KeyCode::new(0x04)), Some(super::MACOS_MOUSE_MIDDLE));
        assert_eq!(super::macos_key_code(KeyCode::MOUSE_WHEEL_UP), None);
        assert_eq!(super::macos_key_code(KeyCode::new(0x1FF)), None);

        assert_eq!(super::macos_windows_key_code(0x00), Some(KeyCode::A));
        assert_eq!(super::macos_windows_key_code(0x36), Some(KeyCode::RWIN));
        assert_eq!(super::macos_windows_key_code(0x3C), Some(KeyCode::SHIFT));
        assert_eq!(super::macos_windows_key_code(0x3D), Some(KeyCode::MENU));
        assert_eq!(super::macos_windows_key_code(0x3E), Some(KeyCode::CONTROL));
        assert_eq!(super::macos_windows_key_code(0x4C), Some(KeyCode::RETURN));
        assert_eq!(super::macos_windows_key_code(0xFFFF), None);
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

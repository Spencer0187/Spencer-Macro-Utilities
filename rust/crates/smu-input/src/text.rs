//! Compatibility text-entry mapping from `platform/text_input_backend.cpp`.

use smu_core::KeyCode;

pub const FALLBACK_SCAN_CODE_NONE: u32 = 0;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TextKeyAction {
    pub key: KeyCode,
    pub scan_code: u32,
    pub needs_shift: bool,
    pub valid: bool,
}

impl TextKeyAction {
    const fn new(key: KeyCode, scan_code: u32, needs_shift: bool) -> Self {
        Self { key, scan_code, needs_shift, valid: key.raw() != 0 }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextPlanEvent {
    Hold(KeyCode),
    WaitMs(i32),
    Release(KeyCode),
}

const LETTER_SCAN_CODES: [u32; 26] = [
    0x1E, 0x30, 0x2E, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31, 0x18, 0x19,
    0x10, 0x13, 0x1F, 0x14, 0x16, 0x2F, 0x11, 0x2D, 0x15, 0x2C,
];

const fn scan_code_for_key(key: KeyCode) -> u32 {
    let raw = key.raw();
    if raw >= b'A' as u32 && raw <= b'Z' as u32 {
        return LETTER_SCAN_CODES[(raw - b'A' as u32) as usize];
    }

    match raw {
        0x31 => 0x02,
        0x32 => 0x03,
        0x33 => 0x04,
        0x34 => 0x05,
        0x35 => 0x06,
        0x36 => 0x07,
        0x37 => 0x08,
        0x38 => 0x09,
        0x39 => 0x0A,
        0x30 => 0x0B,
        0x20 => 0x39,
        0x0D => 0x1C,
        0x09 => 0x0F,
        0xC0 => 0x29,
        0xBD => 0x0C,
        0xBB => 0x0D,
        0xDB => 0x1A,
        0xDD => 0x1B,
        0xDC => 0x2B,
        0xBA => 0x27,
        0xDE => 0x28,
        0xBC => 0x33,
        0xBE => 0x34,
        0xBF => 0x35,
        _ => FALLBACK_SCAN_CODE_NONE,
    }
}

const fn action(key: KeyCode, needs_shift: bool) -> TextKeyAction {
    TextKeyAction::new(key, scan_code_for_key(key), needs_shift)
}

/// Exact US-layout fallback mapping used when a native keyboard-layout service cannot emit a byte.
///
/// The C++ API accepts a `char`, not a Unicode scalar, so this deliberately only maps ASCII.
#[must_use]
pub const fn compatibility_key_action(character: u8) -> TextKeyAction {
    match character {
        b'a'..=b'z' => action(KeyCode::new((character - b'a' + b'A') as u32), false),
        b'A'..=b'Z' => action(KeyCode::new(character as u32), true),
        b'0'..=b'9' => action(KeyCode::new(character as u32), false),
        b' ' => action(KeyCode::SPACE, false),
        b'\n' => action(KeyCode::RETURN, false),
        b'\t' => action(KeyCode::TAB, false),
        b'!' => action(KeyCode::new(0x31), true),
        b'@' => action(KeyCode::new(0x32), true),
        b'#' => action(KeyCode::new(0x33), true),
        b'$' => action(KeyCode::new(0x34), true),
        b'%' => action(KeyCode::new(0x35), true),
        b'^' => action(KeyCode::new(0x36), true),
        b'&' => action(KeyCode::new(0x37), true),
        b'*' => action(KeyCode::new(0x38), true),
        b'(' => action(KeyCode::new(0x39), true),
        b')' => action(KeyCode::new(0x30), true),
        b'`' => action(KeyCode::OEM_3, false),
        b'-' => action(KeyCode::OEM_MINUS, false),
        b'=' => action(KeyCode::OEM_PLUS, false),
        b'[' => action(KeyCode::OEM_4, false),
        b']' => action(KeyCode::OEM_6, false),
        b'\\' => action(KeyCode::OEM_5, false),
        b';' => action(KeyCode::OEM_1, false),
        b'\'' => action(KeyCode::OEM_7, false),
        b',' => action(KeyCode::OEM_COMMA, false),
        b'.' => action(KeyCode::OEM_PERIOD, false),
        b'/' => action(KeyCode::OEM_2, false),
        b'~' => action(KeyCode::OEM_3, true),
        b'_' => action(KeyCode::OEM_MINUS, true),
        b'+' => action(KeyCode::OEM_PLUS, true),
        b'{' => action(KeyCode::OEM_4, true),
        b'}' => action(KeyCode::OEM_6, true),
        b'|' => action(KeyCode::OEM_5, true),
        b':' => action(KeyCode::OEM_1, true),
        b'"' => action(KeyCode::OEM_7, true),
        b'<' => action(KeyCode::OEM_COMMA, true),
        b'>' => action(KeyCode::OEM_PERIOD, true),
        b'?' => action(KeyCode::OEM_2, true),
        _ => TextKeyAction { key: KeyCode::NONE, scan_code: 0, needs_shift: false, valid: false },
    }
}

/// Produce the fallback key/wait sequence used by C++ `typeText` after native-layout handling fails.
/// Invalid bytes are silently skipped, and negative delays clamp to zero.
#[must_use]
pub fn fallback_text_plan(text: &[u8], delay_ms: i32) -> Vec<TextPlanEvent> {
    let safe_delay_ms = delay_ms.max(0);
    let mut events = Vec::with_capacity(text.len().saturating_mul(5));
    for &character in text {
        let action = compatibility_key_action(character);
        if !action.valid {
            continue;
        }
        if action.needs_shift {
            events.push(TextPlanEvent::Hold(KeyCode::SHIFT));
        }
        events.push(TextPlanEvent::Hold(action.key));
        events.push(TextPlanEvent::WaitMs(safe_delay_ms));
        events.push(TextPlanEvent::Release(action.key));
        if action.needs_shift {
            events.push(TextPlanEvent::Release(KeyCode::SHIFT));
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use super::{
        TextKeyAction, TextPlanEvent, compatibility_key_action, fallback_text_plan,
        scan_code_for_key,
    };
    use smu_core::KeyCode;

    #[test]
    fn letter_scan_codes_match_cpp_table() {
        assert_eq!(scan_code_for_key(KeyCode::A), 0x1E);
        assert_eq!(scan_code_for_key(KeyCode::new(u32::from(b'M'))), 0x32);
        assert_eq!(scan_code_for_key(KeyCode::Z), 0x2C);
    }

    #[test]
    fn representative_character_mapping_matches_cpp() {
        assert_eq!(
            compatibility_key_action(b'a'),
            TextKeyAction { key: KeyCode::A, scan_code: 0x1E, needs_shift: false, valid: true }
        );
        assert_eq!(
            compatibility_key_action(b'A'),
            TextKeyAction { key: KeyCode::A, scan_code: 0x1E, needs_shift: true, valid: true }
        );
        assert_eq!(
            compatibility_key_action(b'?'),
            TextKeyAction { key: KeyCode::OEM_2, scan_code: 0x35, needs_shift: true, valid: true }
        );
        assert!(!compatibility_key_action(0).valid);
        assert!(!compatibility_key_action(0xFF).valid);
    }

    #[test]
    fn every_printable_ascii_byte_is_supported() {
        for byte in 0x20_u8..=0x7E {
            assert!(compatibility_key_action(byte).valid, "ASCII byte 0x{byte:02X}");
        }
        assert!(compatibility_key_action(b'\n').valid);
        assert!(compatibility_key_action(b'\t').valid);
    }

    #[test]
    fn fallback_plan_preserves_shift_order_and_delay() {
        assert_eq!(
            fallback_text_plan(b"aA?", 25),
            [
                TextPlanEvent::Hold(KeyCode::A),
                TextPlanEvent::WaitMs(25),
                TextPlanEvent::Release(KeyCode::A),
                TextPlanEvent::Hold(KeyCode::SHIFT),
                TextPlanEvent::Hold(KeyCode::A),
                TextPlanEvent::WaitMs(25),
                TextPlanEvent::Release(KeyCode::A),
                TextPlanEvent::Release(KeyCode::SHIFT),
                TextPlanEvent::Hold(KeyCode::SHIFT),
                TextPlanEvent::Hold(KeyCode::OEM_2),
                TextPlanEvent::WaitMs(25),
                TextPlanEvent::Release(KeyCode::OEM_2),
                TextPlanEvent::Release(KeyCode::SHIFT),
            ]
        );
    }

    #[test]
    fn fallback_plan_clamps_negative_delay_and_skips_invalid_bytes() {
        assert_eq!(
            fallback_text_plan(&[0, b'1', 0xFF], -99),
            [
                TextPlanEvent::Hold(KeyCode::new(0x31)),
                TextPlanEvent::WaitMs(0),
                TextPlanEvent::Release(KeyCode::new(0x31)),
            ]
        );
    }
}

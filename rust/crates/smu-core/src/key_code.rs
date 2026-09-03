//! SMU key-code compatibility values.

use std::fmt;

/// A key code using SMU's historical, Windows-VK-shaped numeric representation.
///
/// This remains an open numeric type instead of an enum because settings/scripts may contain values
/// unknown to a particular build and the C++ implementation accepts those values.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct KeyCode(u32);

impl KeyCode {
    pub const NONE: Self = Self(0x00);
    pub const LBUTTON: Self = Self(0x01);
    pub const RBUTTON: Self = Self(0x02);
    pub const CANCEL: Self = Self(0x03);
    pub const MBUTTON: Self = Self(0x04);
    pub const XBUTTON1: Self = Self(0x05);
    pub const XBUTTON2: Self = Self(0x06);
    pub const BACK: Self = Self(0x08);
    pub const TAB: Self = Self(0x09);
    pub const RETURN: Self = Self(0x0D);
    pub const SHIFT: Self = Self(0x10);
    pub const CONTROL: Self = Self(0x11);
    pub const MENU: Self = Self(0x12);
    pub const PAUSE: Self = Self(0x13);
    pub const CAPITAL: Self = Self(0x14);
    pub const ESCAPE: Self = Self(0x1B);
    pub const SPACE: Self = Self(0x20);
    pub const PRIOR: Self = Self(0x21);
    pub const NEXT: Self = Self(0x22);
    pub const END: Self = Self(0x23);
    pub const HOME: Self = Self(0x24);
    pub const LEFT: Self = Self(0x25);
    pub const UP: Self = Self(0x26);
    pub const RIGHT: Self = Self(0x27);
    pub const DOWN: Self = Self(0x28);
    pub const SNAPSHOT: Self = Self(0x2C);
    pub const INSERT: Self = Self(0x2D);
    pub const DELETE: Self = Self(0x2E);

    pub const DIGIT_0: Self = Self(0x30);
    pub const DIGIT_9: Self = Self(0x39);
    pub const A: Self = Self(0x41);
    pub const Z: Self = Self(0x5A);

    pub const LWIN: Self = Self(0x5B);
    pub const RWIN: Self = Self(0x5C);
    pub const APPS: Self = Self(0x5D);
    pub const NUMPAD0: Self = Self(0x60);
    pub const F1: Self = Self(0x70);
    pub const F12: Self = Self(0x7B);
    pub const NUMLOCK: Self = Self(0x90);
    pub const SCROLL: Self = Self(0x91);
    pub const LSHIFT: Self = Self(0xA0);
    pub const RSHIFT: Self = Self(0xA1);
    pub const LCONTROL: Self = Self(0xA2);
    pub const RCONTROL: Self = Self(0xA3);
    pub const LMENU: Self = Self(0xA4);
    pub const RMENU: Self = Self(0xA5);
    pub const OEM_1: Self = Self(0xBA);
    pub const OEM_PLUS: Self = Self(0xBB);
    pub const OEM_COMMA: Self = Self(0xBC);
    pub const OEM_MINUS: Self = Self(0xBD);
    pub const OEM_PERIOD: Self = Self(0xBE);
    pub const OEM_2: Self = Self(0xBF);
    pub const OEM_3: Self = Self(0xC0);
    pub const OEM_4: Self = Self(0xDB);
    pub const OEM_5: Self = Self(0xDC);
    pub const OEM_6: Self = Self(0xDD);
    pub const OEM_7: Self = Self(0xDE);
    pub const OEM_8: Self = Self(0xDF);

    pub const MOUSE_WHEEL_UP: Self = Self(256);
    pub const MOUSE_WHEEL_DOWN: Self = Self(257);

    /// Constructs a key code without rejecting unknown compatibility values.
    #[must_use]
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns the compatibility numeric value.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// Matches `smu::core::IsModifierKey` from the C++ oracle.
    #[must_use]
    pub const fn is_modifier(self) -> bool {
        matches!(self.0, 0x10 | 0xA0 | 0xA1 | 0x11 | 0xA2 | 0xA3 | 0x12 | 0xA4 | 0xA5 | 0x5B | 0x5C)
    }

    /// Returns the explicit name used by the current C++ UI, if it has one.
    #[must_use]
    pub const fn name(self) -> Option<&'static str> {
        match self.0 {
            0x01 => Some("Mouse Left"),
            0x02 => Some("Mouse Right"),
            0x04 => Some("Mouse Middle"),
            0x05 => Some("Mouse X1"),
            0x06 => Some("Mouse X2"),
            0x08 => Some("Backspace"),
            0x09 => Some("Tab"),
            0x0D => Some("Enter"),
            0x10 => Some("Shift"),
            0x11 => Some("Ctrl"),
            0x12 => Some("Alt"),
            0x13 => Some("Pause"),
            0x14 => Some("Caps Lock"),
            0x1B => Some("Escape"),
            0x20 => Some("Space"),
            0x21 => Some("Page Up"),
            0x22 => Some("Page Down"),
            0x23 => Some("End"),
            0x24 => Some("Home"),
            0x25 => Some("Left"),
            0x26 => Some("Up"),
            0x27 => Some("Right"),
            0x28 => Some("Down"),
            0x2C => Some("Print Screen"),
            0x2D => Some("Insert"),
            0x2E => Some("Delete"),
            0x5B => Some("Left Super"),
            0x5C => Some("Right Super"),
            0x5D => Some("Menu"),
            0x90 => Some("Num Lock"),
            0x91 => Some("Scroll Lock"),
            0xA0 => Some("Left Shift"),
            0xA1 => Some("Right Shift"),
            0xA2 => Some("Left Ctrl"),
            0xA3 => Some("Right Ctrl"),
            0xA4 => Some("Left Alt"),
            0xA5 => Some("Right Alt"),
            0xBA => Some(";"),
            0xBB => Some("="),
            0xBC => Some(","),
            0xBD => Some("-"),
            0xBE => Some("."),
            0xBF => Some("/"),
            0xC0 => Some("`"),
            0xDB => Some("["),
            0xDC => Some("\\"),
            0xDD => Some("]"),
            0xDE => Some("'"),
            0xDF => Some("VK_OEM_8"),
            256 => Some("Mouse Wheel Up"),
            257 => Some("Mouse Wheel Down"),
            _ => None,
        }
    }

    /// Reproduces the C++ `FormatKeyCodeFallback` output exactly, including its historical
    /// `"0x" + decimal` fallback for unknown values.
    #[must_use]
    pub fn legacy_label(self) -> String {
        if (Self::A.0..=Self::Z.0).contains(&self.0) {
            return char::from_u32(self.0).map_or_else(String::new, |value| value.to_string());
        }
        if (Self::DIGIT_0.0..=Self::DIGIT_9.0).contains(&self.0) {
            return char::from_u32(self.0).map_or_else(String::new, |value| value.to_string());
        }
        if (Self::F1.0..=Self::F12.0).contains(&self.0) {
            return format!("F{}", self.0 - Self::F1.0 + 1);
        }
        if (Self::NUMPAD0.0..=Self::NUMPAD0.0 + 9).contains(&self.0) {
            return format!("Numpad {}", self.0 - Self::NUMPAD0.0);
        }
        if let Some(name) = self.name() {
            return name.to_owned();
        }
        format!("0x{}", self.0)
    }
}

impl From<u32> for KeyCode {
    fn from(value: u32) -> Self {
        Self::new(value)
    }
}

impl From<KeyCode> for u32 {
    fn from(value: KeyCode) -> Self {
        value.raw()
    }
}

impl fmt::Display for KeyCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.legacy_label())
    }
}

#[cfg(test)]
mod tests {
    use super::KeyCode;

    #[test]
    fn modifier_detection_matches_cpp_contract() {
        assert!(KeyCode::SHIFT.is_modifier());
        assert!(KeyCode::LCONTROL.is_modifier());
        assert!(KeyCode::RWIN.is_modifier());
        assert!(!KeyCode::A.is_modifier());
        assert!(!KeyCode::MOUSE_WHEEL_UP.is_modifier());
    }

    #[test]
    fn labels_match_cpp_contract() {
        assert_eq!(KeyCode::new(0x41).legacy_label(), "A");
        assert_eq!(KeyCode::new(0x39).legacy_label(), "9");
        assert_eq!(KeyCode::new(0x74).legacy_label(), "F5");
        assert_eq!(KeyCode::new(0x63).legacy_label(), "Numpad 3");
        assert_eq!(KeyCode::OEM_5.legacy_label(), "\\");
        assert_eq!(KeyCode::new(999).legacy_label(), "0x999");
    }
}

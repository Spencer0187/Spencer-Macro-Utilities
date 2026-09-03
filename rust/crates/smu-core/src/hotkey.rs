//! Packed hotkey compatibility representation.

use crate::KeyCode;

const MASK_SHIFT: u32 = 0x1_0000;
const MASK_CTRL: u32 = 0x2_0000;
const MASK_ALT: u32 = 0x4_0000;
const MASK_WIN: u32 = 0x8_0000;
const KEY_MASK: u32 = 0xFFFF;

/// SMU's persisted/script-visible packed hotkey value.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct Hotkey(u32);

impl Hotkey {
    /// Constructs a hotkey from its persisted raw value.
    #[must_use]
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns the persisted raw value unchanged.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// Returns the main key portion of the packed hotkey.
    #[must_use]
    pub const fn key(self) -> KeyCode {
        KeyCode::new(self.0 & KEY_MASK)
    }

    #[must_use]
    pub const fn has_shift(self) -> bool {
        self.0 & MASK_SHIFT != 0
    }

    #[must_use]
    pub const fn has_ctrl(self) -> bool {
        self.0 & MASK_CTRL != 0
    }

    #[must_use]
    pub const fn has_alt(self) -> bool {
        self.0 & MASK_ALT != 0
    }

    #[must_use]
    pub const fn has_win(self) -> bool {
        self.0 & MASK_WIN != 0
    }

    /// Matches the C++ script-manager definition of a bound hotkey.
    #[must_use]
    pub const fn is_bound(self) -> bool {
        self.0 & KEY_MASK != 0
    }

    /// Matches `ExpandScriptHotkeyToKeys`: Win, Ctrl, Alt, Shift, then the main key.
    #[must_use]
    pub fn expanded_keys(self) -> Vec<KeyCode> {
        let mut keys = Vec::with_capacity(5);
        if self.has_win() {
            keys.push(KeyCode::LWIN);
        }
        if self.has_ctrl() {
            keys.push(KeyCode::CONTROL);
        }
        if self.has_alt() {
            keys.push(KeyCode::MENU);
        }
        if self.has_shift() {
            keys.push(KeyCode::SHIFT);
        }
        let key = self.key();
        if key != KeyCode::NONE {
            keys.push(key);
        }
        keys
    }
}

#[cfg(test)]
mod tests {
    use super::{Hotkey, MASK_ALT, MASK_CTRL, MASK_SHIFT, MASK_WIN};
    use crate::KeyCode;

    #[test]
    fn packed_hotkey_expands_in_cpp_order() {
        let hotkey = Hotkey::new(MASK_SHIFT | MASK_CTRL | MASK_ALT | MASK_WIN | 0x41);
        assert_eq!(
            hotkey.expanded_keys(),
            [KeyCode::LWIN, KeyCode::CONTROL, KeyCode::MENU, KeyCode::SHIFT, KeyCode::A,]
        );
    }

    #[test]
    fn modifier_only_hotkey_is_unbound() {
        let hotkey = Hotkey::new(MASK_CTRL | MASK_SHIFT);
        assert!(!hotkey.is_bound());
        assert_eq!(hotkey.expanded_keys(), [KeyCode::CONTROL, KeyCode::SHIFT]);
    }
}

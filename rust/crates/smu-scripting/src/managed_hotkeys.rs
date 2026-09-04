//! Script-owned hotkey state from `ScriptInstance::managedSetHotkeyHeld`.

use smu_core::{Hotkey, KeyCode};
use std::collections::HashMap;

const SCRIPT_HOTKEY_MASK: u32 = 0x000F_FFFF;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedKeyAction {
    Hold(KeyCode),
    Release(KeyCode),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ManagedHotkeyResult {
    /// State returned by the C++ call: true after a hold, false after a release/rejected hotkey.
    pub down: bool,
    /// Physical key transitions required to realize the shared held-key counts.
    pub actions: Vec<ManagedKeyAction>,
}

/// Tracks script-owned hotkeys while reference-counting overlapping physical keys.
#[derive(Clone, Debug, Default)]
pub struct ManagedHotkeys {
    held_keys: HashMap<KeyCode, u32>,
    hotkey_states: HashMap<Hotkey, bool>,
}

impl ManagedHotkeys {
    const fn normalize(hotkey: Hotkey) -> Hotkey {
        Hotkey::new(hotkey.raw() & SCRIPT_HOTKEY_MASK)
    }

    #[must_use]
    pub fn is_held(&self, hotkey: Hotkey) -> bool {
        self.hotkey_states.get(&Self::normalize(hotkey)).copied().unwrap_or(false)
    }

    /// Apply C++ `managedSetHotkeyHeld` state transitions without performing OS input directly.
    #[must_use]
    pub fn set_held(&mut self, hotkey: Hotkey, down: bool) -> ManagedHotkeyResult {
        if !hotkey.is_bound() {
            return ManagedHotkeyResult::default();
        }

        let hotkey = Self::normalize(hotkey);
        let was_down = self.hotkey_states.get(&hotkey).copied().unwrap_or(false);
        if was_down == down {
            return ManagedHotkeyResult { down: was_down, actions: Vec::new() };
        }

        let keys = hotkey.expanded_keys();
        let mut actions = Vec::with_capacity(keys.len());
        if down {
            for key in keys {
                let count = self.held_keys.entry(key).or_default();
                if *count == 0 {
                    actions.push(ManagedKeyAction::Hold(key));
                }
                *count = count.saturating_add(1);
            }
            self.hotkey_states.insert(hotkey, true);
            return ManagedHotkeyResult { down: true, actions };
        }

        for key in keys.into_iter().rev() {
            let Some(count) = self.held_keys.get_mut(&key) else {
                continue;
            };
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.held_keys.remove(&key);
                actions.push(ManagedKeyAction::Release(key));
            }
        }
        self.hotkey_states.insert(hotkey, false);
        ManagedHotkeyResult { down: false, actions }
    }

    #[must_use]
    pub fn hold(&mut self, hotkey: Hotkey) -> ManagedHotkeyResult {
        self.set_held(hotkey, true)
    }

    #[must_use]
    pub fn release(&mut self, hotkey: Hotkey) -> ManagedHotkeyResult {
        self.set_held(hotkey, false)
    }

    #[must_use]
    pub fn toggle(&mut self, hotkey: Hotkey) -> ManagedHotkeyResult {
        let normalized = Self::normalize(hotkey);
        let down = !self.hotkey_states.get(&normalized).copied().unwrap_or(false);
        self.set_held(normalized, down)
    }

    /// Release every script-owned key. C++ uses unordered-map iteration here, so cleanup release
    /// order is intentionally not exposed as compatibility data; callers receive the release set.
    #[must_use]
    pub fn release_all(&mut self) -> Vec<KeyCode> {
        let mut keys = self.held_keys.keys().copied().collect::<Vec<_>>();
        keys.sort_unstable();
        self.held_keys.clear();
        self.hotkey_states.clear();
        keys
    }

    #[must_use]
    pub fn held_key_count(&self, key: KeyCode) -> u32 {
        self.held_keys.get(&key).copied().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::{ManagedHotkeys, ManagedKeyAction};
    use smu_core::{Hotkey, KeyCode};

    const CTRL: u32 = 0x2_0000;
    const SHIFT: u32 = 0x1_0000;

    #[test]
    fn hold_and_release_follow_cpp_modifier_and_reverse_release_order() {
        let mut managed = ManagedHotkeys::default();
        let hotkey = Hotkey::new(CTRL | SHIFT | KeyCode::A.raw());
        assert_eq!(
            managed.hold(hotkey).actions,
            [
                ManagedKeyAction::Hold(KeyCode::CONTROL),
                ManagedKeyAction::Hold(KeyCode::SHIFT),
                ManagedKeyAction::Hold(KeyCode::A),
            ]
        );
        assert_eq!(
            managed.release(hotkey).actions,
            [
                ManagedKeyAction::Release(KeyCode::A),
                ManagedKeyAction::Release(KeyCode::SHIFT),
                ManagedKeyAction::Release(KeyCode::CONTROL),
            ]
        );
    }

    #[test]
    fn repeated_state_is_idempotent_and_returns_existing_state() {
        let mut managed = ManagedHotkeys::default();
        let hotkey = Hotkey::new(KeyCode::A.raw());
        assert!(managed.hold(hotkey).down);
        let duplicate = managed.hold(hotkey);
        assert!(duplicate.down);
        assert!(duplicate.actions.is_empty());
        assert!(!managed.release(hotkey).down);
        let duplicate = managed.release(hotkey);
        assert!(!duplicate.down);
        assert!(duplicate.actions.is_empty());
    }

    #[test]
    fn overlapping_hotkeys_reference_count_shared_modifier() {
        let mut managed = ManagedHotkeys::default();
        let first = Hotkey::new(CTRL | KeyCode::A.raw());
        let second = Hotkey::new(CTRL | KeyCode::new(u32::from(b'B')).raw());
        assert_eq!(
            managed.hold(first).actions,
            [ManagedKeyAction::Hold(KeyCode::CONTROL), ManagedKeyAction::Hold(KeyCode::A)]
        );
        assert_eq!(
            managed.hold(second).actions,
            [ManagedKeyAction::Hold(KeyCode::new(u32::from(b'B')))]
        );
        assert_eq!(managed.held_key_count(KeyCode::CONTROL), 2);
        assert_eq!(managed.release(first).actions, [ManagedKeyAction::Release(KeyCode::A)]);
        assert_eq!(managed.held_key_count(KeyCode::CONTROL), 1);
        assert_eq!(
            managed.release(second).actions,
            [
                ManagedKeyAction::Release(KeyCode::new(u32::from(b'B'))),
                ManagedKeyAction::Release(KeyCode::CONTROL),
            ]
        );
    }

    #[test]
    fn unknown_high_bits_are_ignored_after_bound_check() {
        let mut managed = ManagedHotkeys::default();
        let raw = Hotkey::new(0xF000_0000 | CTRL | KeyCode::A.raw());
        assert!(managed.hold(raw).down);
        assert!(managed.is_held(Hotkey::new(CTRL | KeyCode::A.raw())));
    }

    #[test]
    fn modifier_only_hotkey_is_rejected() {
        let mut managed = ManagedHotkeys::default();
        let result = managed.hold(Hotkey::new(CTRL | SHIFT));
        assert!(!result.down);
        assert!(result.actions.is_empty());
    }

    #[test]
    fn toggle_matches_cpp_state_lookup() {
        let mut managed = ManagedHotkeys::default();
        let hotkey = Hotkey::new(KeyCode::A.raw());
        assert!(managed.toggle(hotkey).down);
        assert!(!managed.toggle(hotkey).down);
        assert!(managed.toggle(hotkey).down);
    }

    #[test]
    fn release_all_returns_each_physical_key_once() {
        let mut managed = ManagedHotkeys::default();
        let first = Hotkey::new(CTRL | KeyCode::A.raw());
        let second = Hotkey::new(CTRL | KeyCode::new(u32::from(b'B')).raw());
        let _ = managed.hold(first);
        let _ = managed.hold(second);
        assert_eq!(
            managed.release_all(),
            [KeyCode::CONTROL, KeyCode::A, KeyCode::new(u32::from(b'B'))]
        );
        assert_eq!(managed.held_key_count(KeyCode::CONTROL), 0);
        assert!(!managed.is_held(first));
    }
}

//! C++-compatible keybind capture state shared by app UI controls.

use dear_imgui_rs::Ui;
use smu_core::{Hotkey, KeyCode};
use smu_input::KeyState;

const HOTKEY_KEY_MASK: u32 = 0xFFFF;
const HOTKEY_MASK_SHIFT: u32 = 0x1_0000;
const HOTKEY_MASK_CTRL: u32 = 0x2_0000;
const HOTKEY_MASK_ALT: u32 = 0x4_0000;
const HOTKEY_MASK_WIN: u32 = 0x8_0000;
const KEY_STATE_COUNT: usize = 256;

#[derive(Clone, Debug)]
pub struct InputSnapshot {
    pressed: [bool; KEY_STATE_COUNT],
    transient: Option<KeyCode>,
}

impl Default for InputSnapshot {
    fn default() -> Self {
        Self { pressed: [false; KEY_STATE_COUNT], transient: None }
    }
}

impl InputSnapshot {
    #[must_use]
    pub fn capture(state: &dyn KeyState, consume_transient: bool) -> Self {
        let mut pressed = [false; KEY_STATE_COUNT];
        for (raw, slot) in pressed.iter_mut().enumerate().skip(1) {
            let Ok(raw) = u32::try_from(raw) else {
                continue;
            };
            *slot = state.is_pressed(KeyCode::new(raw));
        }
        let transient = consume_transient.then(|| state.consume_next_transient_input()).flatten();
        Self { pressed, transient }
    }

    #[must_use]
    const fn any_pressed(&self) -> bool {
        let mut index = 1;
        while index < KEY_STATE_COUNT {
            if self.pressed[index] {
                return true;
            }
            index += 1;
        }
        false
    }

    #[must_use]
    fn is_pressed(&self, key: KeyCode) -> bool {
        usize::try_from(key.raw())
            .ok()
            .and_then(|index| self.pressed.get(index))
            .copied()
            .unwrap_or(false)
    }
}

#[derive(Debug)]
pub struct CaptureState {
    active_id: Option<String>,
    waiting_for_release: bool,
    pending_modifier_key: KeyCode,
    pending_modifier_combo: u32,
    key_was_pressed: [bool; KEY_STATE_COUNT],
    input: InputSnapshot,
    external_capture_active: bool,
    suppress_hotkeys_until_release: bool,
}

impl Default for CaptureState {
    fn default() -> Self {
        Self {
            active_id: None,
            waiting_for_release: false,
            pending_modifier_key: KeyCode::NONE,
            pending_modifier_combo: 0,
            key_was_pressed: [false; KEY_STATE_COUNT],
            input: InputSnapshot::default(),
            external_capture_active: false,
            suppress_hotkeys_until_release: false,
        }
    }
}

impl CaptureState {
    #[must_use]
    pub const fn capture_active(&self) -> bool {
        self.active_id.is_some()
    }

    #[must_use]
    pub const fn runtime_allowed(&self) -> bool {
        !self.capture_active()
            && !self.external_capture_active
            && !self.suppress_hotkeys_until_release
    }

    #[must_use]
    pub const fn input_capture_active(&self) -> bool {
        self.capture_active() || self.external_capture_active
    }

    #[must_use]
    pub const fn input_poll_needed(&self) -> bool {
        self.input_capture_active() || self.suppress_hotkeys_until_release
    }

    pub const fn set_external_capture(&mut self, active: bool) {
        if active || self.external_capture_active != active {
            self.suppress_hotkeys_until_release = true;
        }
        self.external_capture_active = active;
    }

    pub const fn set_input(&mut self, input: InputSnapshot) {
        self.input = input;
        if !self.capture_active()
            && !self.external_capture_active
            && self.suppress_hotkeys_until_release
            && !self.input.any_pressed()
        {
            self.suppress_hotkeys_until_release = false;
        }
    }

    pub fn begin(&mut self, id: &str) {
        self.active_id = Some(id.to_owned());
        self.waiting_for_release = true;
        self.pending_modifier_key = KeyCode::NONE;
        self.pending_modifier_combo = 0;
        self.key_was_pressed.fill(false);
        self.suppress_hotkeys_until_release = true;
    }

    #[must_use]
    fn is_active(&self, id: &str) -> bool {
        self.active_id.as_deref() == Some(id)
    }

    fn finish(&mut self) {
        self.active_id = None;
        self.waiting_for_release = false;
        self.pending_modifier_key = KeyCode::NONE;
        self.pending_modifier_combo = 0;
        self.suppress_hotkeys_until_release = true;
    }

    fn capture_value(&mut self, id: &str, current: Hotkey) -> Hotkey {
        if !self.is_active(id) {
            return current;
        }

        if self.waiting_for_release {
            if self.input.any_pressed() {
                return current;
            }
            self.waiting_for_release = false;
            self.pending_modifier_key = KeyCode::NONE;
            self.pending_modifier_combo = 0;
            self.key_was_pressed.fill(false);
        }

        let modifiers = current_modifier_mask(&self.input);
        if let Some(transient) = self.input.transient.take() {
            let bound = normalize_bound_hotkey(Hotkey::new(transient.raw() | modifiers));
            self.finish();
            return bound;
        }

        for raw in 1..KEY_STATE_COUNT {
            let currently_pressed = self.input.pressed[raw];
            let was_pressed = self.key_was_pressed[raw];
            self.key_was_pressed[raw] = currently_pressed;
            if !currently_pressed || was_pressed {
                continue;
            }
            let Ok(raw) = u32::try_from(raw) else {
                continue;
            };
            let key = KeyCode::new(raw);
            if key.is_modifier() {
                self.pending_modifier_key = key;
                self.pending_modifier_combo = modifiers;
                continue;
            }
            let bound = normalize_bound_hotkey(Hotkey::new(key.raw() | modifiers));
            self.finish();
            return bound;
        }

        if self.pending_modifier_key != KeyCode::NONE
            && !self.input.is_pressed(self.pending_modifier_key)
            && modifiers == 0
        {
            let bound = normalize_bound_hotkey(Hotkey::new(
                self.pending_modifier_key.raw() | self.pending_modifier_combo,
            ));
            self.finish();
            return bound;
        }

        current
    }

    fn display_strings(&self, id: &str, value: Hotkey) -> (String, String, &'static str) {
        if !self.is_active(id) {
            return (format_hotkey_human(value), format_hotkey_hex(value), "Click to Bind Key");
        }
        if self.waiting_for_release {
            return ("Release keys...".to_owned(), "Release keys...".to_owned(), "Release Keys...");
        }
        let modifiers = current_modifier_mask(&self.input);
        if modifiers == 0 {
            ("Waiting...".to_owned(), "Waiting...".to_owned(), "Press a Key...")
        } else {
            (
                format_modifier_preview_human(modifiers),
                format_modifier_preview_hex(modifiers),
                "Press a Key...",
            )
        }
    }
}

pub fn render_control(
    ui: &Ui,
    state: &mut CaptureState,
    id: &str,
    value: &mut u32,
    human_width: f32,
    hex_width: f32,
    wrap_label: bool,
) {
    let current = Hotkey::new(*value);
    let (_, _, button_label) = state.display_strings(id, current);
    let _id = ui.push_id(id);
    if ui.button(button_label) {
        state.begin(id);
    }
    ui.same_line();
    let next = state.capture_value(id, current);
    *value = next.raw();
    let (mut human, mut hex, _) = state.display_strings(id, next);

    ui.set_next_item_width(human_width);
    ui.input_text("##KeyHuman", &mut human).read_only(true).build();
    if !wrap_label {
        ui.same_line();
    }
    ui.text_wrapped("Key Binding");
    ui.same_line();
    ui.set_next_item_width(hex_width);
    ui.input_text("##KeyHex", &mut hex).read_only(true).build();
    ui.same_line();
    ui.text_wrapped("(Hex)");
}

#[must_use]
fn current_modifier_mask(input: &InputSnapshot) -> u32 {
    let mut modifiers = 0;
    if input.is_pressed(KeyCode::LWIN) || input.is_pressed(KeyCode::RWIN) {
        modifiers |= HOTKEY_MASK_WIN;
    }
    if input.is_pressed(KeyCode::CONTROL)
        || input.is_pressed(KeyCode::LCONTROL)
        || input.is_pressed(KeyCode::RCONTROL)
    {
        modifiers |= HOTKEY_MASK_CTRL;
    }
    if input.is_pressed(KeyCode::MENU)
        || input.is_pressed(KeyCode::LMENU)
        || input.is_pressed(KeyCode::RMENU)
    {
        modifiers |= HOTKEY_MASK_ALT;
    }
    if input.is_pressed(KeyCode::SHIFT)
        || input.is_pressed(KeyCode::LSHIFT)
        || input.is_pressed(KeyCode::RSHIFT)
    {
        modifiers |= HOTKEY_MASK_SHIFT;
    }
    modifiers
}

#[must_use]
const fn normalize_bound_hotkey(combined: Hotkey) -> Hotkey {
    let mut key = combined.key();
    let mut modifiers = combined.raw() & !HOTKEY_KEY_MASK;
    match key {
        KeyCode::SHIFT | KeyCode::LSHIFT | KeyCode::RSHIFT => {
            key = KeyCode::SHIFT;
            modifiers &= !HOTKEY_MASK_SHIFT;
        }
        KeyCode::CONTROL | KeyCode::LCONTROL | KeyCode::RCONTROL => {
            key = KeyCode::CONTROL;
            modifiers &= !HOTKEY_MASK_CTRL;
        }
        KeyCode::MENU | KeyCode::LMENU | KeyCode::RMENU => {
            key = KeyCode::MENU;
            modifiers &= !HOTKEY_MASK_ALT;
        }
        KeyCode::LWIN | KeyCode::RWIN => modifiers &= !HOTKEY_MASK_WIN,
        _ => {}
    }
    Hotkey::new((key.raw() & HOTKEY_KEY_MASK) | modifiers)
}

#[must_use]
fn format_hotkey_human(hotkey: Hotkey) -> String {
    let mut output = String::new();
    if hotkey.has_win() {
        output.push_str("Win + ");
    }
    if hotkey.has_ctrl() {
        output.push_str("Ctrl + ");
    }
    if hotkey.has_alt() {
        output.push_str("Alt + ");
    }
    if hotkey.has_shift() {
        output.push_str("Shift + ");
    }
    output.push_str(&hotkey.key().legacy_label());
    output
}

#[must_use]
fn format_hotkey_hex(hotkey: Hotkey) -> String {
    let mut parts = Vec::new();
    if hotkey.has_win() {
        parts.push("0x5B".to_owned());
    }
    if hotkey.has_ctrl() {
        parts.push("0x11".to_owned());
    }
    if hotkey.has_alt() {
        parts.push("0x12".to_owned());
    }
    if hotkey.has_shift() {
        parts.push("0x10".to_owned());
    }
    parts.push(format!("0x{:X}", hotkey.key().raw()));
    parts.join(" + ")
}

fn format_modifier_preview_human(modifiers: u32) -> String {
    let mut output = String::new();
    if modifiers & HOTKEY_MASK_WIN != 0 {
        output.push_str("Win + ");
    }
    if modifiers & HOTKEY_MASK_CTRL != 0 {
        output.push_str("Ctrl + ");
    }
    if modifiers & HOTKEY_MASK_ALT != 0 {
        output.push_str("Alt + ");
    }
    if modifiers & HOTKEY_MASK_SHIFT != 0 {
        output.push_str("Shift + ");
    }
    output.push_str("...");
    output
}

fn format_modifier_preview_hex(modifiers: u32) -> String {
    let mut parts = Vec::new();
    if modifiers & HOTKEY_MASK_WIN != 0 {
        parts.push("0x5B");
    }
    if modifiers & HOTKEY_MASK_CTRL != 0 {
        parts.push("0x11");
    }
    if modifiers & HOTKEY_MASK_ALT != 0 {
        parts.push("0x12");
    }
    if modifiers & HOTKEY_MASK_SHIFT != 0 {
        parts.push("0x10");
    }
    format!("{} + ...", parts.join(" + "))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn snapshot(keys: &[KeyCode], transient: Option<KeyCode>) -> InputSnapshot {
        let mut snapshot = InputSnapshot::default();
        for key in keys {
            snapshot.pressed[usize::try_from(key.raw()).expect("key index")] = true;
        }
        snapshot.transient = transient;
        snapshot
    }

    #[test]
    fn capture_waits_for_initial_release_then_binds_modifier_chord() {
        let mut state = CaptureState::default();
        state.set_input(snapshot(&[KeyCode::LBUTTON], None));
        state.begin("test");
        assert_eq!(state.capture_value("test", Hotkey::new(0x41)), Hotkey::new(0x41));
        assert!(state.waiting_for_release);

        state.set_input(snapshot(&[], None));
        assert_eq!(state.capture_value("test", Hotkey::new(0x41)), Hotkey::new(0x41));
        state.set_input(snapshot(&[KeyCode::LCONTROL], None));
        assert_eq!(state.capture_value("test", Hotkey::new(0x41)), Hotkey::new(0x41));
        state.set_input(snapshot(&[KeyCode::LCONTROL, KeyCode::A], None));
        assert_eq!(state.capture_value("test", Hotkey::new(0x41)).raw(), HOTKEY_MASK_CTRL | 0x41);
        assert!(!state.capture_active());
        assert!(state.suppress_hotkeys_until_release);
    }

    #[test]
    fn modifier_only_binding_commits_when_modifier_is_released() {
        let mut state = CaptureState::default();
        state.begin("test");
        state.set_input(snapshot(&[], None));
        let _ = state.capture_value("test", Hotkey::new(0x41));
        state.set_input(snapshot(&[KeyCode::LSHIFT], None));
        let _ = state.capture_value("test", Hotkey::new(0x41));
        state.set_input(snapshot(&[], None));
        let result = state.capture_value("test", Hotkey::new(0x41));
        assert_eq!(result, Hotkey::new(KeyCode::SHIFT.raw()));
    }

    #[test]
    fn wheel_transient_binds_with_current_modifiers() {
        let mut state = CaptureState::default();
        state.begin("test");
        state.set_input(snapshot(&[], None));
        let _ = state.capture_value("test", Hotkey::new(0x41));
        state.set_input(snapshot(&[KeyCode::LMENU], Some(KeyCode::MOUSE_WHEEL_DOWN)));
        let result = state.capture_value("test", Hotkey::new(0x41));
        assert_eq!(result.raw(), HOTKEY_MASK_ALT | KeyCode::MOUSE_WHEEL_DOWN.raw());
    }

    #[test]
    fn modifier_main_key_normalization_removes_duplicate_modifier_bit() {
        assert_eq!(
            normalize_bound_hotkey(Hotkey::new(HOTKEY_MASK_SHIFT | KeyCode::LSHIFT.raw())),
            Hotkey::new(KeyCode::SHIFT.raw())
        );
        assert_eq!(
            normalize_bound_hotkey(Hotkey::new(HOTKEY_MASK_WIN | KeyCode::RWIN.raw())),
            Hotkey::new(KeyCode::RWIN.raw())
        );
    }

    #[test]
    fn suppression_clears_only_after_capture_finished_and_all_keys_released() {
        let mut state = CaptureState::default();
        state.begin("test");
        state.set_input(snapshot(&[], None));
        let _ = state.capture_value("test", Hotkey::new(0x41));
        state.set_input(snapshot(&[KeyCode::A], None));
        let _ = state.capture_value("test", Hotkey::new(0x41));
        assert!(state.suppress_hotkeys_until_release);
        state.set_input(snapshot(&[KeyCode::A], None));
        assert!(state.suppress_hotkeys_until_release);
        state.set_input(snapshot(&[], None));
        assert!(!state.suppress_hotkeys_until_release);
    }

    #[test]
    fn external_modal_capture_blocks_runtime_then_suppresses_until_release() {
        let mut state = CaptureState::default();
        state.set_external_capture(true);
        assert!(state.input_capture_active());
        assert!(!state.runtime_allowed());
        state.set_external_capture(false);
        assert!(!state.input_capture_active());
        assert!(state.suppress_hotkeys_until_release);
        state.set_input(snapshot(&[KeyCode::A], None));
        assert!(state.suppress_hotkeys_until_release);
        state.set_input(snapshot(&[], None));
        assert!(!state.suppress_hotkeys_until_release);
        assert!(state.runtime_allowed());
    }

    #[test]
    fn display_strings_match_cpp_prefix_order() {
        let hotkey = Hotkey::new(
            HOTKEY_MASK_WIN | HOTKEY_MASK_CTRL | HOTKEY_MASK_ALT | HOTKEY_MASK_SHIFT | 0x41,
        );
        assert_eq!(format_hotkey_human(hotkey), "Win + Ctrl + Alt + Shift + A");
        assert_eq!(format_hotkey_hex(hotkey), "0x5B + 0x11 + 0x12 + 0x10 + 0x41");
    }
}

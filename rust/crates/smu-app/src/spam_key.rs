use crate::multi_instance::MultiInstanceState;
use crate::theme::ThemeDefinition;
use dear_imgui_rs::{StyleColor, Ui};
use serde_json::{Map, Value, json};
use smu_core::{Hotkey, KeyCode};
use smu_profile::payload::PreservedProfilePayload;
use std::ffi::{CStr, CString};

const ARMED_TEXT: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const ARMED_BUTTON: [f32; 4] = [1.0, 1.0, 0.0, 1.0];

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, PartialEq)]
pub struct SpamKeyUiInstance {
    pub enabled: bool,
    pub trigger: Hotkey,
    pub output: Hotkey,
    pub disable_outside_target: bool,
    pub hold_mode: bool,
    spam_delay: f32,
    real_delay_ms: i32,
    delay_text: String,
}

impl Default for SpamKeyUiInstance {
    fn default() -> Self {
        Self {
            enabled: false,
            trigger: Hotkey::new(KeyCode::new(0xDB).raw()),
            output: Hotkey::new(KeyCode::LBUTTON.raw()),
            disable_outside_target: false,
            hold_mode: false,
            spam_delay: 20.0,
            // Intentional legacy split: constructor displays 20 but leaves real_delay at 1000 until
            // the delay text is edited.
            real_delay_ms: 1_000,
            delay_text: "20".to_owned(),
        }
    }
}

#[derive(Debug, Default)]
pub struct SpamKeyUiState {
    instances: MultiInstanceState<SpamKeyUiInstance>,
}

impl SpamKeyUiState {
    #[must_use]
    pub fn instances(&self) -> &[SpamKeyUiInstance] {
        self.instances.instances()
    }

    #[must_use]
    pub const fn selected_index(&self) -> usize {
        self.instances.selected_index()
    }

    pub const fn select(&mut self, index: usize) {
        self.instances.select(index);
    }

    pub fn add_default(&mut self) {
        self.instances.add_default();
    }

    pub fn tick_remove_confirmation(&mut self, delta_seconds: f32, section_selected: bool) {
        self.instances.tick_remove_confirmation(delta_seconds, section_selected);
    }

    pub(crate) fn set_base_section_flags(&mut self, enabled: bool, disable_outside: bool) {
        if let Some(instance) = self.instances.instances_mut().first_mut() {
            instance.enabled = enabled;
            instance.disable_outside_target = disable_outside;
        }
    }

    #[must_use]
    pub fn first_enabled(&self) -> bool {
        self.instances().first().is_some_and(|instance| instance.enabled)
    }

    #[must_use]
    pub fn enabled_flags(&self) -> Vec<bool> {
        self.instances().iter().map(|instance| instance.enabled).collect()
    }

    pub(crate) fn reset_base_to_profile_defaults(&mut self, defaults: &PreservedProfilePayload) {
        let Some(base) = self.instances.instances_mut().first_mut() else {
            return;
        };
        let enabled = base.enabled;
        let disable_outside_target = base.disable_outside_target;
        let mut default_state = Self::default();
        default_state.apply_profile(defaults);
        let Some(default_base) = default_state.instances().first().cloned() else {
            return;
        };
        *base = default_base;
        base.enabled = enabled;
        base.disable_outside_target = disable_outside_target;
    }

    pub(crate) fn apply_profile(&mut self, payload: &PreservedProfilePayload) {
        // C++ clears the instance deque but preserves flat globals before rebuilding instances[0].
        // The current base instance is the Rust owner of those flat-global values, so omitted profile
        // fields must retain it while extra instances are rebuilt from the selected snapshot.
        let mut base = self.instances().first().cloned().unwrap_or_default();
        if let Some(value) = crate::profile_adapter::u32_value(payload, "vk_leftbracket") {
            base.trigger = Hotkey::new(value);
        }
        if let Some(value) = crate::profile_adapter::u32_value(payload, "vk_spamkey") {
            base.output = Hotkey::new(value);
        }
        if let Some(value) = crate::profile_adapter::bool_value(payload, "isspamswitch") {
            base.hold_mode = value;
        }
        if let Some(value) = crate::profile_adapter::f32_value(payload, "spam_delay") {
            base.spam_delay = value;
        }
        if let Some(value) = crate::profile_adapter::i32_value(payload, "real_delay") {
            base.real_delay_ms = value;
        }
        if let Some(value) = crate::profile_adapter::string_value(payload, "SpamDelay") {
            value.clone_into(&mut base.delay_text);
        }
        if let Some(value) = crate::profile_adapter::section_flag(payload, "section_toggles", 11) {
            base.enabled = value;
        }
        if let Some(value) =
            crate::profile_adapter::section_flag(payload, "disable_outside_roblox", 11)
        {
            base.disable_outside_target = value;
        }

        let mut instances = vec![base];
        if let Some(extras) = payload.raw_value("extra_spamkey_instances").and_then(Value::as_array)
        {
            for extra in extras {
                let Some(object) = extra.as_object() else {
                    continue;
                };
                let mut instance = SpamKeyUiInstance::default();
                if let Some(value) = crate::profile_adapter::object_u32(object, "vk_trigger") {
                    instance.trigger = Hotkey::new(value);
                }
                if let Some(value) = crate::profile_adapter::object_u32(object, "vk_spamkey") {
                    instance.output = Hotkey::new(value);
                }
                if let Some(value) = crate::profile_adapter::object_f32(object, "spam_delay") {
                    instance.spam_delay = value;
                }
                if let Some(value) = crate::profile_adapter::object_i32(object, "real_delay") {
                    instance.real_delay_ms = value;
                }
                if let Some(value) = crate::profile_adapter::object_string(object, "SpamDelay") {
                    value.clone_into(&mut instance.delay_text);
                }
                if let Some(value) = crate::profile_adapter::object_bool(object, "isspamswitch") {
                    instance.hold_mode = value;
                }
                instance.disable_outside_target =
                    crate::profile_adapter::object_bool(object, "disable_outside_roblox")
                        .unwrap_or(instances[0].disable_outside_target);
                instance.enabled =
                    crate::profile_adapter::object_bool(object, "section_enabled").unwrap_or(false);
                instances.push(instance);
            }
        }
        self.instances.replace_instances(instances, 0);
    }

    pub(crate) fn overlay_profile(&self, overlay: &mut Map<String, Value>) {
        let Some(base) = self.instances().first() else {
            return;
        };
        crate::profile_adapter::insert_u32(overlay, "vk_leftbracket", base.trigger.raw());
        crate::profile_adapter::insert_u32(overlay, "vk_spamkey", base.output.raw());
        crate::profile_adapter::insert_bool(overlay, "isspamswitch", base.hold_mode);
        crate::profile_adapter::insert_f32(overlay, "spam_delay", base.spam_delay);
        crate::profile_adapter::insert_i32(overlay, "real_delay", base.real_delay_ms);
        crate::profile_adapter::insert_string(overlay, "SpamDelay", base.delay_text.clone());
        let extras: Vec<Value> = self
            .instances()
            .iter()
            .skip(1)
            .map(|instance| {
                json!({
                    "vk_trigger": instance.trigger.raw(),
                    "vk_spamkey": instance.output.raw(),
                    "spam_delay": instance.spam_delay,
                    "real_delay": instance.real_delay_ms,
                    "SpamDelay": instance.delay_text,
                    "isspamswitch": instance.hold_mode,
                    "disable_outside_roblox": instance.disable_outside_target,
                    "section_enabled": instance.enabled
                })
            })
            .collect();
        if !extras.is_empty() {
            overlay.insert("extra_spamkey_instances".to_owned(), Value::Array(extras));
        }
    }

    #[must_use]
    pub fn controller_settings(
        &self,
        macro_enabled: bool,
        process_selector: String,
    ) -> crate::spam_key_runtime::SpamKeyControllerSettings {
        crate::spam_key_runtime::SpamKeyControllerSettings {
            macro_enabled,
            process_selector,
            instances: self
                .instances()
                .iter()
                .map(|instance| crate::spam_key_runtime::SpamKeyInstanceSettings {
                    section_enabled: instance.enabled,
                    trigger: instance.trigger,
                    output: instance.output,
                    disable_outside_target: instance.disable_outside_target,
                    hold_mode: instance.hold_mode,
                    real_delay_ms: instance.real_delay_ms,
                })
                .collect(),
        }
    }
}

pub fn render(
    ui: &Ui,
    state: &mut SpamKeyUiState,
    capture: &mut crate::keybind::CaptureState,
    theme: &ThemeDefinition,
) {
    let count = state.instances.instances().len();
    let selected = state.instances.selected_index();
    let sid = format!("_sk{selected}");
    render_header(ui, state, count, selected, capture);
    {
        let instance = state.instances.selected_mut();
        render_enablement(ui, instance, &sid, theme);
    }
    render_instance_controls(ui, state, count, selected, &sid);
    let instance = state.instances.selected_mut();
    render_settings(ui, instance, &sid, capture);

    crate::parity::separator(ui);
    ui.text_wrapped("Explanation:");
    ui.new_line();
    ui.text_wrapped("This macro allows you to spam a key very fast while the macro is active. You can use it for gear clipping, autoclicking, or general key-spam.");
    ui.text_wrapped("For gear clipping, set up near the wall, trigger the macro, and hold W. This can be RNG, but lower FPS often helps.");
}

fn render_header(
    ui: &Ui,
    state: &mut SpamKeyUiState,
    count: usize,
    selected: usize,
    capture: &mut crate::keybind::CaptureState,
) {
    if count > 1 {
        ui.text_wrapped(format!("Settings for Spam a Key {}", selected + 1));
    } else {
        ui.text_wrapped("Settings for Spam a Key");
    }
    crate::parity::separator(ui);
    ui.new_line();
    ui.text_wrapped("Keybind:");
    ui.same_line();
    crate::keybind::render_hotkey_control(
        ui,
        capture,
        &format!("SectionKey11_sk{selected}"),
        &mut state.instances.selected_mut().trigger,
        170.0,
        130.0,
        false,
    );
}

fn render_enablement(
    ui: &Ui,
    instance: &mut SpamKeyUiInstance,
    sid: &str,
    theme: &ThemeDefinition,
) {
    let color = ui.push_style_color(
        StyleColor::Text,
        if instance.enabled { theme.success_color } else { theme.error_color },
    );
    ui.text_wrapped("Enable This Macro:");
    drop(color);
    ui.same_line();
    ui.checkbox(format!("##SpamKeyEnabled{sid}"), &mut instance.enabled);
    ui.same_line();
    crate::foreground_ui::render_checkbox(
        ui,
        "Disable outside of Target Application:",
        &format!("##SpamKeyOutside{sid}"),
        &mut instance.disable_outside_target,
    );
}

fn render_instance_controls(
    ui: &Ui,
    state: &mut SpamKeyUiState,
    count: usize,
    selected: usize,
    sid: &str,
) {
    if count > 1 {
        ui.text_wrapped(format!("Spam a Key {} of {count}", selected + 1));
        ui.same_line();
    }
    if ui.button(format!("+ Add New##AddSK{sid}")) {
        state.instances.add_default();
    }
    render_remove_button(ui, state, sid);
    crate::parity::separator(ui);
}

fn render_settings(
    ui: &Ui,
    instance: &mut SpamKeyUiInstance,
    sid: &str,
    capture: &mut crate::keybind::CaptureState,
) {
    ui.text_wrapped("Key to Press:");
    ui.same_line();
    crate::keybind::render_hotkey_control(
        ui,
        capture,
        &format!("SpamKey{sid}"),
        &mut instance.output,
        170.0,
        130.0,
        true,
    );
    ui.text_wrapped("Spam Delay (Milliseconds):");
    ui.same_line();
    ui.set_next_item_width(120.0);
    if ui
        .input_text(format!("##SpamDelay{sid}"), &mut instance.delay_text)
        .chars_decimal(true)
        .chars_no_blank(true)
        .build()
        && let Some(delay) = cpp_stof_prefix(&instance.delay_text)
    {
        instance.spam_delay = delay;
        instance.real_delay_ms = spam_real_delay(delay);
    }
    ui.text_wrapped("I do not take any responsibility if you set the delay to 0ms");
    ui.checkbox(format!("Switch from Toggle Key to Hold Key##{sid}"), &mut instance.hold_mode);
}

fn render_remove_button(ui: &Ui, state: &mut SpamKeyUiState, sid: &str) {
    let Some(_target) = state.instances.removal_target() else {
        return;
    };
    ui.same_line();
    let armed = state.instances.removal_is_armed();
    let _text = armed.then(|| ui.push_style_color(StyleColor::Text, ARMED_TEXT));
    let _button = armed.then(|| ui.push_style_color(StyleColor::Button, ARMED_BUTTON));
    let _hovered = armed.then(|| ui.push_style_color(StyleColor::ButtonHovered, ARMED_BUTTON));
    let _active = armed.then(|| ui.push_style_color(StyleColor::ButtonActive, ARMED_BUTTON));
    let label = if state.instances.selected_index() == 0 {
        "- Remove Last Instance"
    } else {
        "- Remove Current Instance"
    };
    if ui.button(format!("{label}##RemSK{sid}")) {
        let _ = state.instances.click_remove();
    }
}

#[allow(clippy::cast_possible_truncation, clippy::manual_midpoint)] // Exact C++ `(delay + .5f) / 2` then int cast.
fn spam_real_delay(delay: f32) -> i32 {
    ((delay + 0.5) / 2.0) as i32
}

fn cpp_stof_prefix(value: &str) -> Option<f32> {
    let value = CString::new(value).ok()?;
    let mut end = std::ptr::null_mut();
    // SAFETY: `value` is NUL-terminated and `end` is a valid out-pointer for `strtof`.
    let parsed = unsafe { libc::strtof(value.as_ptr(), &raw mut end) };
    if end == value.as_ptr().cast_mut() {
        return None;
    }
    // Match std::stof's acceptance of a numeric prefix. Reading the tail only verifies that `end`
    // points inside the same CString allocation.
    // SAFETY: `strtof` returns `end` pointing into the input string or at its terminating NUL.
    let _tail = unsafe { CStr::from_ptr(end) };
    Some(parsed)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{SpamKeyUiInstance, SpamKeyUiState, cpp_stof_prefix, spam_real_delay};
    use smu_core::KeyCode;

    #[test]
    fn defaults_preserve_cpp_display_runtime_delay_split() {
        let instance = SpamKeyUiInstance::default();
        assert!(!instance.enabled);
        assert_eq!(instance.trigger.key(), KeyCode::new(0xDB));
        assert_eq!(instance.output.key(), KeyCode::LBUTTON);
        assert!(!instance.disable_outside_target);
        assert!(!instance.hold_mode);
        assert!((instance.spam_delay - 20.0).abs() < f32::EPSILON);
        assert_eq!(instance.delay_text, "20");
        assert_eq!(instance.real_delay_ms, 1_000);
    }

    #[test]
    fn delay_edit_formula_matches_cpp_cast() {
        let mut state = SpamKeyUiState::default();
        let instance = state.instances.selected_mut();
        let delay = cpp_stof_prefix("20junk").expect("numeric prefix should parse");
        instance.spam_delay = delay;
        instance.real_delay_ms = spam_real_delay(delay);
        assert_eq!(instance.real_delay_ms, 10);
    }
}

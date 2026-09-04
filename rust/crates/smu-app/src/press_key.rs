use crate::multi_instance::MultiInstanceState;
use crate::theme::ThemeDefinition;
use dear_imgui_rs::{StyleColor, Ui};
use serde_json::{Map, Value, json};
use smu_core::{Hotkey, KeyCode};
use smu_profile::payload::PreservedProfilePayload;

const ARMED_TEXT: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const ARMED_BUTTON: [f32; 4] = [1.0, 1.0, 0.0, 1.0];

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PressKeyUiInstance {
    pub enabled: bool,
    pub trigger: Hotkey,
    pub output: Hotkey,
    pub disable_outside_target: bool,
    press_delay_ms: i32,
    bonus_delay_ms: i32,
    press_delay_text: String,
    bonus_delay_text: String,
}

impl Default for PressKeyUiInstance {
    fn default() -> Self {
        Self {
            enabled: false,
            trigger: Hotkey::new(KeyCode::Z.raw()),
            output: Hotkey::new(KeyCode::new(0x44).raw()),
            disable_outside_target: false,
            press_delay_ms: 16,
            bonus_delay_ms: 0,
            press_delay_text: "16".to_owned(),
            bonus_delay_text: "0".to_owned(),
        }
    }
}

#[derive(Debug, Default)]
pub struct PressKeyUiState {
    instances: MultiInstanceState<PressKeyUiInstance>,
}

impl PressKeyUiState {
    #[must_use]
    pub fn instances(&self) -> &[PressKeyUiInstance] {
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
        let mut base = PressKeyUiInstance::default();
        if let Some(value) = crate::profile_adapter::u32_value(payload, "vk_zkey") {
            base.trigger = Hotkey::new(value);
        }
        if let Some(value) = crate::profile_adapter::u32_value(payload, "vk_dkey") {
            base.output = Hotkey::new(value);
        }
        if let Some(value) = crate::profile_adapter::string_value(payload, "PressKeyDelayChar") {
            value.clone_into(&mut base.press_delay_text);
            if let Ok(parsed) = value.parse::<i32>() {
                base.press_delay_ms = parsed;
            }
        }
        if let Some(value) = crate::profile_adapter::string_value(payload, "PressKeyBonusDelayChar")
        {
            value.clone_into(&mut base.bonus_delay_text);
            if let Ok(parsed) = value.parse::<i32>() {
                base.bonus_delay_ms = parsed;
            }
        }
        if let Some(value) = crate::profile_adapter::section_flag(payload, "section_toggles", 5) {
            base.enabled = value;
        }
        if let Some(value) =
            crate::profile_adapter::section_flag(payload, "disable_outside_roblox", 5)
        {
            base.disable_outside_target = value;
        }

        let mut instances = vec![base];
        if let Some(extras) =
            payload.raw_value("extra_presskey_instances").and_then(Value::as_array)
        {
            for extra in extras {
                let Some(object) = extra.as_object() else {
                    continue;
                };
                let mut instance = PressKeyUiInstance::default();
                if let Some(value) = crate::profile_adapter::object_u32(object, "vk_trigger") {
                    instance.trigger = Hotkey::new(value);
                }
                if let Some(value) = crate::profile_adapter::object_u32(object, "vk_presskey") {
                    instance.output = Hotkey::new(value);
                }
                if let Some(value) = crate::profile_adapter::object_i32(object, "PressKeyDelay") {
                    instance.press_delay_ms = value;
                }
                if let Some(value) =
                    crate::profile_adapter::object_i32(object, "PressKeyBonusDelay")
                {
                    instance.bonus_delay_ms = value;
                }
                if let Some(value) =
                    crate::profile_adapter::object_string(object, "PressKeyDelayChar")
                {
                    value.clone_into(&mut instance.press_delay_text);
                }
                if let Some(value) =
                    crate::profile_adapter::object_string(object, "PressKeyBonusDelayChar")
                {
                    value.clone_into(&mut instance.bonus_delay_text);
                }
                instance.disable_outside_target =
                    crate::profile_adapter::object_bool(object, "disable_outside_roblox")
                        .or_else(|| crate::profile_adapter::object_bool(object, "presskeyinroblox"))
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
        crate::profile_adapter::insert_u32(overlay, "vk_zkey", base.trigger.raw());
        crate::profile_adapter::insert_u32(overlay, "vk_dkey", base.output.raw());
        crate::profile_adapter::insert_string(
            overlay,
            "PressKeyDelayChar",
            base.press_delay_text.clone(),
        );
        crate::profile_adapter::insert_string(
            overlay,
            "PressKeyBonusDelayChar",
            base.bonus_delay_text.clone(),
        );
        crate::profile_adapter::insert_bool(
            overlay,
            "presskeyinroblox",
            base.disable_outside_target,
        );
        let extras: Vec<Value> = self
            .instances()
            .iter()
            .skip(1)
            .map(|instance| {
                json!({
                    "vk_trigger": instance.trigger.raw(),
                    "vk_presskey": instance.output.raw(),
                    "PressKeyDelay": instance.press_delay_ms,
                    "PressKeyBonusDelay": instance.bonus_delay_ms,
                    "PressKeyDelayChar": instance.press_delay_text,
                    "PressKeyBonusDelayChar": instance.bonus_delay_text,
                    "disable_outside_roblox": instance.disable_outside_target,
                    "presskeyinroblox": instance.disable_outside_target,
                    "section_enabled": instance.enabled
                })
            })
            .collect();
        if !extras.is_empty() {
            overlay.insert("extra_presskey_instances".to_owned(), Value::Array(extras));
        }
    }

    #[must_use]
    pub fn controller_settings(
        &self,
        macro_enabled: bool,
        process_selector: String,
    ) -> crate::press_key_runtime::PressKeyControllerSettings {
        crate::press_key_runtime::PressKeyControllerSettings {
            macro_enabled,
            process_selector,
            instances: self
                .instances()
                .iter()
                .map(|instance| crate::press_key_runtime::PressKeyInstanceSettings {
                    section_enabled: instance.enabled,
                    trigger: instance.trigger,
                    output: instance.output,
                    disable_outside_target: instance.disable_outside_target,
                    press_delay_ms: instance.press_delay_ms,
                    bonus_delay_ms: instance.bonus_delay_ms,
                })
                .collect(),
        }
    }
}

pub fn render(
    ui: &Ui,
    state: &mut PressKeyUiState,
    capture: &mut crate::keybind::CaptureState,
    theme: &ThemeDefinition,
) {
    let count = state.instances.instances().len();
    let selected = state.instances.selected_index();
    let sid = format!("_pk{selected}");
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
    ui.text_wrapped("This macro allows you to press another key for a short moment whenever you trigger the macro. It is useful for micro-adjustments while moving, especially while jumping.");
}

fn render_header(
    ui: &Ui,
    state: &mut PressKeyUiState,
    count: usize,
    selected: usize,
    capture: &mut crate::keybind::CaptureState,
) {
    if count > 1 {
        ui.text_wrapped(format!("Settings for Press a Button {}", selected + 1));
    } else {
        ui.text_wrapped("Settings for Press a Button");
    }
    crate::parity::separator(ui);
    ui.new_line();
    ui.text_wrapped("Keybind:");
    ui.same_line();
    crate::keybind::render_hotkey_control(
        ui,
        capture,
        &format!("SectionKey5_pk{selected}"),
        &mut state.instances.selected_mut().trigger,
        170.0,
        130.0,
        false,
    );
}

fn render_instance_controls(
    ui: &Ui,
    state: &mut PressKeyUiState,
    count: usize,
    selected: usize,
    sid: &str,
) {
    if count > 1 {
        ui.text_wrapped(format!("Press a Button {} of {count}", selected + 1));
        ui.same_line();
    }
    if ui.button(format!("+ Add New##AddPK{sid}")) {
        state.instances.add_default();
    }
    render_remove_button(ui, state, sid);
    crate::parity::separator(ui);
}

fn render_enablement(
    ui: &Ui,
    instance: &mut PressKeyUiInstance,
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
    ui.checkbox(format!("##PressKeyEnabled{sid}"), &mut instance.enabled);
    ui.same_line();
    ui.text_wrapped("Disable outside of Target Application:");
    ui.same_line();
    ui.checkbox(format!("##PressKeyOutside{sid}"), &mut instance.disable_outside_target);
}

fn render_settings(
    ui: &Ui,
    instance: &mut PressKeyUiInstance,
    sid: &str,
    capture: &mut crate::keybind::CaptureState,
) {
    ui.text_wrapped("Key to Press:");
    ui.same_line();
    crate::keybind::render_hotkey_control(
        ui,
        capture,
        &format!("PressKey{sid}"),
        &mut instance.output,
        170.0,
        130.0,
        true,
    );
    ui.text("Length of Second Button Press (ms):");
    ui.same_line();
    ui.set_next_item_width(80.0);
    if ui
        .input_text(format!("##PressKeyDelayChar{sid}"), &mut instance.press_delay_text)
        .chars_decimal(true)
        .chars_no_blank(true)
        .build()
        && let Ok(value) = instance.press_delay_text.parse::<i32>()
    {
        instance.press_delay_ms = value;
    }
    ui.text("Delay Before Second Press (ms):");
    ui.same_line();
    ui.set_next_item_width(80.0);
    if ui
        .input_text(format!("##PressKeyBonusDelayChar{sid}"), &mut instance.bonus_delay_text)
        .chars_decimal(true)
        .chars_no_blank(true)
        .build()
        && let Ok(value) = instance.bonus_delay_text.parse::<i32>()
    {
        instance.bonus_delay_ms = value;
    }
}

fn render_remove_button(ui: &Ui, state: &mut PressKeyUiState, sid: &str) {
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
    if ui.button(format!("{label}##RemPK{sid}")) {
        let _ = state.instances.click_remove();
    }
}

#[cfg(test)]
mod tests {
    use super::{PressKeyUiInstance, PressKeyUiState};
    use smu_core::KeyCode;

    #[test]
    fn defaults_match_cpp_presskey_instance() {
        let instance = PressKeyUiInstance::default();
        assert!(!instance.enabled);
        assert_eq!(instance.trigger.key(), KeyCode::Z);
        assert_eq!(instance.output.key(), KeyCode::new(0x44));
        assert!(!instance.disable_outside_target);
        assert_eq!(instance.press_delay_ms, 16);
        assert_eq!(instance.bonus_delay_ms, 0);
    }

    #[test]
    fn multi_instance_defaults_append_without_copying_selected_values() {
        let mut state = PressKeyUiState::default();
        state.add_default();
        assert_eq!(state.instances().len(), 2);
        assert_eq!(state.instances()[1], PressKeyUiInstance::default());
    }
}

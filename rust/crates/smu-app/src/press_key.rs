use crate::multi_instance::MultiInstanceState;
use dear_imgui_rs::{StyleColor, Ui};
use smu_core::{Hotkey, KeyCode};

const SUCCESS: [f32; 4] = [0.0, 1.0, 0.50, 1.0];
const ERROR: [f32; 4] = [1.0, 0.20, 0.40, 1.0];
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

    #[must_use]
    pub fn first_enabled(&self) -> bool {
        self.instances().first().is_some_and(|instance| instance.enabled)
    }

    #[must_use]
    pub fn enabled_flags(&self) -> Vec<bool> {
        self.instances().iter().map(|instance| instance.enabled).collect()
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

pub fn render(ui: &Ui, state: &mut PressKeyUiState) {
    let count = state.instances.instances().len();
    let selected = state.instances.selected_index();
    let sid = format!("_pk{selected}");
    render_header(ui, state, count, selected);
    {
        let instance = state.instances.selected_mut();
        render_enablement(ui, instance, &sid);
    }
    render_instance_controls(ui, state, count, selected, &sid);
    let instance = state.instances.selected_mut();
    render_settings(ui, instance, &sid);

    crate::parity::separator(ui);
    ui.text_wrapped("Explanation:");
    ui.new_line();
    ui.text_wrapped("This macro allows you to press another key for a short moment whenever you trigger the macro. It is useful for micro-adjustments while moving, especially while jumping.");
}

fn render_header(ui: &Ui, state: &mut PressKeyUiState, count: usize, selected: usize) {
    if count > 1 {
        ui.text_wrapped(format!("Settings for Press a Button {}", selected + 1));
    } else {
        ui.text_wrapped("Settings for Press a Button");
    }
    crate::parity::separator(ui);
    ui.new_line();
    ui.text_wrapped("Keybind:");
    ui.same_line();
    ui.text(state.instances.selected_mut().trigger.key().legacy_label());
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

fn render_enablement(ui: &Ui, instance: &mut PressKeyUiInstance, sid: &str) {
    let color =
        ui.push_style_color(StyleColor::Text, if instance.enabled { SUCCESS } else { ERROR });
    ui.text_wrapped("Enable This Macro:");
    drop(color);
    ui.same_line();
    ui.checkbox(format!("##PressKeyEnabled{sid}"), &mut instance.enabled);
    ui.same_line();
    ui.text_wrapped("Disable outside of Target Application:");
    ui.same_line();
    ui.checkbox(format!("##PressKeyOutside{sid}"), &mut instance.disable_outside_target);
}

fn render_settings(ui: &Ui, instance: &mut PressKeyUiInstance, sid: &str) {
    ui.text_wrapped("Key to Press:");
    ui.same_line();
    ui.text(instance.output.key().legacy_label());
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

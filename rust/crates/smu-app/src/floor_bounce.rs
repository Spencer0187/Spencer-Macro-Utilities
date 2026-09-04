use dear_imgui_rs::{StyleColor, Ui};
use smu_core::Hotkey;

const SUCCESS: [f32; 4] = [0.0, 1.0, 0.50, 1.0];
const ERROR: [f32; 4] = [1.0, 0.20, 0.40, 1.0];

#[derive(Debug)]
pub struct FloorBounceUiState {
    pub enabled: bool,
    pub hotkey: Hotkey,
    pub disable_outside_target: bool,
    pub hhj_enabled: bool,
    delay1_ms: i32,
    delay2_ms: i32,
    delay3_ms: i32,
    delay1_text: String,
    delay2_text: String,
    delay3_text: String,
}

impl Default for FloorBounceUiState {
    fn default() -> Self {
        Self {
            enabled: false,
            hotkey: crate::floor_bounce_runtime::DEFAULT_FLOOR_BOUNCE_HOTKEY,
            disable_outside_target: true,
            hhj_enabled: false,
            delay1_ms: 5,
            delay2_ms: 8,
            delay3_ms: 100,
            delay1_text: "5".to_owned(),
            delay2_text: "8".to_owned(),
            delay3_text: "100".to_owned(),
        }
    }
}

impl FloorBounceUiState {
    #[must_use]
    pub const fn controller_settings(
        &self,
        macro_enabled: bool,
        process_selector: String,
        take_all_process_ids: bool,
        shift_key: u32,
    ) -> crate::floor_bounce_runtime::FloorBounceControllerSettings {
        crate::floor_bounce_runtime::FloorBounceControllerSettings {
            macro_enabled,
            section_enabled: self.enabled,
            hotkey: self.hotkey,
            disable_outside_target: self.disable_outside_target,
            process_selector,
            take_all_process_ids,
            hhj_enabled: self.hhj_enabled,
            delay1_ms: self.delay1_ms,
            shift_key,
            delay2_ms: self.delay2_ms,
            delay3_ms: self.delay3_ms,
        }
    }
}

pub fn render(ui: &Ui, state: &mut FloorBounceUiState) {
    ui.text_wrapped("Settings for Floor Bounce");
    crate::parity::separator(ui);
    ui.new_line();
    ui.text_wrapped("Keybind:");
    ui.same_line();
    ui.text(state.hotkey.key().legacy_label());

    let color = ui.push_style_color(StyleColor::Text, if state.enabled { SUCCESS } else { ERROR });
    ui.text_wrapped("Enable This Macro:");
    drop(color);
    ui.same_line();
    ui.checkbox("##SectionToggle14", &mut state.enabled);
    ui.same_line();
    ui.text_wrapped("Disable outside of Target Application:");
    ui.same_line();
    ui.checkbox("##DisableOutsideRoblox_14", &mut state.disable_outside_target);

    ui.checkbox(
        "Attempt to HHJ for potentially slightly more height (EXPERIMENTAL, NOT RECOMMENDED)",
        &mut state.hhj_enabled,
    );
    crate::parity::separator(ui);
    ui.text_wrapped("Advanced Floor Bounce HHJ Options");
    render_delay(
        ui,
        "R##FloorBounceDelay1",
        "Delay (Milliseconds) after unfreezing and before shiftlocking",
        &mut state.delay1_ms,
        &mut state.delay1_text,
        5,
    );
    render_delay(
        ui,
        "R##FloorBounceDelay2",
        "Delay (Milliseconds) before enabling helicoptering",
        &mut state.delay2_ms,
        &mut state.delay2_text,
        8,
    );
    render_delay(
        ui,
        "R##FloorBounceDelay3",
        "Time (Milliseconds) spent helicoptering",
        &mut state.delay3_ms,
        &mut state.delay3_text,
        100,
    );
    crate::parity::separator(ui);
    ui.text_wrapped("IMPORTANT:");
    ui.text_wrapped("This module only works at default Roblox gravity. Only works in R6 for now. FPS must be set to 160 or more to function properly. Higher is better.");
    crate::parity::separator(ui);
    ui.text_wrapped("Explanation:");
    ui.new_line();
    ui.text_wrapped("This macro allows you to jump much higher from flat ground. Have 160+ FPS, R6, and default Roblox gravity; higher FPS is better.");
    ui.text_wrapped("After this, trigger the macro on flat ground. Results can vary, and the HHJ option may add a little more height but is experimental.");
}

fn render_delay(
    ui: &Ui,
    reset_id: &str,
    label: &str,
    value: &mut i32,
    text: &mut String,
    default_value: i32,
) {
    ui.indent();
    if ui.button_with_size(reset_id, [25.0, 0.0]) {
        *value = default_value;
        *text = default_value.to_string();
    }
    ui.same_line();
    ui.text_wrapped(label);
    ui.same_line();
    ui.set_next_item_width(50.0);
    if ui.input_text(format!("##{reset_id}"), text).chars_decimal(true).chars_no_blank(true).build()
        && let Ok(parsed) = text.parse::<i32>()
    {
        *value = parsed;
    }
    ui.unindent();
}

#[cfg(test)]
mod tests {
    use super::FloorBounceUiState;
    use smu_core::KeyCode;

    #[test]
    fn defaults_match_cpp_floor_bounce_ui() {
        let state = FloorBounceUiState::default();
        assert!(!state.enabled);
        assert_eq!(state.hotkey.key(), KeyCode::new(0x73));
        assert!(state.disable_outside_target);
        assert!(!state.hhj_enabled);
        assert_eq!((state.delay1_ms, state.delay2_ms, state.delay3_ms), (5, 8, 100));
    }
}

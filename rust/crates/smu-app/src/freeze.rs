use dear_imgui_rs::{StyleColor, Ui};
use smu_core::Hotkey;

const SUCCESS: [f32; 4] = [0.0, 1.0, 0.50, 1.0];
const ERROR: [f32; 4] = [1.0, 0.20, 0.40, 1.0];
const INPUT_WIDTH: f32 = 60.0;
const SLIDER_WIDTH: f32 = 300.0;
const OVERRIDE_WIDTH: f32 = 50.0;

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
pub struct FreezeUiState {
    pub enabled: bool,
    pub hotkey: Hotkey,
    pub disable_outside_target: bool,
    pub max_seconds: f32,
    pub override_ms_text: String,
    pub toggle_mode: bool,
    pub take_all_process_ids: bool,
}

impl Default for FreezeUiState {
    fn default() -> Self {
        Self {
            enabled: true,
            hotkey: crate::freeze_runtime::DEFAULT_FREEZE_HOTKEY,
            disable_outside_target: true,
            max_seconds: 9.0,
            override_ms_text: "50".to_owned(),
            toggle_mode: false,
            take_all_process_ids: false,
        }
    }
}

impl FreezeUiState {
    #[must_use]
    pub fn controller_settings(
        &self,
        macro_enabled: bool,
        process_selector: String,
    ) -> crate::freeze_runtime::FreezeControllerSettings {
        let override_ms = self.override_ms_text.parse::<i32>().unwrap_or(0);
        crate::freeze_runtime::FreezeControllerSettings {
            macro_enabled,
            section_enabled: self.enabled,
            hotkey: self.hotkey,
            configured_toggle: self.toggle_mode,
            max_seconds: self.max_seconds,
            override_ms,
            take_all_process_ids: self.take_all_process_ids,
            disable_outside_target: self.disable_outside_target
                && !self.toggle_mode
                && !self.take_all_process_ids,
            process_selector,
        }
    }
}

pub fn render(ui: &Ui, state: &mut FreezeUiState) {
    ui.text_wrapped("Settings for Freeze");
    crate::parity::separator(ui);
    ui.new_line();

    ui.text_wrapped("Keybind:");
    ui.same_line();
    ui.text(state.hotkey.key().legacy_label());

    let color = ui.push_style_color(StyleColor::Text, if state.enabled { SUCCESS } else { ERROR });
    ui.text_wrapped("Enable This Macro:");
    drop(color);
    ui.same_line();
    ui.checkbox("##SectionToggle0", &mut state.enabled);
    ui.same_line();
    ui.text_wrapped("Disable outside of Target Application:");
    ui.same_line();
    ui.checkbox("##DisableOutsideRoblox_0", &mut state.disable_outside_target);

    ui.text_wrapped("Automatically Unfreeze after this amount of seconds (Anti-Internet-Kick)");
    ui.set_next_item_width(INPUT_WIDTH);
    if let Ok(input) =
        ui.input_float_config("##FreezeFloat").step(0.0).step_fast(0.0).try_display_format("%.2f")
    {
        input.build(&mut state.max_seconds);
    }
    ui.same_line();
    ui.set_next_item_width(SLIDER_WIDTH);
    if let Ok(slider) =
        ui.slider_config("##FreezeSlider", 0.0_f32, 9.8_f32).try_display_format("%.2f Seconds")
    {
        slider.build(&mut state.max_seconds);
    }

    ui.set_next_item_width(OVERRIDE_WIDTH);
    ui.input_text("Modify 50ms Default Unfreeze Time (MS)", &mut state.override_ms_text)
        .chars_decimal(true)
        .chars_no_blank(true)
        .build();
    ui.checkbox("Switch from Hold Key to Toggle Key", &mut state.toggle_mode);
    if state.toggle_mode || state.take_all_process_ids {
        state.disable_outside_target = false;
    }
    ui.checkbox("Freeze all Found Processes Instead of Newest", &mut state.take_all_process_ids);
    ui.same_line();
    ui.text_wrapped("(ONLY EVER USE FOR COMPATIBILITY ISSUES WITH NON-ROBLOX GAMES)");
    crate::parity::separator(ui);
    ui.text_wrapped("Explanation:");
    ui.new_line();
    ui.text_wrapped("This macro freezes the Roblox process. It can be used for lag high jumps, extended clips, head-glide setups, and other timing glitches.");
    ui.text_wrapped("Hold the hotkey to freeze and release it to unfreeze. In toggle mode, press once to freeze and press again to release. Auto-unfreeze keeps the process from staying suspended too long.");
}

#[cfg(test)]
mod tests {
    use super::FreezeUiState;

    #[test]
    fn defaults_match_cpp_freeze_ui() {
        let state = FreezeUiState::default();
        assert!(state.enabled);
        assert_eq!(state.hotkey, crate::freeze_runtime::DEFAULT_FREEZE_HOTKEY);
        assert!(state.disable_outside_target);
        assert!((state.max_seconds - 9.0).abs() < f32::EPSILON);
        assert_eq!(state.override_ms_text, "50");
        assert!(!state.toggle_mode);
        assert!(!state.take_all_process_ids);
    }

    #[test]
    fn toggle_or_all_process_mode_disables_foreground_restriction() {
        let mut state = FreezeUiState { toggle_mode: true, ..FreezeUiState::default() };
        let settings = state.controller_settings(true, "sober".to_owned());
        assert!(!settings.disable_outside_target);

        state.toggle_mode = false;
        state.take_all_process_ids = true;
        let settings = state.controller_settings(true, "sober".to_owned());
        assert!(!settings.disable_outside_target);
    }
}

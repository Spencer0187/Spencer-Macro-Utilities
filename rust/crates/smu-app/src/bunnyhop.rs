use crate::theme::ThemeDefinition;
use dear_imgui_rs::{StyleColor, Ui};
use serde_json::{Map, Value};
use smu_core::Hotkey;
use smu_profile::payload::PreservedProfilePayload;
use std::ffi::CString;

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
pub struct BunnyhopUiState {
    pub enabled: bool,
    pub hotkey: Hotkey,
    pub disable_outside_target: bool,
    pub smart: bool,
    delay_ms: i32,
    delay_text: String,
}

impl Default for BunnyhopUiState {
    fn default() -> Self {
        Self {
            enabled: false,
            hotkey: crate::bunnyhop_runtime::DEFAULT_BUNNYHOP_HOTKEY,
            disable_outside_target: true,
            smart: true,
            delay_ms: 10,
            delay_text: "10".to_owned(),
        }
    }
}

impl BunnyhopUiState {
    pub(crate) fn apply_profile(&mut self, payload: &PreservedProfilePayload) {
        if let Some(v) = crate::profile_adapter::u32_value(payload, "vk_bunnyhopkey") {
            self.hotkey = Hotkey::new(v);
        }
        if let Some(v) = crate::profile_adapter::bool_value(payload, "bunnyhopsmart") {
            self.smart = v;
        }
        if let Some(v) = crate::profile_adapter::string_value(payload, "BunnyHopDelayChar") {
            v.clone_into(&mut self.delay_text);
            self.delay_ms = cpp_atof_to_i32(v);
        }
    }
    pub(crate) fn overlay_profile(&self, overlay: &mut Map<String, Value>) {
        crate::profile_adapter::insert_u32(overlay, "vk_bunnyhopkey", self.hotkey.raw());
        crate::profile_adapter::insert_bool(overlay, "bunnyhopsmart", self.smart);
        crate::profile_adapter::insert_string(
            overlay,
            "BunnyHopDelayChar",
            self.delay_text.clone(),
        );
    }

    #[must_use]
    pub const fn controller_settings(
        &self,
        macro_enabled: bool,
        chat_key: u32,
        process_selector: String,
    ) -> crate::bunnyhop_runtime::BunnyhopControllerSettings {
        crate::bunnyhop_runtime::BunnyhopControllerSettings {
            macro_enabled,
            section_enabled: self.enabled,
            hotkey: self.hotkey,
            disable_outside_target: self.disable_outside_target,
            delay_ms: self.delay_ms,
            smart: self.smart,
            chat_key,
            process_selector,
        }
    }
}

pub fn render(
    ui: &Ui,
    state: &mut BunnyhopUiState,
    capture: &mut crate::keybind::CaptureState,
    theme: &ThemeDefinition,
) {
    ui.text_wrapped("Settings for Bunnyhop");
    crate::parity::separator(ui);
    ui.new_line();

    ui.text_wrapped("Keybind:");
    ui.same_line();
    crate::keybind::render_hotkey_control(
        ui,
        capture,
        "SectionKey13",
        &mut state.hotkey,
        170.0,
        130.0,
        false,
    );

    let color = ui.push_style_color(
        StyleColor::Text,
        if state.enabled { theme.success_color } else { theme.error_color },
    );
    ui.text_wrapped("Enable This Macro:");
    drop(color);
    ui.same_line();
    ui.checkbox("##SectionToggle13", &mut state.enabled);
    ui.same_line();
    crate::foreground_ui::render_checkbox(
        ui,
        "Disable outside of Target Application:",
        "##DisableOutsideRoblox_13",
        &mut state.disable_outside_target,
    );

    ui.text_wrapped("Bunnyhop Delay in Milliseconds (Default 10ms):");
    ui.same_line();
    ui.set_next_item_width(120.0);
    if ui
        .input_text("##BunnyhopDelay", &mut state.delay_text)
        .chars_decimal(true)
        .chars_no_blank(true)
        .build()
    {
        state.delay_ms = cpp_atof_to_i32(&state.delay_text);
    }
    ui.checkbox("Enable Intelligent Auto-Toggle", &mut state.smart);
    crate::parity::separator(ui);
    ui.text_wrapped("If Intelligent Auto-Toggle is on, pressing your chat key will temporarily disable bhop until you press left click or enter to leave the chat.");
    crate::parity::separator(ui);
    ui.text_wrapped("Explanation:");
    ui.new_line();
    ui.text_wrapped("This macro allows you to bunnyhop automatically while a movement key is held. It is a more practical spam-key setup for bhop, with smart toggling to avoid chat/input conflicts.");
    ui.text_wrapped("This will only be restricted to the target application when 'Disable outside of Target Application' is enabled.");
}

#[allow(clippy::cast_possible_truncation)] // Mirrors C++ `static_cast<int>(std::atof(...))`.
fn cpp_atof_to_i32(value: &str) -> i32 {
    let Ok(value) = CString::new(value) else {
        return 0;
    };
    // SAFETY: `CString` guarantees a valid NUL-terminated pointer for the duration of the call.
    unsafe { libc::strtod(value.as_ptr(), std::ptr::null_mut()) as i32 }
}

#[cfg(test)]
mod tests {
    use super::{BunnyhopUiState, cpp_atof_to_i32};
    use smu_core::KeyCode;

    #[test]
    fn defaults_match_cpp_bunnyhop_ui() {
        let state = BunnyhopUiState::default();
        assert!(!state.enabled);
        assert_eq!(state.hotkey.key(), KeyCode::SPACE);
        assert!(state.disable_outside_target);
        assert!(state.smart);
        assert_eq!(state.delay_ms, 10);
        assert_eq!(state.delay_text, "10");
    }

    #[test]
    fn delay_parser_keeps_cpp_numeric_prefix_behavior() {
        assert_eq!(cpp_atof_to_i32("20junk"), 20);
        assert_eq!(cpp_atof_to_i32("junk"), 0);
        assert_eq!(cpp_atof_to_i32("-3.9"), -3);
    }
}

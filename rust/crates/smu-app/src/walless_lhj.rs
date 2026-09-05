use crate::theme::ThemeDefinition;
use dear_imgui_rs::{StyleColor, Ui};
use serde_json::{Map, Value};
use smu_core::Hotkey;
use smu_profile::payload::PreservedProfilePayload;

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
pub struct WallessLhjGlobals {
    pub macro_enabled: bool,
    pub process_selector: String,
    pub global_zoom: bool,
    pub shift_key: u32,
    pub zoom_reverse: bool,
    pub take_all_process_ids: bool,
}

#[derive(Debug)]
pub struct WallessLhjUiState {
    pub enabled: bool,
    pub hotkey: Hotkey,
    pub disable_outside_target: bool,
    pub left_side: bool,
}

impl Default for WallessLhjUiState {
    fn default() -> Self {
        Self {
            enabled: false,
            hotkey: crate::walless_lhj_runtime::DEFAULT_WALLESS_LHJ_HOTKEY,
            disable_outside_target: false,
            left_side: false,
        }
    }
}

impl WallessLhjUiState {
    pub(crate) fn apply_profile(&mut self, payload: &PreservedProfilePayload) {
        if let Some(v) = crate::profile_adapter::u32_value(payload, "vk_f6") {
            self.hotkey = Hotkey::new(v);
        }
        if let Some(v) = crate::profile_adapter::bool_value(payload, "wallesslhjswitch") {
            self.left_side = v;
        }
    }
    pub(crate) fn overlay_profile(&self, overlay: &mut Map<String, Value>) {
        crate::profile_adapter::insert_u32(overlay, "vk_f6", self.hotkey.raw());
        crate::profile_adapter::insert_bool(overlay, "wallesslhjswitch", self.left_side);
    }

    #[must_use]
    pub fn controller_settings(
        &self,
        globals: WallessLhjGlobals,
    ) -> crate::walless_lhj_runtime::WallessLhjControllerSettings {
        crate::walless_lhj_runtime::WallessLhjControllerSettings {
            macro_enabled: globals.macro_enabled,
            section_enabled: self.enabled,
            hotkey: self.hotkey,
            disable_outside_target: self.disable_outside_target,
            side_left: self.left_side,
            global_zoom: globals.global_zoom,
            shift_key: globals.shift_key,
            zoom_reverse: globals.zoom_reverse,
            take_all_process_ids: globals.take_all_process_ids,
            process_selector: globals.process_selector,
        }
    }
}

pub fn render(
    ui: &Ui,
    state: &mut WallessLhjUiState,
    capture: &mut crate::keybind::CaptureState,
    theme: &ThemeDefinition,
) {
    ui.text_wrapped("Settings for Walless LHJ");
    crate::parity::separator(ui);
    ui.new_line();

    ui.text_wrapped("Keybind:");
    ui.same_line();
    crate::keybind::render_hotkey_control(
        ui,
        capture,
        "SectionKey7",
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
    ui.checkbox("##SectionToggle7", &mut state.enabled);
    ui.same_line();
    crate::foreground_ui::render_checkbox(
        ui,
        "Disable outside of Target Application:",
        "##DisableOutsideRoblox_7",
        &mut state.disable_outside_target,
    );

    ui.checkbox("Switch to Left-Sided LHJ", &mut state.left_side);
    crate::parity::separator(ui);
    ui.text_wrapped("Explanation:");
    ui.new_line();
    ui.text_wrapped("This macro allows you to perform a lag high jump without using a wall. Offset your center of mass sideways or downward, keep at least one full foot on the platform, then trigger the macro.");
}

#[cfg(test)]
mod tests {
    use super::WallessLhjUiState;

    #[test]
    fn defaults_match_cpp_walless_lhj_ui() {
        let state = WallessLhjUiState::default();
        assert!(!state.enabled);
        assert_eq!(state.hotkey, crate::walless_lhj_runtime::DEFAULT_WALLESS_LHJ_HOTKEY);
        assert!(!state.disable_outside_target);
        assert!(!state.left_side);
    }
}

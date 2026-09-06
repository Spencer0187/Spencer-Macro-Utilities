use crate::theme::ThemeDefinition;
use dear_imgui_rs::{StyleColor, Ui};
use serde_json::{Map, Value};
use smu_core::Hotkey;
use smu_profile::payload::PreservedProfilePayload;

#[derive(Debug)]
pub struct LaughClipUiState {
    pub enabled: bool,
    pub hotkey: Hotkey,
    pub disable_outside_target: bool,
    pub disable_s_press: bool,
}

impl Default for LaughClipUiState {
    fn default() -> Self {
        Self {
            enabled: false,
            hotkey: crate::laugh_clip_runtime::DEFAULT_LAUGH_CLIP_HOTKEY,
            disable_outside_target: true,
            disable_s_press: false,
        }
    }
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
pub struct LaughClipGlobals {
    pub macro_enabled: bool,
    pub process_selector: String,
    pub chat_override: bool,
    pub chat_key: u32,
    pub enter_key: u32,
    pub paste_delay_ms: i32,
    pub global_zoom: bool,
    pub shift_key: u32,
    pub zoom_reverse: bool,
}

impl LaughClipUiState {
    pub(crate) fn apply_profile(&mut self, payload: &PreservedProfilePayload) {
        if let Some(v) = crate::profile_adapter::u32_value(payload, "vk_laughkey") {
            self.hotkey = Hotkey::new(v);
        }
        if let Some(v) = crate::profile_adapter::bool_value(payload, "laughmoveswitch") {
            self.disable_s_press = v;
        }
    }
    pub(crate) fn overlay_profile(&self, overlay: &mut Map<String, Value>) {
        crate::profile_adapter::insert_u32(overlay, "vk_laughkey", self.hotkey.raw());
        crate::profile_adapter::insert_bool(overlay, "laughmoveswitch", self.disable_s_press);
    }

    #[must_use]
    pub fn controller_settings(
        &self,
        globals: LaughClipGlobals,
    ) -> crate::laugh_clip_runtime::LaughClipControllerSettings {
        crate::laugh_clip_runtime::LaughClipControllerSettings {
            macro_enabled: globals.macro_enabled,
            section_enabled: self.enabled,
            hotkey: self.hotkey,
            disable_outside_target: self.disable_outside_target,
            process_selector: globals.process_selector,
            chat_override: globals.chat_override,
            chat_key: globals.chat_key,
            enter_key: globals.enter_key,
            paste_delay_ms: globals.paste_delay_ms,
            global_zoom: globals.global_zoom,
            shift_key: globals.shift_key,
            zoom_reverse: globals.zoom_reverse,
            laugh_move_switch: self.disable_s_press,
        }
    }
}

pub fn render(
    ui: &Ui,
    state: &mut LaughClipUiState,
    capture: &mut crate::keybind::CaptureState,
    theme: &ThemeDefinition,
    media: crate::tutorial_media::TutorialMedia,
) {
    ui.text_wrapped("Settings for Laugh Clip");
    crate::parity::separator(ui);
    ui.new_line();

    ui.text_wrapped("Keybind:");
    ui.same_line();
    crate::keybind::render_hotkey_control(
        ui,
        capture,
        "SectionKey9",
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
    ui.checkbox("##SectionToggle9", &mut state.enabled);
    ui.same_line();
    crate::foreground_ui::render_checkbox(
        ui,
        "Disable outside of Target Application:",
        "##DisableOutsideRoblox_9",
        &mut state.disable_outside_target,
    );

    crate::parity::separator(ui);
    ui.checkbox(
        "Disable S being pressed (Slightly weaker laugh clips, but interferes with movement less)",
        &mut state.disable_s_press,
    );
    ui.text_wrapped("Explanation:");
    ui.new_line();
    ui.text_wrapped("This macro allows you to clip through walls of 1+ studs of thickness.");
    ui.text_wrapped("MUST BE ABOVE 60 FPS AND IN R6.");
    ui.text_wrapped("Set yourself and the camera up like this:");
    media.render_centered(ui, crate::tutorial_media::TutorialImage::LaughClip, 248.0, 140.0);
    ui.text_wrapped("After this, trigger the macro. It will type /e laugh using the settings from Item Unequip COM Offset. Higher FPS is better.");
}

#[cfg(test)]
mod tests {
    use super::{LaughClipGlobals, LaughClipUiState};
    use smu_core::KeyCode;

    #[test]
    fn defaults_match_cpp_laugh_clip_ui() {
        let state = LaughClipUiState::default();
        assert!(!state.enabled);
        assert_eq!(state.hotkey.key(), KeyCode::new(0x76));
        assert!(state.disable_outside_target);
        assert!(!state.disable_s_press);
    }

    #[test]
    fn controller_settings_forward_shared_chat_and_zoom_defaults() {
        let state = LaughClipUiState::default();
        let settings = state.controller_settings(LaughClipGlobals {
            macro_enabled: true,
            process_selector: "sober".to_owned(),
            chat_override: true,
            chat_key: 0xBF,
            enter_key: 0x0D,
            paste_delay_ms: 1,
            global_zoom: false,
            shift_key: 0x10,
            zoom_reverse: false,
        });
        assert!(settings.chat_override);
        assert_eq!(settings.enter_key, 0x0D);
        assert_eq!(settings.paste_delay_ms, 1);
        assert!(!settings.global_zoom);
        assert_eq!(settings.shift_key, 0x10);
        assert!(!settings.laugh_move_switch);
    }
}

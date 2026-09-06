use crate::theme::ThemeDefinition;
use dear_imgui_rs::{StyleColor, Ui};
use serde_json::{Map, Value};
use smu_core::Hotkey;
use smu_profile::payload::PreservedProfilePayload;

#[derive(Debug)]
pub struct ItemClipUiState {
    pub enabled: bool,
    pub hotkey: Hotkey,
    pub disable_outside_target: bool,
    pub hold_mode: bool,
    slot: i32,
    slot_text: String,
    delay_ms: i32,
    delay_text: String,
}

impl Default for ItemClipUiState {
    fn default() -> Self {
        Self {
            enabled: false,
            hotkey: crate::item_clip_runtime::DEFAULT_ITEM_CLIP_HOTKEY,
            disable_outside_target: true,
            hold_mode: false,
            slot: 7,
            slot_text: "7".to_owned(),
            delay_ms: 34,
            delay_text: "34".to_owned(),
        }
    }
}

impl ItemClipUiState {
    pub(crate) fn apply_profile(&mut self, payload: &PreservedProfilePayload) {
        if let Some(v) = crate::profile_adapter::u32_value(payload, "vk_clipkey") {
            self.hotkey = Hotkey::new(v);
        }
        if let Some(v) = crate::profile_adapter::bool_value(payload, "isitemclipswitch") {
            self.hold_mode = v;
        }
        if let Some(v) = crate::profile_adapter::i32_value(payload, "clip_slot") {
            self.slot = v;
        }
        if let Some(v) = crate::profile_adapter::string_value(payload, "ItemClipSlot") {
            v.clone_into(&mut self.slot_text);
            if let Ok(n) = v.parse::<i32>() {
                self.slot = n;
            }
        }
        if let Some(v) = crate::profile_adapter::i32_value(payload, "clip_delay") {
            self.delay_ms = v;
        }
        if let Some(v) = crate::profile_adapter::string_value(payload, "ItemClipDelay") {
            v.clone_into(&mut self.delay_text);
            if let Ok(n) = v.parse::<i32>() {
                self.delay_ms = n;
            }
        }
    }
    pub(crate) fn overlay_profile(&self, overlay: &mut Map<String, Value>) {
        crate::profile_adapter::insert_u32(overlay, "vk_clipkey", self.hotkey.raw());
        crate::profile_adapter::insert_bool(overlay, "isitemclipswitch", self.hold_mode);
        crate::profile_adapter::insert_i32(overlay, "clip_slot", self.slot);
        crate::profile_adapter::insert_string(overlay, "ItemClipSlot", self.slot_text.clone());
        crate::profile_adapter::insert_i32(overlay, "clip_delay", self.delay_ms);
        crate::profile_adapter::insert_string(overlay, "ItemClipDelay", self.delay_text.clone());
    }

    #[must_use]
    pub const fn controller_settings(
        &self,
        macro_enabled: bool,
        process_selector: String,
    ) -> crate::item_clip_runtime::ItemClipControllerSettings {
        crate::item_clip_runtime::ItemClipControllerSettings {
            macro_enabled,
            section_enabled: self.enabled,
            hotkey: self.hotkey,
            hold_mode: self.hold_mode,
            slot: self.slot,
            delay_ms: self.delay_ms,
            disable_outside_target: self.disable_outside_target,
            process_selector,
        }
    }
}

pub fn render(
    ui: &Ui,
    state: &mut ItemClipUiState,
    capture: &mut crate::keybind::CaptureState,
    theme: &ThemeDefinition,
    media: crate::tutorial_media::TutorialMedia,
) {
    ui.text_wrapped("Settings for Item Clip");
    crate::parity::separator(ui);
    ui.new_line();

    ui.text_wrapped("Keybind:");
    ui.same_line();
    crate::keybind::render_hotkey_control(
        ui,
        capture,
        "SectionKey8",
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
    ui.checkbox("##SectionToggle8", &mut state.enabled);
    ui.same_line();
    crate::foreground_ui::render_checkbox(
        ui,
        "Disable outside of Target Application:",
        "##DisableOutsideRoblox_8",
        &mut state.disable_outside_target,
    );

    ui.text_wrapped("Item Clip Slot:");
    ui.same_line();
    ui.set_next_item_width(30.0);
    if ui
        .input_text("##ItemClipSlot", &mut state.slot_text)
        .chars_decimal(true)
        .chars_no_blank(true)
        .build()
        && let Ok(slot) = state.slot_text.parse::<i32>()
    {
        state.slot = slot;
    }
    ui.text_wrapped("Item Clip Delay in Milliseconds (Default 34ms):");
    ui.same_line();
    ui.set_next_item_width(120.0);
    if ui
        .input_text("##ItemClipDelay", &mut state.delay_text)
        .chars_decimal(true)
        .chars_no_blank(true)
        .build()
        && let Ok(delay) = state.delay_text.parse::<i32>()
    {
        state.delay_ms = delay;
    }
    ui.checkbox("Switch from Toggle Key to Hold Key", &mut state.hold_mode);
    crate::parity::separator(ui);
    ui.text_wrapped("Explanation:");
    ui.new_line();
    ui.text_wrapped("This macro allows you to clip through thin walls using item equip timing.");
    media.render_centered(ui, crate::tutorial_media::TutorialImage::GearClip, 248.0, 140.0);
    ui.text_wrapped("To use this macro, shiftlock near the wall, jump, hold W, and let the macro equip and unequip the selected item using your delay. This can be RNG; lower FPS may help.");
    ui.text_wrapped("If 'Disable outside of Target Application' is enabled, item clip only runs while tabbed into the target application.");
}

#[cfg(test)]
mod tests {
    use super::ItemClipUiState;

    #[test]
    fn defaults_match_cpp_item_clip_ui() {
        let state = ItemClipUiState::default();
        assert!(!state.enabled);
        assert_eq!(state.hotkey, crate::item_clip_runtime::DEFAULT_ITEM_CLIP_HOTKEY);
        assert!(state.disable_outside_target);
        assert!(!state.hold_mode);
        assert_eq!(state.slot, 7);
        assert_eq!(state.delay_ms, 34);
    }
}

use dear_imgui_rs::{StyleColor, Ui};
use smu_core::Hotkey;

const SUCCESS: [f32; 4] = [0.0, 1.0, 0.50, 1.0];
const ERROR: [f32; 4] = [1.0, 0.20, 0.40, 1.0];
const EMOTES: [&str; 3] = ["/e dance2", "/e laugh", "/e cheer"];

#[derive(Debug)]
pub struct ItemUnequipUiState {
    pub enabled: bool,
    pub hotkey: Hotkey,
    pub disable_outside_target: bool,
    pub keep_equipped: bool,
    slot: i32,
    slot_text: String,
    custom_text: String,
    selected_emote: usize,
}

impl Default for ItemUnequipUiState {
    fn default() -> Self {
        Self {
            enabled: false,
            hotkey: crate::item_unequip_runtime::DEFAULT_ITEM_UNEQUIP_HOTKEY,
            disable_outside_target: true,
            keep_equipped: false,
            slot: 3,
            slot_text: "3".to_owned(),
            custom_text: String::new(),
            selected_emote: 0,
        }
    }
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
pub struct ItemUnequipGlobals {
    pub macro_enabled: bool,
    pub process_selector: String,
    pub chat_override: bool,
    pub chat_key: u32,
    pub enter_key: u32,
    pub paste_delay_ms: i32,
}

impl ItemUnequipUiState {
    #[must_use]
    pub fn controller_settings(
        &self,
        globals: ItemUnequipGlobals,
    ) -> crate::item_unequip_runtime::ItemUnequipControllerSettings {
        crate::item_unequip_runtime::ItemUnequipControllerSettings {
            macro_enabled: globals.macro_enabled,
            section_enabled: self.enabled,
            hotkey: self.hotkey,
            disable_outside_target: self.disable_outside_target,
            process_selector: globals.process_selector,
            custom_text: self.custom_text.clone(),
            emote_text: EMOTES[self.selected_emote].to_owned(),
            selected_dropdown: i32::try_from(self.selected_emote).unwrap_or_default(),
            speed_slot: self.slot,
            unequip_toggle: self.keep_equipped,
            chat_override: globals.chat_override,
            chat_key: globals.chat_key,
            enter_key: globals.enter_key,
            paste_delay_ms: globals.paste_delay_ms,
        }
    }
}

pub fn render(ui: &Ui, state: &mut ItemUnequipUiState) {
    ui.text_wrapped("Settings for Item Unequip COM Offset");
    crate::parity::separator(ui);
    ui.new_line();
    render_enablement(ui, state);
    render_content(ui, state);
}

fn render_enablement(ui: &Ui, state: &mut ItemUnequipUiState) {
    ui.text_wrapped("Keybind:");
    ui.same_line();
    ui.text(state.hotkey.key().legacy_label());

    let color = ui.push_style_color(StyleColor::Text, if state.enabled { SUCCESS } else { ERROR });
    ui.text_wrapped("Enable This Macro:");
    drop(color);
    ui.same_line();
    ui.checkbox("##SectionToggle4", &mut state.enabled);
    ui.same_line();
    ui.text_wrapped("Disable outside of Target Application:");
    ui.same_line();
    ui.checkbox("##DisableOutsideRoblox_4", &mut state.disable_outside_target);
}

fn render_content(ui: &Ui, state: &mut ItemUnequipUiState) {
    ui.text_wrapped("Gear Slot:");
    ui.same_line();
    ui.set_next_item_width(30.0);
    if ui
        .input_text("##Gearslot", &mut state.slot_text)
        .chars_decimal(true)
        .chars_no_blank(true)
        .build()
        && let Ok(slot) = state.slot_text.parse::<i32>()
    {
        state.slot = slot;
    }

    ui.text_wrapped(
        "Type in a custom chat message! (Disables gear equipping, just pastes your message in chat)",
    );
    ui.text_wrapped("(Leave this blank if you don't want a custom message)");
    ui.same_line();
    ui.set_next_item_width(ui.content_region_avail()[0]);
    ui.input_text("##CustomText", &mut state.custom_text).build();

    ui.set_next_item_width(150.0);
    let _ = ui.combo_simple_string("Select Emote", &mut state.selected_emote, &EMOTES);
    ui.align_text_to_frame_padding();
    ui.text_wrapped("Key to Press After Message/Emote paste: Return");
    ui.checkbox("Let the macro Keep the item equipped", &mut state.keep_equipped);
    crate::parity::separator(ui);
    ui.text_wrapped("IMPORTANT: This automatic emote unequip setup has been patched by Roblox in many places. You may still get a small COM offset manually by equipping an item without animation.");
    crate::parity::separator(ui);
    ui.text_wrapped("This macro allows you to create COM offset by sending an emote or custom chat message, then unequipping an item from the chosen slot. HHJ, Speedglitch, and Walless LHJ rely on this kind of offset.");
}

#[cfg(test)]
mod tests {
    use super::{EMOTES, ItemUnequipGlobals, ItemUnequipUiState};
    use smu_core::KeyCode;

    #[test]
    fn defaults_match_cpp_item_unequip_ui() {
        let state = ItemUnequipUiState::default();
        assert!(!state.enabled);
        assert_eq!(state.hotkey.key(), KeyCode::new(0x77));
        assert!(state.disable_outside_target);
        assert_eq!(state.slot, 3);
        assert_eq!(state.custom_text, "");
        assert_eq!(state.selected_emote, 0);
        assert_eq!(EMOTES[state.selected_emote], "/e dance2");
        assert!(!state.keep_equipped);
    }

    #[test]
    fn controller_snapshot_exposes_cpp_defaults() {
        let state = ItemUnequipUiState::default();
        let settings = state.controller_settings(ItemUnequipGlobals {
            macro_enabled: true,
            process_selector: "sober".to_owned(),
            chat_override: true,
            chat_key: 0xBF,
            enter_key: 0x0D,
            paste_delay_ms: 1,
        });
        assert_eq!(settings.emote_text, "/e dance2");
        assert_eq!(settings.selected_dropdown, 0);
        assert_eq!(settings.speed_slot, 3);
        assert!(settings.chat_override);
        assert_eq!(settings.paste_delay_ms, 1);
    }
}

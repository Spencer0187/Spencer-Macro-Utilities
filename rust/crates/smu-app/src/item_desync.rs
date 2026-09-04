use dear_imgui_rs::{StyleColor, Ui};
use smu_core::Hotkey;

const SUCCESS: [f32; 4] = [0.0, 1.0, 0.50, 1.0];
const ERROR: [f32; 4] = [1.0, 0.20, 0.40, 1.0];

#[derive(Debug)]
pub struct ItemDesyncUiState {
    pub enabled: bool,
    pub hotkey: Hotkey,
    pub disable_outside_target: bool,
    slot: i32,
    slot_text: String,
}

impl Default for ItemDesyncUiState {
    fn default() -> Self {
        Self {
            enabled: false,
            hotkey: crate::item_desync_runtime::DEFAULT_ITEM_DESYNC_HOTKEY,
            disable_outside_target: true,
            slot: 5,
            slot_text: "5".to_owned(),
        }
    }
}

impl ItemDesyncUiState {
    #[must_use]
    pub const fn controller_settings(
        &self,
        macro_enabled: bool,
        process_selector: String,
    ) -> crate::item_desync_runtime::ItemDesyncControllerSettings {
        crate::item_desync_runtime::ItemDesyncControllerSettings {
            macro_enabled,
            section_enabled: self.enabled,
            hotkey: self.hotkey,
            slot: self.slot,
            disable_outside_target: self.disable_outside_target,
            process_selector,
        }
    }
}

pub fn render(ui: &Ui, state: &mut ItemDesyncUiState) {
    ui.text_wrapped("Settings for Item Desync");
    crate::parity::separator(ui);
    ui.new_line();

    ui.text_wrapped("Keybind:");
    ui.same_line();
    ui.text(state.hotkey.key().legacy_label());

    let color = ui.push_style_color(StyleColor::Text, if state.enabled { SUCCESS } else { ERROR });
    ui.text_wrapped("Enable This Macro:");
    drop(color);
    ui.same_line();
    ui.checkbox("##SectionToggle1", &mut state.enabled);
    ui.same_line();
    ui.text_wrapped("Disable outside of Target Application:");
    ui.same_line();
    ui.checkbox("##DisableOutsideRoblox_1", &mut state.disable_outside_target);

    ui.text_wrapped("Gear Slot:");
    ui.same_line();
    ui.set_next_item_width(30.0);
    if ui
        .input_text("##ItemDesync", &mut state.slot_text)
        .chars_decimal(true)
        .chars_no_blank(true)
        .build()
        && let Ok(slot) = state.slot_text.parse::<i32>()
    {
        state.slot = slot;
    }
    crate::parity::separator(ui);
    ui.text_wrapped(
        "Equip your item in the slot selected here, then hold the keybind for about 4-7 seconds.",
    );
    crate::parity::separator(ui);
    ui.text_wrapped("Explanation:");
    ui.new_line();
    ui.text_wrapped("This macro allows you to desynchronize a held item from the client and server. It rapidly sends slot inputs until Roblox starts throttling the item state.");
    ui.text_wrapped("Use an item that does not make a server-side sound. Loud or server-driven gear can make the desync unstable and may stop your physics data from reaching the server cleanly.");
    crate::parity::separator(ui);
    ui.text_wrapped("If 'Disable outside of Target Application' is enabled, this module will only run while tabbed into the target application. Turning it off allows use in other windows (use carefully).");
}

#[cfg(test)]
mod tests {
    use super::ItemDesyncUiState;

    #[test]
    fn defaults_match_cpp_item_desync_ui() {
        let state = ItemDesyncUiState::default();
        assert!(!state.enabled);
        assert_eq!(state.hotkey, crate::item_desync_runtime::DEFAULT_ITEM_DESYNC_HOTKEY);
        assert!(state.disable_outside_target);
        assert_eq!(state.slot, 5);
        assert_eq!(state.slot_text, "5");
    }
}

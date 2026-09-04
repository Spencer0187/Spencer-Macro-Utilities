use dear_imgui_rs::{StyleColor, Ui};
use serde_json::{Map, Value};
use smu_core::Hotkey;
use smu_profile::payload::PreservedProfilePayload;

const SUCCESS: [f32; 4] = [0.0, 1.0, 0.50, 1.0];
const ERROR: [f32; 4] = [1.0, 0.20, 0.40, 1.0];

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
pub struct LedgeBounceUiState {
    pub enabled: bool,
    pub hotkey: Hotkey,
    pub disable_outside_target: bool,
    pub left_side: bool,
    pub realign: bool,
    pub auto_hold: bool,
}

impl Default for LedgeBounceUiState {
    fn default() -> Self {
        Self {
            enabled: false,
            hotkey: crate::ledge_bounce_runtime::DEFAULT_LEDGE_BOUNCE_HOTKEY,
            disable_outside_target: true,
            left_side: false,
            realign: true,
            auto_hold: true,
        }
    }
}

impl LedgeBounceUiState {
    pub(crate) fn apply_profile(&mut self, payload: &PreservedProfilePayload) {
        if let Some(v) = crate::profile_adapter::u32_value(payload, "vk_bouncekey") {
            self.hotkey = Hotkey::new(v);
        }
        if let Some(v) = crate::profile_adapter::bool_value(payload, "bouncesidetoggle") {
            self.left_side = v;
        }
        if let Some(v) = crate::profile_adapter::bool_value(payload, "bouncerealignsideways") {
            self.realign = v;
        }
        if let Some(v) = crate::profile_adapter::bool_value(payload, "bounceautohold") {
            self.auto_hold = v;
        }
    }
    pub(crate) fn overlay_profile(&self, overlay: &mut Map<String, Value>) {
        crate::profile_adapter::insert_u32(overlay, "vk_bouncekey", self.hotkey.raw());
        crate::profile_adapter::insert_bool(overlay, "bouncesidetoggle", self.left_side);
        crate::profile_adapter::insert_bool(overlay, "bouncerealignsideways", self.realign);
        crate::profile_adapter::insert_bool(overlay, "bounceautohold", self.auto_hold);
    }

    #[must_use]
    pub const fn controller_settings(
        &self,
        macro_enabled: bool,
        sensitivity: String,
        cam_fix: bool,
        process_selector: String,
    ) -> crate::ledge_bounce_runtime::LedgeBounceControllerSettings {
        crate::ledge_bounce_runtime::LedgeBounceControllerSettings {
            macro_enabled,
            section_enabled: self.enabled,
            hotkey: self.hotkey,
            disable_outside_target: self.disable_outside_target,
            sensitivity,
            cam_fix,
            side_left: self.left_side,
            realign: self.realign,
            auto_hold: self.auto_hold,
            process_selector,
        }
    }
}

pub fn render(ui: &Ui, state: &mut LedgeBounceUiState) {
    ui.text_wrapped("Settings for Ledge Bounce");
    crate::parity::separator(ui);
    ui.new_line();

    ui.text_wrapped("Keybind:");
    ui.same_line();
    ui.text(state.hotkey.key().legacy_label());

    let color = ui.push_style_color(StyleColor::Text, if state.enabled { SUCCESS } else { ERROR });
    ui.text_wrapped("Enable This Macro:");
    drop(color);
    ui.same_line();
    ui.checkbox("##SectionToggle12", &mut state.enabled);
    ui.same_line();
    ui.text_wrapped("Disable outside of Target Application:");
    ui.same_line();
    ui.checkbox("##DisableOutsideRoblox_12", &mut state.disable_outside_target);

    ui.checkbox("Switch Ledge Bounce to Left-Sided", &mut state.left_side);
    ui.checkbox("Stay Horizontal After Bounce", &mut state.realign);
    ui.checkbox("Automatically Hold Movement Keys", &mut state.auto_hold);
    crate::parity::separator(ui);
    ui.text_wrapped("IMPORTANT:");
    ui.text_wrapped("PLEASE SET YOUR ROBLOX SENS AND CAM-FIX CORRECTLY SO IT CAN ACTUALLY DO THE PROPER TURNS! It works best at high FPS (120+).");
    crate::parity::separator(ui);
    ui.text_wrapped("Explanation:");
    ui.new_line();
    ui.text_wrapped("This macro allows you to drop from a ledge and bounce back with timed camera movement. Walk up to a ledge with your camera sideways and about half of your left foot on the platform, then trigger the macro.");
}

#[cfg(test)]
mod tests {
    use super::LedgeBounceUiState;
    use smu_core::KeyCode;

    #[test]
    fn defaults_match_cpp_ledge_bounce_ui() {
        let state = LedgeBounceUiState::default();
        assert!(!state.enabled);
        assert_eq!(state.hotkey.key(), KeyCode::new(0x43));
        assert!(state.disable_outside_target);
        assert!(!state.left_side);
        assert!(state.realign);
        assert!(state.auto_hold);
    }
}

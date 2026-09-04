use crate::theme::ThemeDefinition;
use dear_imgui_rs::{StyleColor, Ui};
use serde_json::{Map, Value};
use smu_core::{Hotkey, KeyCode};
use smu_profile::payload::PreservedProfilePayload;

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
pub struct HhjUiState {
    pub enabled: bool,
    pub hotkey: Hotkey,
    pub disable_outside_target: bool,
    pub auto_toggle: bool,
    pub fast_hhj: bool,
    pub freeze_delay_apply: bool,
    length_ms: i32,
    freeze_delay_override_ms: i32,
    delay1_ms: i32,
    delay2_ms: i32,
    delay3_ms: i32,
    auto_key1: Hotkey,
    auto_key1_time_ms: i32,
    auto_key2: Hotkey,
    auto_key2_time_ms: i32,
    length_text: String,
    freeze_delay_text: String,
    delay1_text: String,
    delay2_text: String,
    delay3_text: String,
    auto_key1_time_text: String,
    auto_key2_time_text: String,
}

impl Default for HhjUiState {
    fn default() -> Self {
        Self {
            enabled: false,
            hotkey: crate::hhj_runtime::DEFAULT_HHJ_HOTKEY,
            disable_outside_target: false,
            auto_toggle: false,
            fast_hhj: false,
            freeze_delay_apply: false,
            length_ms: 243,
            freeze_delay_override_ms: 500,
            delay1_ms: 9,
            delay2_ms: 17,
            delay3_ms: 16,
            auto_key1: Hotkey::new(KeyCode::SPACE.raw()),
            auto_key1_time_ms: 550,
            auto_key2: Hotkey::new(0x57),
            auto_key2_time_ms: 68,
            length_text: "243".to_owned(),
            freeze_delay_text: "500".to_owned(),
            delay1_text: "9".to_owned(),
            delay2_text: "17".to_owned(),
            delay3_text: "16".to_owned(),
            auto_key1_time_text: "550".to_owned(),
            auto_key2_time_text: "68".to_owned(),
        }
    }
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
pub struct HhjGlobals {
    pub macro_enabled: bool,
    pub process_selector: String,
    pub take_all_process_ids: bool,
    pub global_zoom: bool,
    pub shift_key: u32,
    pub zoom_reverse: bool,
    pub fps: u32,
    pub strength_x: i32,
    pub strength_y: i32,
}

impl HhjUiState {
    pub(crate) fn apply_profile(&mut self, payload: &PreservedProfilePayload) {
        if let Some(value) = crate::profile_adapter::u32_value(payload, "vk_xbutton1") {
            self.hotkey = Hotkey::new(value);
        }
        if let Some(value) = crate::profile_adapter::bool_value(payload, "autotoggle") {
            self.auto_toggle = value;
        }
        if let Some(value) = crate::profile_adapter::bool_value(payload, "fasthhj") {
            self.fast_hhj = value;
        }
        if let Some(value) = crate::profile_adapter::bool_value(payload, "HHJFreezeDelayApply") {
            self.freeze_delay_apply = value;
        }
        apply_i32_buffer(payload, "HHJLengthChar", &mut self.length_text, &mut self.length_ms);
        apply_i32_buffer(
            payload,
            "HHJFreezeDelayOverrideChar",
            &mut self.freeze_delay_text,
            &mut self.freeze_delay_override_ms,
        );
        apply_i32_buffer(payload, "HHJDelay1Char", &mut self.delay1_text, &mut self.delay1_ms);
        apply_i32_buffer(payload, "HHJDelay2Char", &mut self.delay2_text, &mut self.delay2_ms);
        apply_i32_buffer(payload, "HHJDelay3Char", &mut self.delay3_text, &mut self.delay3_ms);
        if let Some(value) = crate::profile_adapter::u32_value(payload, "vk_autohhjkey1") {
            self.auto_key1 = Hotkey::new(value);
        }
        if let Some(value) = crate::profile_adapter::u32_value(payload, "vk_autohhjkey2") {
            self.auto_key2 = Hotkey::new(value);
        }
        apply_i32_buffer(
            payload,
            "AutoHHJKey1TimeChar",
            &mut self.auto_key1_time_text,
            &mut self.auto_key1_time_ms,
        );
        apply_i32_buffer(
            payload,
            "AutoHHJKey2TimeChar",
            &mut self.auto_key2_time_text,
            &mut self.auto_key2_time_ms,
        );
    }

    pub(crate) fn overlay_profile(&self, overlay: &mut Map<String, Value>) {
        crate::profile_adapter::insert_u32(overlay, "vk_xbutton1", self.hotkey.raw());
        crate::profile_adapter::insert_bool(overlay, "autotoggle", self.auto_toggle);
        crate::profile_adapter::insert_bool(overlay, "fasthhj", self.fast_hhj);
        crate::profile_adapter::insert_bool(
            overlay,
            "HHJFreezeDelayApply",
            self.freeze_delay_apply,
        );
        crate::profile_adapter::insert_string(overlay, "HHJLengthChar", self.length_text.clone());
        crate::profile_adapter::insert_string(
            overlay,
            "HHJFreezeDelayOverrideChar",
            self.freeze_delay_text.clone(),
        );
        crate::profile_adapter::insert_string(overlay, "HHJDelay1Char", self.delay1_text.clone());
        crate::profile_adapter::insert_string(overlay, "HHJDelay2Char", self.delay2_text.clone());
        crate::profile_adapter::insert_string(overlay, "HHJDelay3Char", self.delay3_text.clone());
        crate::profile_adapter::insert_u32(overlay, "vk_autohhjkey1", self.auto_key1.raw());
        crate::profile_adapter::insert_u32(overlay, "vk_autohhjkey2", self.auto_key2.raw());
        crate::profile_adapter::insert_i32(overlay, "AutoHHJKey1Time", self.auto_key1_time_ms);
        crate::profile_adapter::insert_i32(overlay, "AutoHHJKey2Time", self.auto_key2_time_ms);
        crate::profile_adapter::insert_string(
            overlay,
            "AutoHHJKey1TimeChar",
            self.auto_key1_time_text.clone(),
        );
        crate::profile_adapter::insert_string(
            overlay,
            "AutoHHJKey2TimeChar",
            self.auto_key2_time_text.clone(),
        );
    }

    #[must_use]
    pub fn controller_settings(
        &self,
        globals: HhjGlobals,
    ) -> crate::hhj_runtime::HhjControllerSettings {
        crate::hhj_runtime::HhjControllerSettings {
            macro_enabled: globals.macro_enabled,
            section_enabled: self.enabled,
            hotkey: self.hotkey,
            disable_outside_target: self.disable_outside_target,
            process_selector: globals.process_selector,
            take_all_process_ids: globals.take_all_process_ids,
            auto_toggle: self.auto_toggle,
            auto_key1: self.auto_key1,
            auto_key1_time_ms: self.auto_key1_time_ms,
            auto_key2: self.auto_key2,
            auto_key2_time_ms: self.auto_key2_time_ms,
            freeze_delay_apply: self.freeze_delay_apply,
            fast_hhj: self.fast_hhj,
            freeze_delay_override_ms: self.freeze_delay_override_ms,
            delay1_ms: self.delay1_ms,
            global_zoom: globals.global_zoom,
            shift_key: globals.shift_key,
            zoom_reverse: globals.zoom_reverse,
            delay2_ms: self.delay2_ms,
            length_ms: self.length_ms,
            delay3_ms: self.delay3_ms,
            fps: globals.fps,
            strength_x: globals.strength_x,
            strength_y: globals.strength_y,
        }
    }
}

fn apply_i32_buffer(
    payload: &PreservedProfilePayload,
    key: &str,
    text: &mut String,
    value: &mut i32,
) {
    let Some(raw) = crate::profile_adapter::string_value(payload, key) else {
        return;
    };
    raw.clone_into(text);
    if let Ok(parsed) = raw.parse::<i32>() {
        *value = parsed;
    }
}

pub fn render(
    ui: &Ui,
    state: &mut HhjUiState,
    capture: &mut crate::keybind::CaptureState,
    theme: &ThemeDefinition,
) {
    ui.text_wrapped("Settings for Wall Helicopter High Jump");
    crate::parity::separator(ui);
    ui.new_line();
    ui.text_wrapped("Keybind:");
    ui.same_line();
    crate::keybind::render_hotkey_control(
        ui,
        capture,
        "SectionKey2",
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
    ui.checkbox("##SectionToggle2", &mut state.enabled);
    ui.same_line();
    ui.text_wrapped("Disable outside of Target Application:");
    ui.same_line();
    ui.checkbox("##DisableOutsideRoblox_2", &mut state.disable_outside_target);

    ui.checkbox("Automatically time inputs", &mut state.auto_toggle);
    ui.same_line();
    ui.text_wrapped(
        "(EXTREMELY BUGGY/EXPERIMENTAL, WORKS BEST ON HIGH FPS AND SHALLOW ANGLE TO WALL)",
    );
    ui.checkbox("Decrease Freeze Duration (Speedrunner Mode)", &mut state.fast_hhj);
    render_reset_i32(
        ui,
        "R##HHJLength",
        "Length of HHJ flicks (ms):",
        &mut state.length_ms,
        &mut state.length_text,
        243,
    );

    crate::parity::separator(ui);
    ui.text_wrapped("Advanced HHJ Options");
    render_reset_i32(
        ui,
        "R##HHJFreezeDelayOverride",
        "Set freeze delay (ms):",
        &mut state.freeze_delay_override_ms,
        &mut state.freeze_delay_text,
        500,
    );
    ui.same_line();
    ui.checkbox("Apply##HHJFreezeDelayApply", &mut state.freeze_delay_apply);
    render_reset_i32(
        ui,
        "R##HHJDelay1",
        "Delay after freezing before shiftlock is held (ms):",
        &mut state.delay1_ms,
        &mut state.delay1_text,
        9,
    );
    render_reset_i32(
        ui,
        "R##HHJDelay2",
        "Time shiftlock is held before spinning (ms):",
        &mut state.delay2_ms,
        &mut state.delay2_text,
        17,
    );
    render_reset_i32(
        ui,
        "R##HHJDelay3",
        "Time shiftlock is held after freezing (ms):",
        &mut state.delay3_ms,
        &mut state.delay3_text,
        16,
    );

    crate::parity::separator(ui);
    render_auto_hhj_controls(ui, state, capture);

    crate::parity::separator(ui);
    ui.text_wrapped("This macro allows you to jump very high when used properly. It uses COM offset, angular velocity, and Roblox's center-of-mass behavior to launch you upward.");
    crate::parity::separator(ui);
    ui.text_wrapped("IMPORTANT:");
    ui.text_wrapped("Have your Sensitivity and Cam-Fix options set before using this module.");
    crate::parity::separator(ui);
    ui.text_wrapped("Explanation:");
    ui.new_line();
    ui.text_wrapped("First create COM offset, usually with Item Unequip COM Offset. Then align your back near the wall, rotate slightly left, turn your camera toward the wall, hold W, and trigger the macro.");
}

fn render_auto_hhj_controls(
    ui: &Ui,
    state: &mut HhjUiState,
    capture: &mut crate::keybind::CaptureState,
) {
    ui.text_wrapped("Customize Automatic HHJ");
    ui.text_wrapped("First Key Press:");
    ui.indent();
    if ui.button_with_size("R##AutoHHJKey1Reset", [25.0, 0.0]) {
        state.auto_key1 = Hotkey::new(KeyCode::SPACE.raw());
    }
    ui.same_line();
    crate::keybind::render_hotkey_control(
        ui,
        capture,
        "AutoHHJKey1",
        &mut state.auto_key1,
        150.0,
        50.0,
        false,
    );
    render_reset_i32(
        ui,
        "R##AutoHHJKey1Time",
        "Hold time (ms):",
        &mut state.auto_key1_time_ms,
        &mut state.auto_key1_time_text,
        550,
    );
    ui.unindent();
    ui.text_wrapped("Second Key Press:");
    ui.indent();
    if ui.button_with_size("R##AutoHHJKey2Reset", [25.0, 0.0]) {
        state.auto_key2 = Hotkey::new(0x57);
    }
    ui.same_line();
    crate::keybind::render_hotkey_control(
        ui,
        capture,
        "AutoHHJKey2",
        &mut state.auto_key2,
        150.0,
        50.0,
        false,
    );
    render_reset_i32(
        ui,
        "R##AutoHHJKey2Time",
        "Hold time (ms):",
        &mut state.auto_key2_time_ms,
        &mut state.auto_key2_time_text,
        68,
    );
    ui.unindent();
}

fn render_reset_i32(
    ui: &Ui,
    reset_id: &str,
    label: &str,
    value: &mut i32,
    text: &mut String,
    default_value: i32,
) {
    if ui.button_with_size(reset_id, [25.0, 0.0]) {
        *value = default_value;
        *text = default_value.to_string();
    }
    ui.same_line();
    ui.text_wrapped(label);
    ui.same_line();
    ui.set_next_item_width(52.0);
    if ui.input_text(format!("##{reset_id}"), text).chars_decimal(true).chars_no_blank(true).build()
        && let Ok(parsed) = text.parse::<i32>()
    {
        *value = parsed;
    }
}

#[cfg(test)]
mod tests {
    use super::HhjUiState;
    use smu_core::KeyCode;

    #[test]
    fn defaults_match_cpp_hhj_ui() {
        let state = HhjUiState::default();
        assert!(!state.enabled);
        assert_eq!(state.hotkey.key(), KeyCode::XBUTTON1);
        assert!(!state.disable_outside_target);
        assert!(!state.auto_toggle);
        assert!(!state.fast_hhj);
        assert!(!state.freeze_delay_apply);
        assert_eq!(state.length_ms, 243);
        assert_eq!(state.freeze_delay_override_ms, 500);
        assert_eq!((state.delay1_ms, state.delay2_ms, state.delay3_ms), (9, 17, 16));
        assert_eq!(state.auto_key1.key(), KeyCode::SPACE);
        assert_eq!(state.auto_key1_time_ms, 550);
        assert_eq!(state.auto_key2.key(), KeyCode::new(0x57));
        assert_eq!(state.auto_key2_time_ms, 68);
    }
}

use crate::theme::ThemeDefinition;
use dear_imgui_rs::{StyleColor, Ui};
use serde_json::{Map, Value};
use smu_core::Hotkey;
use smu_profile::payload::PreservedProfilePayload;
use std::ffi::CString;

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
pub struct WallWalkUiState {
    pub enabled: bool,
    pub hotkey: Hotkey,
    pub disable_outside_target: bool,
    pub left_flick: bool,
    pub hold_mode: bool,
    pixel_text: String,
    strength_x: i32,
    strength_y: i32,
    tail_delay_text: String,
    tail_delay_us: i32,
    previous_section_sensitivity: f32,
    applied_global_revision: u64,
}

impl Default for WallWalkUiState {
    fn default() -> Self {
        Self {
            enabled: false,
            hotkey: crate::wall_walk_runtime::DEFAULT_WALL_WALK_HOTKEY,
            disable_outside_target: true,
            left_flick: false,
            hold_mode: false,
            pixel_text: "-94".to_owned(),
            strength_x: 94,
            strength_y: -94,
            tail_delay_text: "72720".to_owned(),
            tail_delay_us: 72_720,
            previous_section_sensitivity: 0.5,
            applied_global_revision: 0,
        }
    }
}

impl WallWalkUiState {
    pub(crate) fn apply_profile(&mut self, payload: &PreservedProfilePayload) {
        if let Some(v) = crate::profile_adapter::u32_value(payload, "vk_wallkey") {
            self.hotkey = Hotkey::new(v);
        }
        if let Some(v) = crate::profile_adapter::bool_value(payload, "iswallwalkswitch") {
            self.hold_mode = v;
        }
        if let Some(v) = crate::profile_adapter::bool_value(payload, "wallwalktoggleside") {
            self.left_flick = v;
        }
        if let Some(v) = crate::profile_adapter::string_value(payload, "RobloxWallWalkValueChar") {
            v.clone_into(&mut self.pixel_text);
            if let Ok(n) = v.parse::<i32>() {
                self.strength_x = n;
                self.strength_y = n.wrapping_neg();
            }
        }
        if let Some(v) =
            crate::profile_adapter::string_value(payload, "RobloxWallWalkValueDelayChar")
        {
            v.clone_into(&mut self.tail_delay_text);
            if let Ok(n) = v.parse::<i32>() {
                self.tail_delay_us = n;
            }
        }
    }

    pub(crate) fn overlay_profile(&self, overlay: &mut Map<String, Value>) {
        crate::profile_adapter::insert_u32(overlay, "vk_wallkey", self.hotkey.raw());
        crate::profile_adapter::insert_bool(overlay, "iswallwalkswitch", self.hold_mode);
        crate::profile_adapter::insert_bool(overlay, "wallwalktoggleside", self.left_flick);
        crate::profile_adapter::insert_string(
            overlay,
            "RobloxWallWalkValueChar",
            self.pixel_text.clone(),
        );
        crate::profile_adapter::insert_i32(overlay, "RobloxWallWalkValueDelay", self.tail_delay_us);
        crate::profile_adapter::insert_string(
            overlay,
            "RobloxWallWalkValueDelayChar",
            self.tail_delay_text.clone(),
        );
    }

    #[allow(clippy::cast_possible_truncation)] // Mirrors C++ round()/int calibration behavior.
    pub fn sync_global_calibration(&mut self, sensitivity: &str, cam_fix: bool, revision: u64) {
        if self.applied_global_revision == revision {
            return;
        }
        let sensitivity = cpp_atof_f32(sensitivity);
        self.previous_section_sensitivity = -1.0;
        if sensitivity == 0.0 {
            return;
        }
        let base = if cam_fix { 500.0_f32 } else { 360.0_f32 };
        self.strength_x = -((base / sensitivity) * 0.13_f32).round() as i32;
        self.strength_y = -self.strength_x;
        self.pixel_text = self.strength_x.to_string();
        self.applied_global_revision = revision;
    }

    #[must_use]
    pub const fn controller_settings(
        &self,
        macro_enabled: bool,
        fps: u32,
        process_selector: String,
    ) -> crate::wall_walk_runtime::WallWalkControllerSettings {
        crate::wall_walk_runtime::WallWalkControllerSettings {
            macro_enabled,
            section_enabled: self.enabled,
            hotkey: self.hotkey,
            hold_mode: self.hold_mode,
            fps,
            left_flick: self.left_flick,
            strength_x: self.strength_x,
            strength_y: self.strength_y,
            tail_delay_us: self.tail_delay_us,
            disable_outside_target: self.disable_outside_target,
            process_selector,
        }
    }
}

#[allow(clippy::cast_possible_truncation, clippy::float_cmp)] // Mirrors exact C++ UI comparisons/casts.
pub fn render(
    ui: &Ui,
    state: &mut WallWalkUiState,
    sensitivity: &str,
    cam_fix: bool,
    capture: &mut crate::keybind::CaptureState,
    theme: &ThemeDefinition,
) {
    ui.text_wrapped("Settings for Wall-Walk");
    crate::parity::separator(ui);
    ui.new_line();

    ui.text_wrapped("Keybind:");
    ui.same_line();
    crate::keybind::render_hotkey_control(
        ui,
        capture,
        "SectionKey10",
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
    ui.checkbox("##SectionToggle10", &mut state.enabled);
    ui.same_line();
    crate::foreground_ui::render_checkbox(
        ui,
        "Disable outside of Target Application:",
        "##DisableOutsideRoblox_10",
        &mut state.disable_outside_target,
    );

    let current_sensitivity = cpp_atof_f32(sensitivity);
    if current_sensitivity != 0.0 && current_sensitivity != state.previous_section_sensitivity {
        let base = if cam_fix { 500.0_f32 } else { 360.0_f32 };
        state.strength_x = ((base / current_sensitivity) * 0.13_f32).round() as i32;
        state.strength_y = -state.strength_x;
    }
    state.previous_section_sensitivity = current_sensitivity;
    state.pixel_text = state.strength_x.to_string();

    ui.text_wrapped("Wall-Walk Pixel Value BASED ON SENSITIVITY (meant to be low):");
    ui.set_next_item_width(90.0);
    ui.same_line();
    ui.input_text("##WallWalkPixelValue", &mut state.pixel_text)
        .chars_decimal(true)
        .chars_no_blank(true)
        .build();
    ui.checkbox("Switch to Left-Flick Wallwalk", &mut state.left_flick);
    ui.set_next_item_width(100.0);
    ui.input_text(
        "Delay Between Flicks (Don't change from 72720 unless neccessary):",
        &mut state.tail_delay_text,
    )
    .chars_decimal(true)
    .chars_no_blank(true)
    .build();
    state.tail_delay_us = cpp_atof_f32(&state.tail_delay_text) as i32;
    if let Ok(pixel) = state.pixel_text.parse::<i32>() {
        state.strength_x = pixel;
        state.strength_y = -pixel;
    }
    ui.checkbox("Switch from Toggle Key to Hold Key", &mut state.hold_mode);
    crate::parity::separator(ui);
    ui.text_wrapped("IMPORTANT: For best results, input your Roblox in-game sensitivity.");
    ui.text_wrapped(
        "Higher FPS makes this more stable, but 60 FPS is enough for infinite distance.",
    );
    crate::parity::separator(ui);
    ui.text_wrapped("Explanation:");
    ui.new_line();
    ui.text_wrapped("This macro allows you to walk inside wall seams when there are two parts stacked above each other.");
    ui.text_wrapped("Walk up to the seam at a slight angle, then trigger the macro while holding W and D or W and A depending on the selected flick side.");
}

fn cpp_atof_f32(value: &str) -> f32 {
    let Ok(value) = CString::new(value) else {
        return 0.0;
    };
    // SAFETY: CString guarantees a NUL-terminated pointer valid through the call.
    unsafe { libc::strtof(value.as_ptr(), std::ptr::null_mut()) }
}

#[cfg(test)]
mod tests {
    use super::WallWalkUiState;

    #[test]
    fn defaults_match_cpp_pre_render_wall_walk_globals() {
        let state = WallWalkUiState::default();
        assert!(!state.enabled);
        assert_eq!(state.hotkey, crate::wall_walk_runtime::DEFAULT_WALL_WALK_HOTKEY);
        assert!(state.disable_outside_target);
        assert!(!state.left_flick);
        assert!(!state.hold_mode);
        assert_eq!(state.strength_x, 94);
        assert_eq!(state.strength_y, -94);
        assert_eq!(state.tail_delay_us, 72_720);
    }

    #[test]
    #[allow(clippy::cast_possible_truncation, clippy::float_cmp)]
    fn global_then_section_calibration_preserves_cpp_sign_flip() {
        let mut state = WallWalkUiState::default();
        state.sync_global_calibration("0.5", false, 1);
        assert_eq!(state.strength_x, -94);
        assert_eq!(state.strength_y, 94);
        // The selected section then sees PreviousWallWalkValue == -1 and flips to the positive form.
        let current = super::cpp_atof_f32("0.5");
        if current != 0.0 && current != state.previous_section_sensitivity {
            state.strength_x = ((360.0_f32 / current) * 0.13_f32).round() as i32;
            state.strength_y = -state.strength_x;
        }
        assert_eq!(state.strength_x, 94);
        assert_eq!(state.strength_y, -94);
    }
}

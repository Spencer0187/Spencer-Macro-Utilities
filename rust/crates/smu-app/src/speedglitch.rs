use crate::theme::ThemeDefinition;
use dear_imgui_rs::{StyleColor, Ui};
use serde_json::{Map, Value};
use smu_core::Hotkey;
use smu_profile::payload::PreservedProfilePayload;
use std::ffi::CString;

#[derive(Debug)]
pub struct SpeedglitchUiState {
    pub enabled: bool,
    pub hotkey: Hotkey,
    pub disable_outside_target: bool,
    pub hold_mode: bool,
    pixel_text: String,
    strength_x: i32,
    strength_y: i32,
    previous_sensitivity: f32,
    applied_calibration_revision: u64,
}

impl Default for SpeedglitchUiState {
    fn default() -> Self {
        Self {
            enabled: false,
            hotkey: crate::speedglitch_runtime::DEFAULT_SPEEDGLITCH_HOTKEY,
            disable_outside_target: true,
            hold_mode: false,
            pixel_text: "716".to_owned(),
            strength_x: 959,
            strength_y: -959,
            previous_sensitivity: -1.0,
            applied_calibration_revision: 0,
        }
    }
}

impl SpeedglitchUiState {
    pub(crate) fn apply_profile(&mut self, payload: &PreservedProfilePayload) {
        if let Some(value) = crate::profile_adapter::u32_value(payload, "vk_xkey") {
            self.hotkey = Hotkey::new(value);
        }
        if let Some(value) = crate::profile_adapter::bool_value(payload, "isspeedswitch") {
            self.hold_mode = value;
        }
        if let Some(value) = crate::profile_adapter::string_value(payload, "RobloxPixelValueChar") {
            value.clone_into(&mut self.pixel_text);
            if let Ok(parsed) = value.parse::<i32>() {
                self.strength_x = parsed;
                self.strength_y = parsed.wrapping_neg();
            }
        }
        if let Some(value) = crate::profile_adapter::f32_value(payload, "PreviousSensValue") {
            self.previous_sensitivity = value;
        }
    }

    pub(crate) fn reset_section_to_defaults(&mut self, defaults: &PreservedProfilePayload) {
        let previous_sensitivity = self.previous_sensitivity;
        let applied_calibration_revision = self.applied_calibration_revision;
        self.apply_profile(defaults);
        self.previous_sensitivity = previous_sensitivity;
        self.applied_calibration_revision = applied_calibration_revision;
    }

    pub(crate) fn overlay_profile(&self, overlay: &mut Map<String, Value>) {
        crate::profile_adapter::insert_u32(overlay, "vk_xkey", self.hotkey.raw());
        crate::profile_adapter::insert_bool(overlay, "isspeedswitch", self.hold_mode);
        crate::profile_adapter::insert_i32(overlay, "RobloxPixelValue", self.strength_x);
        crate::profile_adapter::insert_i32(overlay, "speed_strengthx", self.strength_x);
        crate::profile_adapter::insert_i32(overlay, "speed_strengthy", self.strength_y);
        crate::profile_adapter::insert_f32(overlay, "PreviousSensValue", self.previous_sensitivity);
        crate::profile_adapter::insert_string(
            overlay,
            "RobloxPixelValueChar",
            self.pixel_text.clone(),
        );
    }

    #[allow(clippy::cast_possible_truncation)] // Mirrors the C++ float-to-int calibration cast.
    pub fn sync_global_calibration(&mut self, sensitivity: &str, cam_fix: bool, revision: u64) {
        if self.applied_calibration_revision == revision {
            return;
        }
        let sensitivity = cpp_atof_f32(sensitivity);
        self.previous_sensitivity = -1.0;
        if sensitivity == 0.0 {
            return;
        }
        let base = if cam_fix { 500.0_f32 } else { 360.0_f32 };
        let ratio = 359.0_f32 / 360.0_f32;
        let pixel = ((base / sensitivity) * ratio * ratio).round() as i32;
        self.strength_x = pixel;
        self.strength_y = -pixel;
        self.pixel_text = pixel.to_string();
        self.previous_sensitivity = sensitivity;
        self.applied_calibration_revision = revision;
    }

    #[allow(clippy::float_cmp)] // Mirrors C++ sentinel comparison with PreviousSensValue == -1.
    pub(crate) fn restore_profile_calibration_latch(&mut self, revision: u64) -> bool {
        if self.previous_sensitivity == -1.0 {
            self.applied_calibration_revision = revision.wrapping_sub(1);
            false
        } else {
            self.applied_calibration_revision = revision;
            true
        }
    }

    #[allow(clippy::cast_possible_truncation, clippy::float_cmp, clippy::suboptimal_flops)] // Mirrors exact C++ float comparisons and operation order.
    fn sync_section_calibration(&mut self, sensitivity: &str, cam_fix: bool) {
        let sensitivity = cpp_atof_f32(sensitivity);
        if sensitivity == 0.0 || sensitivity == self.previous_sensitivity {
            return;
        }
        let base = if cam_fix { 500.0_f32 } else { 360.0_f32 };
        let ratio = 359.0_f32 / 360.0_f32;
        let pixel = ((base / sensitivity) * ratio + 0.5_f32) as i32;
        self.strength_x = pixel;
        self.strength_y = -pixel;
        self.pixel_text = pixel.to_string();
        self.previous_sensitivity = sensitivity;
    }

    #[must_use]
    pub const fn motion_strengths(&self) -> (i32, i32) {
        (self.strength_x, self.strength_y)
    }

    #[must_use]
    pub const fn controller_settings(
        &self,
        macro_enabled: bool,
        fps: u32,
        process_selector: String,
    ) -> crate::speedglitch_runtime::SpeedglitchControllerSettings {
        crate::speedglitch_runtime::SpeedglitchControllerSettings {
            macro_enabled,
            section_enabled: self.enabled,
            hotkey: self.hotkey,
            hold_mode: self.hold_mode,
            fps,
            strength_x: self.strength_x,
            strength_y: self.strength_y,
            disable_outside_target: self.disable_outside_target,
            process_selector,
        }
    }
}

pub fn render(
    ui: &Ui,
    state: &mut SpeedglitchUiState,
    sensitivity: &str,
    cam_fix: bool,
    capture: &mut crate::keybind::CaptureState,
    theme: &ThemeDefinition,
) {
    state.sync_section_calibration(sensitivity, cam_fix);
    ui.text_wrapped("Settings for Speedglitch");
    crate::parity::separator(ui);
    ui.new_line();

    ui.text_wrapped("Keybind:");
    ui.same_line();
    crate::keybind::render_hotkey_control(
        ui,
        capture,
        "SectionKey3",
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
    ui.checkbox("##SectionToggle3", &mut state.enabled);
    ui.same_line();
    crate::foreground_ui::render_checkbox(
        ui,
        "Disable outside of Target Application:",
        "##DisableOutsideRoblox_3",
        &mut state.disable_outside_target,
    );

    ui.text_wrapped("Pixel Value for 180 Degree Turn BASED ON SENSITIVITY:");
    ui.set_next_item_width(90.0);
    ui.same_line();
    if ui
        .input_text("##PixelValue", &mut state.pixel_text)
        .chars_decimal(true)
        .chars_no_blank(true)
        .build()
        && let Ok(pixel) = state.pixel_text.parse::<i32>()
    {
        state.strength_x = pixel;
        state.strength_y = -pixel;
    }
    ui.checkbox("Switch from Toggle Key to Hold Key", &mut state.hold_mode);
    ui.text_wrapped("This macro allows you to travel extremely fast in midair. It uses COM offset and a timed 180 degree camera turn to convert angular velocity into regular velocity.");
    crate::parity::separator(ui);
    ui.text_wrapped(
        "IMPORTANT: Have your Sensitivity and Cam-Fix options set before using this module.",
    );
    crate::parity::separator(ui);
    ui.text_wrapped("Explanation:");
    ui.new_line();
    ui.text_wrapped("Enable shiftlock, press the keybind once, then jump and hold W. The macro should rotate you exactly 180 degrees. If the turn is off, fix your Roblox sensitivity and Cam-Fix settings first.");
    ui.text_wrapped("Reference video:");
    let link = ui.push_style_color(StyleColor::Text, theme.accent_secondary);
    ui.same_line();
    ui.text_wrapped("https://www.youtube.com/watch?v=5rmeivUegHc");
    drop(link);
}

fn cpp_atof_f32(value: &str) -> f32 {
    let Ok(value) = CString::new(value) else {
        return 0.0;
    };
    // SAFETY: `CString` guarantees a NUL-terminated pointer valid for the duration of this call.
    unsafe { libc::strtof(value.as_ptr(), std::ptr::null_mut()) }
}

#[cfg(test)]
mod tests {
    use super::{SpeedglitchUiState, cpp_atof_f32};
    use smu_profile::payload::PreservedProfilePayload;

    #[test]
    fn defaults_match_cpp_pre_render_speedglitch_globals() {
        let state = SpeedglitchUiState::default();
        assert!(!state.enabled);
        assert_eq!(state.hotkey, crate::speedglitch_runtime::DEFAULT_SPEEDGLITCH_HOTKEY);
        assert!(state.disable_outside_target);
        assert!(!state.hold_mode);
        assert_eq!(state.pixel_text, "716");
        assert_eq!(state.strength_x, 959);
        assert_eq!(state.strength_y, -959);
        assert!((state.previous_sensitivity + 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn first_global_calibration_matches_cpp_squared_ratio_formula() {
        let mut state = SpeedglitchUiState::default();
        state.sync_global_calibration("0.5", false, 1);
        assert_eq!(state.pixel_text, "716");
        assert_eq!(state.strength_x, 716);
        assert_eq!(state.strength_y, -716);
        state.sync_global_calibration("0.6", false, 2);
        assert_eq!(state.strength_x, 597);
        assert_eq!(state.strength_y, -597);
    }

    #[test]
    fn zero_sensitivity_leaves_cpp_previous_calibration_pending() {
        let mut state = SpeedglitchUiState::default();
        state.sync_global_calibration("0", false, 1);
        assert_eq!(state.strength_x, 959);
        state.sync_global_calibration("0.5", false, 1);
        assert_eq!(state.strength_x, 716);
    }

    #[test]
    fn loaded_custom_strength_is_preserved_when_previous_sensitivity_matches() {
        let payload = PreservedProfilePayload::from_value(&serde_json::json!({
            "RobloxPixelValueChar": "812",
            "PreviousSensValue": 0.5
        }));
        assert!(payload.is_some());
        let Some(payload) = payload else { return };
        let mut state = SpeedglitchUiState::default();
        state.apply_profile(&payload);
        assert!(state.restore_profile_calibration_latch(7));
        state.sync_global_calibration("0.5", false, 7);
        state.sync_section_calibration("0.5", false);
        assert_eq!(state.motion_strengths(), (812, -812));
        assert_eq!(state.pixel_text, "812");
    }

    #[test]
    fn section_reset_preserves_previous_sensitivity_until_real_edit() {
        let defaults = PreservedProfilePayload::cpp_default();
        let mut state = SpeedglitchUiState {
            previous_sensitivity: 0.5,
            applied_calibration_revision: 7,
            strength_x: 812,
            strength_y: -812,
            pixel_text: "812".to_owned(),
            ..SpeedglitchUiState::default()
        };
        state.reset_section_to_defaults(&defaults);
        let reset_strengths = state.motion_strengths();
        assert!((state.previous_sensitivity - 0.5).abs() < f32::EPSILON);
        assert_eq!(state.applied_calibration_revision, 7);

        state.sync_section_calibration("0.5", false);
        assert_eq!(state.motion_strengths(), reset_strengths);

        state.sync_section_calibration("0.6", false);
        assert_ne!(state.motion_strengths(), reset_strengths);
        assert!((state.previous_sensitivity - 0.6).abs() < f32::EPSILON);
    }

    #[test]
    fn section_recalibrates_after_real_sensitivity_change() {
        let mut state = SpeedglitchUiState {
            previous_sensitivity: 0.5,
            strength_x: 812,
            strength_y: -812,
            ..SpeedglitchUiState::default()
        };
        state.sync_section_calibration("0.6", false);
        assert_eq!(state.motion_strengths(), (598, -598));
        assert!((state.previous_sensitivity - 0.6).abs() < f32::EPSILON);
    }

    #[test]
    fn atof_prefix_behavior_is_kept_for_profile_compatibility() {
        assert!((cpp_atof_f32(" 0.5junk") - 0.5).abs() < f32::EPSILON);
        assert!(cpp_atof_f32("junk").abs() < f32::EPSILON);
    }
}

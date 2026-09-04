use dear_imgui_rs::{StyleColor, Ui};
use smu_core::Hotkey;
use std::ffi::CString;

const ACCENT_SECONDARY: [f32; 4] = [0.0, 0.44, 0.60, 1.0];
const SUCCESS: [f32; 4] = [0.0, 1.0, 0.50, 1.0];
const ERROR: [f32; 4] = [1.0, 0.20, 0.40, 1.0];

#[derive(Debug)]
pub struct SpeedglitchUiState {
    pub enabled: bool,
    pub hotkey: Hotkey,
    pub disable_outside_target: bool,
    pub hold_mode: bool,
    pixel_text: String,
    strength_x: i32,
    strength_y: i32,
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
            applied_calibration_revision: 0,
        }
    }
}

impl SpeedglitchUiState {
    #[allow(clippy::cast_possible_truncation)] // Mirrors the C++ float-to-int calibration cast.
    pub fn sync_global_calibration(&mut self, sensitivity: &str, cam_fix: bool, revision: u64) {
        if self.applied_calibration_revision == revision {
            return;
        }
        let sensitivity = cpp_atof_f32(sensitivity);
        if sensitivity == 0.0 {
            return;
        }
        let base = if cam_fix { 500.0_f32 } else { 360.0_f32 };
        let ratio = 359.0_f32 / 360.0_f32;
        let pixel = ((base / sensitivity) * ratio * ratio).round() as i32;
        self.strength_x = pixel;
        self.strength_y = -pixel;
        self.pixel_text = pixel.to_string();
        self.applied_calibration_revision = revision;
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

pub fn render(ui: &Ui, state: &mut SpeedglitchUiState) {
    ui.text_wrapped("Settings for Speedglitch");
    crate::parity::separator(ui);
    ui.new_line();

    ui.text_wrapped("Keybind:");
    ui.same_line();
    ui.text(state.hotkey.key().legacy_label());

    let color = ui.push_style_color(StyleColor::Text, if state.enabled { SUCCESS } else { ERROR });
    ui.text_wrapped("Enable This Macro:");
    drop(color);
    ui.same_line();
    ui.checkbox("##SectionToggle3", &mut state.enabled);
    ui.same_line();
    ui.text_wrapped("Disable outside of Target Application:");
    ui.same_line();
    ui.checkbox("##DisableOutsideRoblox_3", &mut state.disable_outside_target);

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
    let link = ui.push_style_color(StyleColor::Text, ACCENT_SECONDARY);
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
    fn atof_prefix_behavior_is_kept_for_profile_compatibility() {
        assert!((cpp_atof_f32(" 0.5junk") - 0.5).abs() < f32::EPSILON);
        assert!(cpp_atof_f32("junk").abs() < f32::EPSILON);
    }
}

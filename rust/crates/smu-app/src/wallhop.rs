use crate::multi_instance::MultiInstanceState;
use dear_imgui_rs::{StyleColor, TreeNodeFlags, Ui};
use smu_core::{Hotkey, KeyCode};
use std::ffi::CString;

const SUCCESS: [f32; 4] = [0.0, 1.0, 0.50, 1.0];
const ERROR: [f32; 4] = [1.0, 0.20, 0.40, 1.0];
const ARMED_TEXT: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const ARMED_BUTTON: [f32; 4] = [1.0, 1.0, 0.0, 1.0];

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WallhopUiInstance {
    pub enabled: bool,
    pub trigger: Hotkey,
    pub disable_outside_target: bool,
    pub left_flick: bool,
    pub jump: bool,
    pub flick_back: bool,
    pub jump_key: Hotkey,
    dx: i32,
    dy: i32,
    vertical: i32,
    delay_ms: i32,
    bonus_delay_ms: i32,
    pixels_text: String,
    vertical_text: String,
    delay_text: String,
    bonus_delay_text: String,
    degrees_text: String,
}

impl Default for WallhopUiInstance {
    fn default() -> Self {
        Self {
            enabled: false,
            trigger: Hotkey::new(KeyCode::XBUTTON2.raw()),
            disable_outside_target: false,
            left_flick: false,
            jump: true,
            flick_back: true,
            jump_key: Hotkey::new(KeyCode::SPACE.raw()),
            dx: 300,
            dy: -300,
            vertical: 0,
            delay_ms: 19,
            bonus_delay_ms: 0,
            pixels_text: "300".to_owned(),
            vertical_text: "0".to_owned(),
            delay_text: "19".to_owned(),
            bonus_delay_text: "0".to_owned(),
            degrees_text: "150".to_owned(),
        }
    }
}

#[derive(Debug, Default)]
pub struct WallhopUiState {
    instances: MultiInstanceState<WallhopUiInstance>,
    applied_sensitivity_revision: u64,
}

impl WallhopUiState {
    #[must_use]
    pub fn instances(&self) -> &[WallhopUiInstance] {
        self.instances.instances()
    }

    #[must_use]
    pub const fn selected_index(&self) -> usize {
        self.instances.selected_index()
    }

    pub const fn select(&mut self, index: usize) {
        self.instances.select(index);
    }

    pub fn add_default(&mut self) {
        self.instances.add_default();
    }

    pub fn tick_remove_confirmation(&mut self, delta_seconds: f32, section_selected: bool) {
        self.instances.tick_remove_confirmation(delta_seconds, section_selected);
    }

    #[must_use]
    pub fn first_enabled(&self) -> bool {
        self.instances().first().is_some_and(|instance| instance.enabled)
    }

    #[must_use]
    pub fn enabled_flags(&self) -> Vec<bool> {
        self.instances().iter().map(|instance| instance.enabled).collect()
    }

    #[allow(clippy::cast_possible_truncation)] // Mirrors C++ sensitivity-edit recalibration.
    pub fn sync_sensitivity_edit(&mut self, sensitivity: &str, cam_fix: bool, revision: u64) {
        if self.applied_sensitivity_revision == revision {
            return;
        }
        let sensitivity = cpp_atof_f32(sensitivity);
        if sensitivity != 0.0 {
            let scale = if cam_fix { 1_000.0_f32 } else { 720.0_f32 };
            for instance in self.instances.instances_mut() {
                let degrees = cpp_atof_f32(&instance.degrees_text);
                let pixels = degrees * scale / (360.0 * sensitivity);
                let rounded = pixels.round() as i32;
                instance.dx = rounded;
                instance.dy = rounded.wrapping_neg();
                instance.pixels_text = rounded.to_string();
            }
        }
        self.applied_sensitivity_revision = revision;
    }

    #[must_use]
    pub fn controller_settings(
        &self,
        macro_enabled: bool,
        process_selector: String,
    ) -> crate::wallhop_runtime::WallhopControllerSettings {
        crate::wallhop_runtime::WallhopControllerSettings {
            macro_enabled,
            process_selector,
            instances: self
                .instances()
                .iter()
                .map(|instance| crate::wallhop_runtime::WallhopInstanceSettings {
                    section_enabled: instance.enabled,
                    trigger: instance.trigger,
                    disable_outside_target: instance.disable_outside_target,
                    dx: instance.dx,
                    dy: instance.dy,
                    vertical: instance.vertical,
                    delay_ms: instance.delay_ms,
                    bonus_delay_ms: instance.bonus_delay_ms,
                    left: instance.left_flick,
                    jump: instance.jump,
                    flick_back: instance.flick_back,
                    jump_key: instance.jump_key,
                })
                .collect(),
        }
    }
}

#[allow(clippy::cast_possible_truncation, clippy::float_cmp)]
pub fn render(ui: &Ui, state: &mut WallhopUiState, sensitivity: &str, cam_fix: bool) {
    let count = state.instances.instances().len();
    let selected = state.instances.selected_index();
    let sid = format!("_wh{selected}");
    render_instance_header(ui, state, count, selected);
    {
        let instance = state.instances.selected_mut();
        render_instance_enablement(ui, instance, &sid);
    }
    render_instance_controls(ui, state, count, selected, &sid);
    let instance = state.instances.selected_mut();
    render_instance_motion(ui, instance, sensitivity, cam_fix, &sid);
    render_instance_options(ui, instance, &sid);

    crate::parity::separator(ui);
    ui.text_wrapped("Explanation:");
    ui.new_line();
    ui.text_wrapped("This macro automates the camera flick, optional jump, and flick-back timing used for wallhops. Each instance has independent settings and a trigger key.");
}

fn render_instance_header(ui: &Ui, state: &mut WallhopUiState, count: usize, selected: usize) {
    if count > 1 {
        ui.text_wrapped(format!("Settings for Wallhop/Rotation {}", selected + 1));
    } else {
        ui.text_wrapped("Settings for Wallhop/Rotation");
    }
    crate::parity::separator(ui);
    ui.new_line();
    ui.text_wrapped("Keybind:");
    ui.same_line();
    ui.text(state.instances.selected_mut().trigger.key().legacy_label());
}

fn render_instance_controls(
    ui: &Ui,
    state: &mut WallhopUiState,
    count: usize,
    selected: usize,
    sid: &str,
) {
    if count > 1 {
        ui.text_wrapped(format!("Wallhop/Rotation {} of {count}", selected + 1));
        ui.same_line();
    }
    if ui.button(format!("+ Add New##AddWH{sid}")) {
        state.instances.add_default();
    }
    render_remove_button(ui, state, sid);
    crate::parity::separator(ui);
}

fn render_instance_enablement(ui: &Ui, instance: &mut WallhopUiInstance, sid: &str) {
    let color =
        ui.push_style_color(StyleColor::Text, if instance.enabled { SUCCESS } else { ERROR });
    ui.text_wrapped("Enable This Macro:");
    drop(color);
    ui.same_line();
    ui.checkbox(format!("##WallhopEnabled{sid}"), &mut instance.enabled);
    ui.same_line();
    ui.text_wrapped("Disable outside of Target Application:");
    ui.same_line();
    ui.checkbox(format!("##WallhopOutside{sid}"), &mut instance.disable_outside_target);
}

#[allow(clippy::cast_possible_truncation, clippy::float_cmp)]
fn render_instance_motion(
    ui: &Ui,
    instance: &mut WallhopUiInstance,
    sensitivity: &str,
    cam_fix: bool,
    sid: &str,
) {
    let sens = cpp_atof_f32(sensitivity);
    if sens != 0.0 {
        let denom = if cam_fix { 1_000.0_f32 } else { 720.0_f32 };
        let pixels = cpp_atof_f32(&instance.pixels_text);
        instance.degrees_text = ((360.0 * (pixels * sens) / denom) as i32).to_string();
    }

    ui.text_wrapped("Flick Degrees (Estimated):");
    ui.same_line();
    ui.set_next_item_width(70.0);
    if ui
        .input_text(format!("##WallhopDegrees{sid}"), &mut instance.degrees_text)
        .chars_decimal(true)
        .chars_no_blank(true)
        .build()
        && sens != 0.0
    {
        let scale = if cam_fix { 1_000.0_f32 } else { 720.0_f32 };
        let pixels = cpp_atof_f32(&instance.degrees_text) * scale / (360.0 * sens);
        let rounded = pixels.round() as i32;
        instance.dx = rounded;
        instance.dy = rounded.wrapping_neg();
        instance.pixels_text = rounded.to_string();
    }

    ui.text_wrapped("Flick Pixel Amount:");
    ui.same_line();
    ui.set_next_item_width(70.0);
    if ui
        .input_text(format!("##WallhopPixels{sid}"), &mut instance.pixels_text)
        .chars_decimal(true)
        .chars_no_blank(true)
        .build()
        && let Ok(value) = instance.pixels_text.parse::<i32>()
    {
        instance.dx = value;
        instance.dy = value.wrapping_neg();
    }
    ui.same_line();
    ui.text("Vertical Pixel Movement:");
    ui.same_line();
    ui.set_next_item_width(70.0);
    if ui
        .input_text(format!("##WallhopVertical{sid}"), &mut instance.vertical_text)
        .chars_decimal(true)
        .chars_no_blank(true)
        .build()
        && let Ok(value) = instance.vertical_text.parse::<i32>()
    {
        instance.vertical = value;
    }
}

fn render_instance_options(ui: &Ui, instance: &mut WallhopUiInstance, sid: &str) {
    ui.text_wrapped("Wallhop Length (ms):");
    ui.same_line();
    ui.set_next_item_width(70.0);
    if ui
        .input_text(format!("##WallhopDelay{sid}"), &mut instance.delay_text)
        .chars_decimal(true)
        .chars_no_blank(true)
        .build()
        && let Ok(value) = instance.delay_text.parse::<i32>()
    {
        instance.delay_ms = value;
    }
    ui.text_wrapped("Bonus Wallhop Delay Before Jumping (ms):");
    ui.same_line();
    ui.set_next_item_width(70.0);
    if ui
        .input_text(format!("##WallhopBonusDelay{sid}"), &mut instance.bonus_delay_text)
        .chars_decimal(true)
        .chars_no_blank(true)
        .build()
        && let Ok(value) = instance.bonus_delay_text.parse::<i32>()
    {
        instance.bonus_delay_ms = value;
    }
    ui.checkbox(format!("Switch to Left-Flick Wallhop##{sid}"), &mut instance.left_flick);
    ui.checkbox(format!("Jump During Wallhop##{sid}"), &mut instance.jump);
    ui.checkbox(format!("Flick-Back During Wallhop##{sid}"), &mut instance.flick_back);
    if ui.collapsing_header(format!("Hotkeys##WallhopHotkeys{sid}"), TreeNodeFlags::DEFAULT_OPEN) {
        if ui.button(format!("R##WallhopJumpKeyReset{sid}")) {
            instance.jump_key = Hotkey::new(KeyCode::SPACE.raw());
        }
        ui.same_line();
        ui.text_wrapped("Jump Key:");
        ui.same_line();
        ui.text(instance.jump_key.key().legacy_label());
    }
}

fn render_remove_button(ui: &Ui, state: &mut WallhopUiState, sid: &str) {
    let Some(_target) = state.instances.removal_target() else {
        return;
    };
    ui.same_line();
    let armed = state.instances.removal_is_armed();
    let _text = armed.then(|| ui.push_style_color(StyleColor::Text, ARMED_TEXT));
    let _button = armed.then(|| ui.push_style_color(StyleColor::Button, ARMED_BUTTON));
    let _hovered = armed.then(|| ui.push_style_color(StyleColor::ButtonHovered, ARMED_BUTTON));
    let _active = armed.then(|| ui.push_style_color(StyleColor::ButtonActive, ARMED_BUTTON));
    let label = if state.instances.selected_index() == 0 {
        "- Remove Last Instance"
    } else {
        "- Remove Current Instance"
    };
    if ui.button(format!("{label}##RemWH{sid}")) {
        let _ = state.instances.click_remove();
    }
}

fn cpp_atof_f32(value: &str) -> f32 {
    let Ok(value) = CString::new(value) else {
        return 0.0;
    };
    // SAFETY: CString guarantees a NUL-terminated pointer valid for this call.
    unsafe { libc::strtof(value.as_ptr(), std::ptr::null_mut()) }
}

#[cfg(test)]
mod tests {
    use super::{WallhopUiInstance, WallhopUiState};
    use smu_core::KeyCode;

    #[test]
    fn defaults_match_cpp_wallhop_instance() {
        let instance = WallhopUiInstance::default();
        assert!(!instance.enabled);
        assert_eq!(instance.trigger.key(), KeyCode::XBUTTON2);
        assert!(!instance.disable_outside_target);
        assert_eq!(instance.dx, 300);
        assert_eq!(instance.dy, -300);
        assert_eq!(instance.vertical, 0);
        assert_eq!(instance.delay_ms, 19);
        assert_eq!(instance.bonus_delay_ms, 0);
        assert!(!instance.left_flick);
        assert!(instance.jump);
        assert!(instance.flick_back);
        assert_eq!(instance.jump_key.key(), KeyCode::SPACE);
        assert_eq!(instance.degrees_text, "150");
    }

    #[test]
    fn sensitivity_edit_recalibrates_all_existing_instances_only_once() {
        let mut state = WallhopUiState::default();
        state.add_default();
        state.sync_sensitivity_edit("0.5", false, 1);
        assert_eq!(state.instances()[0].dx, 600);
        assert_eq!(state.instances()[1].dx, 600);
        state.add_default();
        state.sync_sensitivity_edit("0.5", false, 1);
        assert_eq!(state.instances()[2].dx, 300);
    }
}

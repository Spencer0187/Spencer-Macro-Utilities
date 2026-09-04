use crate::multi_instance::MultiInstanceState;
use crate::theme::ThemeDefinition;
use dear_imgui_rs::{StyleColor, TreeNodeFlags, Ui};
use serde_json::{Map, Value, json};
use smu_core::{Hotkey, KeyCode};
use smu_profile::payload::PreservedProfilePayload;
use std::ffi::CString;

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
    wallhop_cam_fix: bool,
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
            wallhop_cam_fix: false,
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

fn wallhop_base_from_profile(payload: &PreservedProfilePayload) -> WallhopUiInstance {
    let mut instance = WallhopUiInstance::default();
    if let Some(value) = crate::profile_adapter::u32_value(payload, "vk_xbutton2") {
        instance.trigger = Hotkey::new(value);
    }
    if let Some(value) = crate::profile_adapter::u32_value(payload, "vk_wallhopjumpkey") {
        instance.jump_key = Hotkey::new(value);
    }
    if let Some(value) = crate::profile_adapter::i32_value(payload, "wallhop_dx") {
        instance.dx = value;
    }
    if let Some(value) = crate::profile_adapter::i32_value(payload, "wallhop_dy") {
        instance.dy = value;
    }
    if let Some(value) = crate::profile_adapter::i32_value(payload, "wallhop_vertical") {
        instance.vertical = value;
    }
    for (key, target) in [
        ("WallhopPixels", &mut instance.pixels_text),
        ("WallhopVerticalChar", &mut instance.vertical_text),
        ("WallhopDelayChar", &mut instance.delay_text),
        ("WallhopBonusDelayChar", &mut instance.bonus_delay_text),
    ] {
        if let Some(value) = crate::profile_adapter::string_value(payload, key) {
            value.clone_into(target);
        }
    }
    if let Ok(parsed) = instance.delay_text.parse::<i32>() {
        instance.delay_ms = parsed;
    }
    if let Ok(parsed) = instance.bonus_delay_text.parse::<i32>() {
        instance.bonus_delay_ms = parsed;
    }
    if let Some(value) = crate::profile_adapter::bool_value(payload, "wallhopswitch") {
        instance.left_flick = value;
    }
    if let Some(value) = crate::profile_adapter::bool_value(payload, "toggle_jump") {
        instance.jump = value;
    }
    if let Some(value) = crate::profile_adapter::bool_value(payload, "toggle_flick") {
        instance.flick_back = value;
    }
    if let Some(value) = crate::profile_adapter::bool_value(payload, "wallhopcamfix") {
        instance.wallhop_cam_fix = value;
    }
    if let Some(value) = crate::profile_adapter::section_flag(payload, "section_toggles", 6) {
        instance.enabled = value;
    }
    if let Some(value) = crate::profile_adapter::section_flag(payload, "disable_outside_roblox", 6)
    {
        instance.disable_outside_target = value;
    }
    instance
}

fn wallhop_extra_from_profile(
    object: &Map<String, Value>,
    base_disable_outside: bool,
) -> WallhopUiInstance {
    let mut instance = WallhopUiInstance::default();
    if let Some(value) = crate::profile_adapter::object_u32(object, "vk_trigger") {
        instance.trigger = Hotkey::new(value);
    }
    if let Some(value) = crate::profile_adapter::object_u32(object, "vk_jumpkey") {
        instance.jump_key = Hotkey::new(value);
    }
    if let Some(value) = crate::profile_adapter::object_i32(object, "wallhop_dx") {
        instance.dx = value;
    }
    if let Some(value) = crate::profile_adapter::object_i32(object, "wallhop_dy") {
        instance.dy = value;
    }
    if let Some(value) = crate::profile_adapter::object_i32(object, "wallhop_vertical") {
        instance.vertical = value;
    }
    if let Some(value) = crate::profile_adapter::object_i32(object, "WallhopDelay") {
        instance.delay_ms = value;
    }
    if let Some(value) = crate::profile_adapter::object_i32(object, "WallhopBonusDelay") {
        instance.bonus_delay_ms = value;
    }
    for (key, target) in [
        ("WallhopPixels", &mut instance.pixels_text),
        ("WallhopVerticalChar", &mut instance.vertical_text),
        ("WallhopDelayChar", &mut instance.delay_text),
        ("WallhopBonusDelayChar", &mut instance.bonus_delay_text),
        ("WallhopDegrees", &mut instance.degrees_text),
    ] {
        if let Some(value) = crate::profile_adapter::object_string(object, key) {
            value.clone_into(target);
        }
    }
    if let Some(value) = crate::profile_adapter::object_bool(object, "wallhopswitch") {
        instance.left_flick = value;
    }
    if let Some(value) = crate::profile_adapter::object_bool(object, "toggle_jump") {
        instance.jump = value;
    }
    if let Some(value) = crate::profile_adapter::object_bool(object, "toggle_flick") {
        instance.flick_back = value;
    }
    if let Some(value) = crate::profile_adapter::object_bool(object, "wallhopcamfix") {
        instance.wallhop_cam_fix = value;
    }
    instance.disable_outside_target =
        crate::profile_adapter::object_bool(object, "disable_outside_roblox")
            .unwrap_or(base_disable_outside);
    instance.enabled =
        crate::profile_adapter::object_bool(object, "section_enabled").unwrap_or(false);
    instance
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

    pub(crate) fn set_base_section_flags(&mut self, enabled: bool, disable_outside: bool) {
        if let Some(instance) = self.instances.instances_mut().first_mut() {
            instance.enabled = enabled;
            instance.disable_outside_target = disable_outside;
        }
    }

    #[must_use]
    pub fn first_enabled(&self) -> bool {
        self.instances().first().is_some_and(|instance| instance.enabled)
    }

    #[must_use]
    pub fn enabled_flags(&self) -> Vec<bool> {
        self.instances().iter().map(|instance| instance.enabled).collect()
    }

    pub(crate) fn reset_base_to_profile_defaults(&mut self, defaults: &PreservedProfilePayload) {
        let Some(base) = self.instances.instances_mut().first_mut() else {
            return;
        };
        let enabled = base.enabled;
        let disable_outside_target = base.disable_outside_target;
        let mut default_state = Self::default();
        default_state.apply_profile(defaults);
        let Some(default_base) = default_state.instances().first().cloned() else {
            return;
        };
        *base = default_base;
        base.enabled = enabled;
        base.disable_outside_target = disable_outside_target;
    }

    pub(crate) fn apply_profile(&mut self, payload: &PreservedProfilePayload) {
        let base = wallhop_base_from_profile(payload);
        let base_disable_outside = base.disable_outside_target;
        let mut instances = vec![base];
        if let Some(extras) = payload.raw_value("extra_wallhop_instances").and_then(Value::as_array)
        {
            instances.extend(
                extras
                    .iter()
                    .filter_map(Value::as_object)
                    .map(|object| wallhop_extra_from_profile(object, base_disable_outside)),
            );
        }
        let selected = crate::profile_adapter::i32_value(payload, "selected_wallhop_instance")
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(0);
        self.instances.replace_instances(instances, selected);
    }

    pub(crate) fn overlay_profile(&self, overlay: &mut Map<String, Value>) {
        let Some(base) = self.instances().first() else {
            return;
        };
        crate::profile_adapter::insert_u32(overlay, "vk_xbutton2", base.trigger.raw());
        crate::profile_adapter::insert_u32(overlay, "vk_wallhopjumpkey", base.jump_key.raw());
        crate::profile_adapter::insert_i32(overlay, "wallhop_dx", base.dx);
        crate::profile_adapter::insert_i32(overlay, "wallhop_dy", base.dy);
        crate::profile_adapter::insert_i32(overlay, "wallhop_vertical", base.vertical);
        crate::profile_adapter::insert_string(overlay, "WallhopPixels", base.pixels_text.clone());
        crate::profile_adapter::insert_string(
            overlay,
            "WallhopVerticalChar",
            base.vertical_text.clone(),
        );
        crate::profile_adapter::insert_string(overlay, "WallhopDelayChar", base.delay_text.clone());
        crate::profile_adapter::insert_string(
            overlay,
            "WallhopBonusDelayChar",
            base.bonus_delay_text.clone(),
        );
        crate::profile_adapter::insert_bool(overlay, "wallhopswitch", base.left_flick);
        crate::profile_adapter::insert_bool(overlay, "toggle_jump", base.jump);
        crate::profile_adapter::insert_bool(overlay, "toggle_flick", base.flick_back);
        crate::profile_adapter::insert_bool(overlay, "wallhopcamfix", base.wallhop_cam_fix);
        crate::profile_adapter::insert_i32(
            overlay,
            "selected_wallhop_instance",
            i32::try_from(self.selected_index()).unwrap_or_default(),
        );
        let extras: Vec<Value> = self
            .instances()
            .iter()
            .skip(1)
            .map(|instance| {
                json!({
                    "vk_trigger": instance.trigger.raw(),
                    "vk_jumpkey": instance.jump_key.raw(),
                    "wallhop_dx": instance.dx,
                    "wallhop_dy": instance.dy,
                    "wallhop_vertical": instance.vertical,
                    "WallhopDelay": instance.delay_ms,
                    "WallhopBonusDelay": instance.bonus_delay_ms,
                    "WallhopPixels": instance.pixels_text,
                    "WallhopVerticalChar": instance.vertical_text,
                    "WallhopDelayChar": instance.delay_text,
                    "WallhopBonusDelayChar": instance.bonus_delay_text,
                    "WallhopDegrees": instance.degrees_text,
                    "wallhopswitch": instance.left_flick,
                    "toggle_jump": instance.jump,
                    "toggle_flick": instance.flick_back,
                    "wallhopcamfix": instance.wallhop_cam_fix,
                    "disable_outside_roblox": instance.disable_outside_target,
                    "section_enabled": instance.enabled
                })
            })
            .collect();
        if !extras.is_empty() {
            overlay.insert("extra_wallhop_instances".to_owned(), Value::Array(extras));
        }
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
pub fn render(
    ui: &Ui,
    state: &mut WallhopUiState,
    sensitivity: &str,
    cam_fix: bool,
    capture: &mut crate::keybind::CaptureState,
    theme: &ThemeDefinition,
) {
    let count = state.instances.instances().len();
    let selected = state.instances.selected_index();
    let sid = format!("_wh{selected}");
    render_instance_header(ui, state, count, selected, capture);
    {
        let instance = state.instances.selected_mut();
        render_instance_enablement(ui, instance, &sid, theme);
    }
    render_instance_controls(ui, state, count, selected, &sid);
    let instance = state.instances.selected_mut();
    render_instance_motion(ui, instance, sensitivity, cam_fix, &sid);
    render_instance_options(ui, instance, &sid, capture);

    crate::parity::separator(ui);
    ui.text_wrapped("Explanation:");
    ui.new_line();
    ui.text_wrapped("This macro automates the camera flick, optional jump, and flick-back timing used for wallhops. Each instance has independent settings and a trigger key.");
}

fn render_instance_header(
    ui: &Ui,
    state: &mut WallhopUiState,
    count: usize,
    selected: usize,
    capture: &mut crate::keybind::CaptureState,
) {
    if count > 1 {
        ui.text_wrapped(format!("Settings for Wallhop/Rotation {}", selected + 1));
    } else {
        ui.text_wrapped("Settings for Wallhop/Rotation");
    }
    crate::parity::separator(ui);
    ui.new_line();
    ui.text_wrapped("Keybind:");
    ui.same_line();
    crate::keybind::render_hotkey_control(
        ui,
        capture,
        &format!("SectionKey6_wh{selected}"),
        &mut state.instances.selected_mut().trigger,
        170.0,
        130.0,
        false,
    );
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

fn render_instance_enablement(
    ui: &Ui,
    instance: &mut WallhopUiInstance,
    sid: &str,
    theme: &ThemeDefinition,
) {
    let color = ui.push_style_color(
        StyleColor::Text,
        if instance.enabled { theme.success_color } else { theme.error_color },
    );
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

fn render_instance_options(
    ui: &Ui,
    instance: &mut WallhopUiInstance,
    sid: &str,
    capture: &mut crate::keybind::CaptureState,
) {
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
        crate::keybind::render_hotkey_control(
            ui,
            capture,
            &format!("WallhopJumpKey{sid}"),
            &mut instance.jump_key,
            150.0,
            50.0,
            false,
        );
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

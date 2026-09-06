use crate::theme::ThemeDefinition;
use dear_imgui_rs::{MouseCursor, StyleColor, Ui};
use serde_json::{Map, Value, json};
use smu_profile::payload::PreservedProfilePayload;

const OUTDATED_FROM_LEFT: f32 = 135.0;
const DISCLAIMER_FROM_RIGHT: f32 = 795.0;
const OFFICIAL_SOURCE_FROM_RIGHT: f32 = 790.0;
const OFFICIAL_URL_FROM_RIGHT: f32 = 499.0;
const ANTI_AFK_FROM_RIGHT: f32 = 360.0;
const VERSION_FROM_RIGHT: f32 = 130.0;
const SETTINGS_FROM_RIGHT: f32 = 107.0;
const THEME_FROM_RIGHT: f32 = 257.0;
pub const OFFICIAL_SOURCE_URL: &str = "https://github.com/Spencer0187/Spencer-Macro-Utilities";
const ANTI_AFK_TOOLTIP: &str = "Anti-AFK is currently available on Windows only.\nAnti-AFK functions by counting up a timer constantly. If you are tabbed into Roblox\nand you press any key on your keyboard, the timer resets.\nIf the timer expires, it presses your configured Anti-AFK key directly.\nIf Roblox is not foreground, it briefly focuses the window first.";

const PROCESS_INPUT_WIDTH: f32 = 250.0;
const PROCESS_RESET_WIDTH: f32 = 25.0;
const SENSITIVITY_INPUT_WIDTH: f32 = 70.0;
const FPS_INPUT_WIDTH: f32 = 40.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GlobalSettingsAction {
    ToggleSettings,
    ToggleThemeEditor,
    OpenOfficialSource,
}

#[derive(Debug)]
struct GlobalToggles {
    macro_enabled: bool,
    anti_afk_enabled: bool,
    cam_fix_enabled: bool,
}

#[derive(Debug)]
struct ProcessUiState {
    name: String,
    found: bool,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
pub struct GlobalSettingsState {
    toggles: GlobalToggles,
    process: ProcessUiState,
    sensitivity: String,
    fps: String,
    fps_runtime: u32,
    speed_calibration_revision: u64,
    wallhop_sensitivity_revision: u64,
    shift_key: u32,
    chat_key: u32,
    enter_key: u32,
    chat_override: bool,
    paste_delay_ms: i32,
    paste_delay_text: String,
    display_scale: i32,
    use_old_paste: bool,
    afk_key: u32,
    anti_afk_minutes: i32,
    anti_afk_minutes_text: String,
    double_press_afk_key: bool,
    short_descriptions: bool,
    global_zoom: bool,
    zoom_reverse: bool,
}

impl GlobalSettingsState {
    #[must_use]
    pub const fn macro_enabled(&self) -> bool {
        self.toggles.macro_enabled
    }

    #[must_use]
    pub fn process_selector(&self) -> String {
        self.process.name.clone()
    }

    #[must_use]
    pub const fn fps_runtime(&self) -> u32 {
        self.fps_runtime
    }

    #[must_use]
    pub fn sensitivity(&self) -> &str {
        &self.sensitivity
    }

    #[must_use]
    pub const fn cam_fix_enabled(&self) -> bool {
        self.toggles.cam_fix_enabled
    }

    #[must_use]
    pub const fn speed_calibration_revision(&self) -> u64 {
        self.speed_calibration_revision
    }

    #[must_use]
    pub const fn wallhop_sensitivity_revision(&self) -> u64 {
        self.wallhop_sensitivity_revision
    }

    #[must_use]
    pub const fn shift_key(&self) -> u32 {
        self.shift_key
    }

    #[must_use]
    pub const fn chat_key(&self) -> u32 {
        self.chat_key
    }

    #[must_use]
    pub const fn enter_key(&self) -> u32 {
        self.enter_key
    }

    pub const fn enter_key_mut(&mut self) -> &mut u32 {
        &mut self.enter_key
    }

    #[must_use]
    pub const fn chat_override(&self) -> bool {
        self.chat_override
    }

    #[must_use]
    pub const fn paste_delay_ms(&self) -> i32 {
        self.paste_delay_ms
    }

    pub const fn set_process_found(&mut self, found: bool) {
        self.process.found = found;
    }

    pub fn reset_process_selector(&mut self) {
        self.process.name = if cfg!(target_os = "linux") {
            "sober".to_owned()
        } else {
            "RobloxPlayerBeta.exe".to_owned()
        };
    }

    pub const fn shift_key_mut(&mut self) -> &mut u32 {
        &mut self.shift_key
    }

    pub const fn chat_key_mut(&mut self) -> &mut u32 {
        &mut self.chat_key
    }

    pub const fn afk_key_mut(&mut self) -> &mut u32 {
        &mut self.afk_key
    }

    pub const fn chat_override_mut(&mut self) -> &mut bool {
        &mut self.chat_override
    }

    pub const fn use_old_paste_mut(&mut self) -> &mut bool {
        &mut self.use_old_paste
    }

    pub const fn global_zoom_mut(&mut self) -> &mut bool {
        &mut self.global_zoom
    }

    pub const fn zoom_reverse_mut(&mut self) -> &mut bool {
        &mut self.zoom_reverse
    }

    pub const fn double_press_afk_key_mut(&mut self) -> &mut bool {
        &mut self.double_press_afk_key
    }

    pub const fn short_descriptions_mut(&mut self) -> &mut bool {
        &mut self.short_descriptions
    }

    #[must_use]
    pub const fn short_descriptions(&self) -> bool {
        self.short_descriptions
    }

    pub fn apply_metadata(&mut self, metadata: &Map<String, Value>) {
        if let Some(value) = metadata.get("shortdescriptions").and_then(Value::as_bool) {
            self.short_descriptions = value;
        }
    }

    #[must_use]
    pub fn metadata_overlay(&self) -> Map<String, Value> {
        Map::from_iter([("shortdescriptions".to_owned(), json!(self.short_descriptions))])
    }

    pub const fn display_scale_mut(&mut self) -> &mut i32 {
        &mut self.display_scale
    }

    pub const fn paste_delay_parts(&mut self) -> (&mut String, &mut i32) {
        (&mut self.paste_delay_text, &mut self.paste_delay_ms)
    }

    pub const fn anti_afk_minutes_parts(&mut self) -> (&mut String, &mut i32) {
        (&mut self.anti_afk_minutes_text, &mut self.anti_afk_minutes)
    }

    #[must_use]
    pub const fn global_zoom(&self) -> bool {
        self.global_zoom
    }

    #[must_use]
    pub const fn zoom_reverse(&self) -> bool {
        self.zoom_reverse
    }

    pub(crate) fn apply_profile(&mut self, payload: &PreservedProfilePayload) {
        if let Some(value) = crate::profile_adapter::bool_value(payload, "macrotoggled") {
            self.toggles.macro_enabled = value;
        }
        if let Some(value) = crate::profile_adapter::bool_value(payload, "camfixtoggle") {
            self.toggles.cam_fix_enabled = value;
        }
        if let Some(value) = crate::profile_adapter::bool_value(payload, "antiafktoggle") {
            self.toggles.anti_afk_enabled = value;
        }
        if let Some(value) = crate::profile_adapter::string_value(payload, "settingsBuffer") {
            value.clone_into(&mut self.process.name);
        }
        if let Some(value) = crate::profile_adapter::string_value(payload, "RobloxSensValue") {
            value.clone_into(&mut self.sensitivity);
            self.speed_calibration_revision = self.speed_calibration_revision.wrapping_add(1);
        }
        if let Some(value) = crate::profile_adapter::string_value(payload, "RobloxFPSChar") {
            value.clone_into(&mut self.fps);
            if let Ok(parsed) = value.parse::<u32>() {
                self.fps_runtime = parsed;
            }
        }
        if let Some(value) = crate::profile_adapter::u32_value(payload, "vk_shiftkey") {
            self.shift_key = value;
        }
        if let Some(value) = crate::profile_adapter::u32_value(payload, "vk_enterkey") {
            self.enter_key = value;
        }
        if let Some(value) = crate::profile_adapter::u32_value(payload, "vk_chatkey") {
            self.chat_key = value;
        }
        if let Some(value) = crate::profile_adapter::u32_value(payload, "vk_afkkey") {
            self.afk_key = value;
        }
        if let Some(value) = crate::profile_adapter::bool_value(payload, "chatoverride") {
            self.chat_override = value;
        }
        if let Some(value) = crate::profile_adapter::bool_value(payload, "useoldpaste") {
            self.use_old_paste = value;
        }
        if let Some(value) = crate::profile_adapter::bool_value(payload, "doublepressafkkey") {
            self.double_press_afk_key = value;
        }
        if let Some(value) = crate::profile_adapter::bool_value(payload, "globalzoomin") {
            self.global_zoom = value;
        }
        if let Some(value) = crate::profile_adapter::bool_value(payload, "globalzoominreverse") {
            self.zoom_reverse = value;
        }
        if let Some(value) = crate::profile_adapter::i32_value(payload, "display_scale") {
            self.display_scale = value;
        }
        if let Some(value) = crate::profile_adapter::string_value(payload, "PasteDelayChar") {
            value.clone_into(&mut self.paste_delay_text);
            if let Ok(parsed) = value.parse::<i32>() {
                self.paste_delay_ms = parsed;
            }
        }
        if let Some(value) = crate::profile_adapter::string_value(payload, "AntiAFKTimeChar") {
            value.clone_into(&mut self.anti_afk_minutes_text);
            if let Ok(parsed) = value.parse::<i32>() {
                self.anti_afk_minutes = parsed;
            }
        }
    }

    pub(crate) fn overlay_profile(&self, overlay: &mut Map<String, Value>) {
        crate::profile_adapter::insert_bool(overlay, "macrotoggled", self.toggles.macro_enabled);
        crate::profile_adapter::insert_bool(overlay, "camfixtoggle", self.toggles.cam_fix_enabled);
        crate::profile_adapter::insert_bool(
            overlay,
            "antiafktoggle",
            self.toggles.anti_afk_enabled,
        );
        crate::profile_adapter::insert_string(overlay, "settingsBuffer", self.process.name.clone());
        crate::profile_adapter::insert_string(overlay, "RobloxSensValue", self.sensitivity.clone());
        crate::profile_adapter::insert_string(overlay, "RobloxFPSChar", self.fps.clone());
        crate::profile_adapter::insert_u32(overlay, "vk_shiftkey", self.shift_key);
        crate::profile_adapter::insert_u32(overlay, "vk_enterkey", self.enter_key);
        crate::profile_adapter::insert_u32(overlay, "vk_chatkey", self.chat_key);
        crate::profile_adapter::insert_u32(overlay, "vk_afkkey", self.afk_key);
        crate::profile_adapter::insert_bool(overlay, "chatoverride", self.chat_override);
        crate::profile_adapter::insert_bool(overlay, "useoldpaste", self.use_old_paste);
        crate::profile_adapter::insert_bool(
            overlay,
            "doublepressafkkey",
            self.double_press_afk_key,
        );
        crate::profile_adapter::insert_bool(overlay, "globalzoomin", self.global_zoom);
        crate::profile_adapter::insert_bool(overlay, "globalzoominreverse", self.zoom_reverse);
        crate::profile_adapter::insert_i32(overlay, "display_scale", self.display_scale);
        crate::profile_adapter::insert_string(
            overlay,
            "PasteDelayChar",
            self.paste_delay_text.clone(),
        );
        crate::profile_adapter::insert_i32(overlay, "AntiAFKTime", self.anti_afk_minutes);
        crate::profile_adapter::insert_string(
            overlay,
            "AntiAFKTimeChar",
            self.anti_afk_minutes_text.clone(),
        );
    }

    pub fn apply_lag_switch_reference_globals(&mut self) {
        "0.6".clone_into(&mut self.sensitivity);
        "200".clone_into(&mut self.fps);
        self.toggles.cam_fix_enabled = false;
        // The C++ parity hook writes the display buffer directly and intentionally does not call
        // SyncRobloxFPSFromBuffer(), so the runtime atomic remains at its production initial 120.
        self.fps_runtime = 120;
    }
}

impl Default for GlobalSettingsState {
    fn default() -> Self {
        Self {
            toggles: GlobalToggles {
                macro_enabled: true,
                anti_afk_enabled: false,
                cam_fix_enabled: false,
            },
            process: ProcessUiState {
                name: if cfg!(target_os = "linux") {
                    "sober".to_owned()
                } else {
                    "RobloxPlayerBeta.exe".to_owned()
                },
                found: false,
            },
            sensitivity: "0.5".to_owned(),
            fps: "60".to_owned(),
            fps_runtime: 120,
            speed_calibration_revision: 1,
            wallhop_sensitivity_revision: 1,
            shift_key: 0x10,
            chat_key: 0xBF,
            enter_key: 0x0D,
            chat_override: true,
            paste_delay_ms: 1,
            paste_delay_text: "1".to_owned(),
            display_scale: 100,
            use_old_paste: true,
            afk_key: 0xDC,
            anti_afk_minutes: 15,
            anti_afk_minutes_text: "15".to_owned(),
            double_press_afk_key: true,
            short_descriptions: false,
            global_zoom: false,
            zoom_reverse: false,
        }
    }
}

#[allow(clippy::suboptimal_flops)] // Preserve the C++ oracle's absolute-placement arithmetic.
pub fn render(
    ui: &Ui,
    state: &mut GlobalSettingsState,
    user_outdated: bool,
    settings_open: bool,
    theme_editor_open: bool,
    theme: &ThemeDefinition,
) -> Option<GlobalSettingsAction> {
    let window_width = ui.window_size()[0];
    ui.align_text_to_frame_padding();
    ui.text_wrapped("Global Settings");
    if user_outdated {
        ui.same_line_with_pos(OUTDATED_FROM_LEFT);
        ui.text_colored(theme.error_color, "(OUTDATED VERSION)");
    }

    ui.same_line_with_pos(window_width - DISCLAIMER_FROM_RIGHT);
    ui.text_wrapped("DISCLAIMER: THIS IS NOT A CHEAT, IT NEVER INTERACTS WITH ROBLOX MEMORY.");

    let macro_color =
        if state.toggles.macro_enabled { theme.success_color } else { theme.error_color };
    let macro_text = ui.push_style_color(StyleColor::Text, macro_color);
    ui.align_text_to_frame_padding();
    #[cfg(target_os = "windows")]
    ui.checkbox("Macro Toggle (Anti-AFK remains!)", &mut state.toggles.macro_enabled);
    #[cfg(not(target_os = "windows"))]
    ui.checkbox("Macro Toggle", &mut state.toggles.macro_enabled);
    drop(macro_text);

    let action = render_official_source_link(ui, window_width, theme);

    ui.align_text_to_frame_padding();
    #[cfg(target_os = "linux")]
    ui.text_wrapped("Roblox Executable Name/PIDs (Space Separated):");
    #[cfg(not(target_os = "linux"))]
    ui.text_wrapped("Roblox Executable Name:");
    ui.same_line();
    ui.set_next_item_width(PROCESS_INPUT_WIDTH);
    if ui.input_text("##SettingsTextbox", &mut state.process.name).build() {
        state.process.name = state.process.name.trim().to_owned();
    }
    ui.same_line();
    if ui.button_with_size("R", [PROCESS_RESET_WIDTH, 0.0]) {
        state.reset_process_selector();
    }
    ui.same_line();

    let position = ui.cursor_screen_pos();
    let center = [position[0] + 5.0, position[1] + ui.text_line_height() / 2.0 + 3.0];
    ui.get_window_draw_list()
        .add_circle(
            center,
            5.0,
            if state.process.found { theme.success_color } else { theme.error_color },
        )
        .filled(true)
        .build();
    ui.dummy([10.0, 10.0]);
    if !state.process.found {
        #[cfg(not(target_os = "linux"))]
        {
            ui.same_line();
            ui.set_cursor_pos_y(ui.cursor_pos_y() - 1.0);
            ui.text_wrapped("Roblox Not Found");
        }
    }

    render_anti_afk_toggle(ui, window_width, &mut state.toggles.anti_afk_enabled, theme);

    ui.same_line_with_pos(window_width - VERSION_FROM_RIGHT);
    ui.text("VERSION 3.4.0");

    ui.align_text_to_frame_padding();
    ui.text_wrapped("Roblox Sensitivity (0-4):");
    ui.same_line();
    ui.set_next_item_width(SENSITIVITY_INPUT_WIDTH);
    if ui
        .input_text("##Sens", &mut state.sensitivity)
        .chars_decimal(true)
        .chars_no_blank(true)
        .build()
    {
        state.speed_calibration_revision = state.speed_calibration_revision.wrapping_add(1);
        state.wallhop_sensitivity_revision = state.wallhop_sensitivity_revision.wrapping_add(1);
    }
    ui.same_line();
    ui.text_wrapped("Your Roblox FPS:");
    ui.same_line();
    ui.set_next_item_width(FPS_INPUT_WIDTH);
    if ui.input_text("##FPS", &mut state.fps).chars_decimal(true).chars_no_blank(true).build()
        && let Some(parsed) = parse_cpp_fps_buffer(&state.fps)
    {
        state.fps_runtime = parsed;
    }
    ui.same_line();
    ui.text("Game Uses Cam-Fix:");
    ui.same_line();
    if ui.checkbox("##CamfixToggle", &mut state.toggles.cam_fix_enabled) {
        state.speed_calibration_revision = state.speed_calibration_revision.wrapping_add(1);
    }

    render_global_window_buttons(ui, window_width, settings_open, theme_editor_open).or(action)
}

fn render_official_source_link(
    ui: &Ui,
    window_width: f32,
    theme: &ThemeDefinition,
) -> Option<GlobalSettingsAction> {
    ui.same_line_with_pos(window_width - OFFICIAL_SOURCE_FROM_RIGHT);
    ui.text_wrapped("The ONLY official source for this is");
    let link = ui.push_style_color(StyleColor::Text, theme.accent_secondary);
    ui.same_line_with_pos(window_width - OFFICIAL_URL_FROM_RIGHT);
    ui.text_wrapped(OFFICIAL_SOURCE_URL);
    let action = if ui.is_item_hovered() {
        ui.set_mouse_cursor(Some(MouseCursor::Hand));
        ui.is_item_clicked().then_some(GlobalSettingsAction::OpenOfficialSource)
    } else {
        None
    };
    drop(link);
    action
}

fn render_anti_afk_toggle(ui: &Ui, window_width: f32, enabled: &mut bool, theme: &ThemeDefinition) {
    ui.same_line_with_pos(window_width - ANTI_AFK_FROM_RIGHT);
    let tooltip_start = ui.cursor_screen_pos();
    ui.text("Toggle Anti-AFK (");
    ui.same_line_with_spacing(0.0, 0.0);
    ui.text_colored(theme.accent_primary, "?");
    ui.same_line_with_spacing(0.0, 0.0);
    ui.text("):");
    let tooltip_size = ui.calc_text_size("Toggle Anti-AFK (?)");
    if ui.is_mouse_hovering_rect(
        tooltip_start,
        [tooltip_start[0] + tooltip_size[0], tooltip_start[1] + tooltip_size[1]],
    ) {
        ui.set_tooltip(ANTI_AFK_TOOLTIP);
    }
    ui.same_line();
    normalize_anti_afk_for_platform(enabled);
    #[cfg(not(target_os = "windows"))]
    let disabled = ui.begin_disabled();
    ui.checkbox("##AntiAFKToggle", enabled);
    #[cfg(not(target_os = "windows"))]
    drop(disabled);
}

const fn normalize_anti_afk_for_platform(enabled: &mut bool) {
    #[cfg(not(target_os = "windows"))]
    {
        *enabled = false;
    }
    #[cfg(target_os = "windows")]
    let _ = enabled;
}

fn render_global_window_buttons(
    ui: &Ui,
    window_width: f32,
    settings_open: bool,
    theme_editor_open: bool,
) -> Option<GlobalSettingsAction> {
    ui.same_line_with_pos(window_width - SETTINGS_FROM_RIGHT);
    if ui.button(settings_button_label(settings_open)) {
        return Some(GlobalSettingsAction::ToggleSettings);
    }
    ui.same_line_with_pos(window_width - THEME_FROM_RIGHT);
    ui.button(theme_editor_button_label(theme_editor_open))
        .then_some(GlobalSettingsAction::ToggleThemeEditor)
}

const fn settings_button_label(open: bool) -> &'static str {
    if open { "Settings <-" } else { "Settings ->" }
}

const fn theme_editor_button_label(open: bool) -> &'static str {
    if open { "Theme Editor <-" } else { "Theme Editor ->" }
}

fn parse_cpp_fps_buffer(value: &str) -> Option<u32> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let mut parsed = 0_u32;
    for digit in value.bytes().map(|byte| u32::from(byte - b'0')) {
        parsed = parsed.checked_mul(10)?.checked_add(digit)?;
    }
    Some(parsed)
}

#[cfg(test)]
mod tests {
    use super::{
        ANTI_AFK_TOOLTIP, GlobalSettingsState, OFFICIAL_SOURCE_URL,
        normalize_anti_afk_for_platform, parse_cpp_fps_buffer, settings_button_label,
        theme_editor_button_label,
    };
    use serde_json::{Map, json};

    #[test]
    fn production_defaults_keep_cpp_display_and_runtime_fps_split() {
        let state = GlobalSettingsState::default();
        assert_eq!(state.sensitivity(), "0.5");
        assert_eq!(state.fps, "60");
        assert_eq!(state.fps_runtime(), 120);
        assert!(!state.cam_fix_enabled());
        assert_eq!(state.shift_key(), 0x10);
        assert_eq!(state.chat_key(), 0xBF);
        assert_eq!(state.enter_key(), 0x0D);
        assert!(state.chat_override());
        assert_eq!(state.paste_delay_ms(), 1);
        assert_eq!(state.display_scale, 100);
        assert!(state.use_old_paste);
        assert_eq!(state.afk_key, 0xDC);
        assert_eq!(state.anti_afk_minutes, 15);
        assert!(state.double_press_afk_key);
        assert!(!state.short_descriptions);
        assert!(!state.global_zoom());
        assert!(!state.zoom_reverse());
    }

    #[test]
    fn short_descriptions_metadata_matches_cpp_bool_load_and_save() {
        let mut state = GlobalSettingsState::default();
        state.apply_metadata(&Map::from_iter([("shortdescriptions".to_owned(), json!(true))]));
        assert!(state.short_descriptions());
        assert_eq!(state.metadata_overlay()["shortdescriptions"], json!(true));

        state.apply_metadata(&Map::from_iter([(
            "shortdescriptions".to_owned(),
            json!("not-a-bool"),
        )]));
        assert!(state.short_descriptions());
    }

    #[test]
    fn cpp_fps_buffer_parser_rejects_empty_nondigit_and_overflow() {
        assert_eq!(parse_cpp_fps_buffer("0"), Some(0));
        assert_eq!(parse_cpp_fps_buffer("200"), Some(200));
        assert_eq!(parse_cpp_fps_buffer(""), None);
        assert_eq!(parse_cpp_fps_buffer("20x"), None);
        assert_eq!(parse_cpp_fps_buffer("4294967296"), None);
    }

    #[test]
    fn parity_globals_change_text_without_syncing_runtime_fps() {
        let mut state = GlobalSettingsState::default();
        state.apply_lag_switch_reference_globals();
        assert_eq!(state.sensitivity(), "0.6");
        assert_eq!(state.fps, "200");
        assert_eq!(state.fps_runtime(), 120);
    }

    #[test]
    fn profile_load_does_not_count_as_wallhop_sensitivity_edit() {
        let mut state = GlobalSettingsState::default();
        let revision = state.wallhop_sensitivity_revision();
        state.apply_profile(&smu_profile::payload::PreservedProfilePayload::cpp_default());
        assert_eq!(state.wallhop_sensitivity_revision(), revision);
    }
    #[test]
    fn official_source_url_matches_cpp_global_header_link() {
        assert_eq!(OFFICIAL_SOURCE_URL, "https://github.com/Spencer0187/Spencer-Macro-Utilities");
    }

    #[test]
    fn anti_afk_tooltip_matches_cpp_copy() {
        assert_eq!(
            ANTI_AFK_TOOLTIP,
            "Anti-AFK is currently available on Windows only.\nAnti-AFK functions by counting up a timer constantly. If you are tabbed into Roblox\nand you press any key on your keyboard, the timer resets.\nIf the timer expires, it presses your configured Anti-AFK key directly.\nIf Roblox is not foreground, it briefly focuses the window first."
        );
    }
    #[test]
    fn global_window_button_arrows_match_cpp_open_state() {
        assert_eq!(settings_button_label(false), "Settings ->");
        assert_eq!(settings_button_label(true), "Settings <-");
        assert_eq!(theme_editor_button_label(false), "Theme Editor ->");
        assert_eq!(theme_editor_button_label(true), "Theme Editor <-");
    }
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn non_windows_global_header_forces_anti_afk_off_like_cpp() {
        let mut enabled = true;
        normalize_anti_afk_for_platform(&mut enabled);
        assert!(!enabled);
    }
}

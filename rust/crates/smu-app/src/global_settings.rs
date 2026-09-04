use dear_imgui_rs::{StyleColor, Ui};

const DISCLAIMER_FROM_RIGHT: f32 = 795.0;
const OFFICIAL_SOURCE_FROM_RIGHT: f32 = 790.0;
const OFFICIAL_URL_FROM_RIGHT: f32 = 499.0;
const ANTI_AFK_FROM_RIGHT: f32 = 360.0;
const VERSION_FROM_RIGHT: f32 = 130.0;
const SETTINGS_FROM_RIGHT: f32 = 107.0;
const THEME_FROM_RIGHT: f32 = 257.0;
const PROCESS_INPUT_WIDTH: f32 = 250.0;
const PROCESS_RESET_WIDTH: f32 = 25.0;
const SENSITIVITY_INPUT_WIDTH: f32 = 70.0;
const FPS_INPUT_WIDTH: f32 = 40.0;

const ACCENT_PRIMARY: [f32; 4] = [0.0, 0.49, 0.62, 1.0];
const ACCENT_SECONDARY: [f32; 4] = [0.0, 0.44, 0.60, 1.0];
const SUCCESS: [f32; 4] = [0.0, 1.0, 0.50, 1.0];
const ERROR: [f32; 4] = [1.0, 0.20, 0.40, 1.0];

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

#[derive(Debug)]
pub struct GlobalSettingsState {
    toggles: GlobalToggles,
    process: ProcessUiState,
    sensitivity: String,
    fps: String,
    fps_runtime: u32,
    speed_calibration_revision: u64,
    shift_key: u32,
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
    pub const fn shift_key(&self) -> u32 {
        self.shift_key
    }

    #[must_use]
    pub const fn global_zoom(&self) -> bool {
        self.global_zoom
    }

    #[must_use]
    pub const fn zoom_reverse(&self) -> bool {
        self.zoom_reverse
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
            shift_key: 0x10,
            global_zoom: false,
            zoom_reverse: false,
        }
    }
}

#[allow(clippy::suboptimal_flops)] // Preserve the C++ oracle's absolute-placement arithmetic.
pub fn render(ui: &Ui, state: &mut GlobalSettingsState) {
    let window_width = ui.window_size()[0];

    ui.align_text_to_frame_padding();
    ui.text_wrapped("Global Settings");

    ui.same_line_with_pos(window_width - DISCLAIMER_FROM_RIGHT);
    ui.text_wrapped("DISCLAIMER: THIS IS NOT A CHEAT, IT NEVER INTERACTS WITH ROBLOX MEMORY.");

    let macro_color = if state.toggles.macro_enabled { SUCCESS } else { ERROR };
    let macro_text = ui.push_style_color(StyleColor::Text, macro_color);
    ui.align_text_to_frame_padding();
    #[cfg(target_os = "windows")]
    ui.checkbox("Macro Toggle (Anti-AFK remains!)", &mut state.toggles.macro_enabled);
    #[cfg(not(target_os = "windows"))]
    ui.checkbox("Macro Toggle", &mut state.toggles.macro_enabled);
    drop(macro_text);

    ui.same_line_with_pos(window_width - OFFICIAL_SOURCE_FROM_RIGHT);
    ui.text_wrapped("The ONLY official source for this is");
    let link = ui.push_style_color(StyleColor::Text, ACCENT_SECONDARY);
    ui.same_line_with_pos(window_width - OFFICIAL_URL_FROM_RIGHT);
    ui.text_wrapped("https://github.com/Spencer0187/Spencer-Macro-Utilities");
    drop(link);

    ui.align_text_to_frame_padding();
    #[cfg(target_os = "linux")]
    ui.text_wrapped("Roblox Executable Name/PIDs (Space Separated):");
    #[cfg(not(target_os = "linux"))]
    ui.text_wrapped("Roblox Executable Name:");
    ui.same_line();
    ui.set_next_item_width(PROCESS_INPUT_WIDTH);
    ui.input_text("##SettingsTextbox", &mut state.process.name).build();
    ui.same_line();
    let _ = ui.button_with_size("R", [PROCESS_RESET_WIDTH, 0.0]);
    ui.same_line();

    let position = ui.cursor_screen_pos();
    let center = [position[0] + 5.0, position[1] + ui.text_line_height() / 2.0 + 3.0];
    ui.get_window_draw_list()
        .add_circle(center, 5.0, if state.process.found { SUCCESS } else { ERROR })
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

    ui.same_line_with_pos(window_width - ANTI_AFK_FROM_RIGHT);
    ui.text("Toggle Anti-AFK (");
    ui.same_line_with_spacing(0.0, 0.0);
    ui.text_colored(ACCENT_PRIMARY, "?");
    ui.same_line_with_spacing(0.0, 0.0);
    ui.text("):");
    ui.same_line();
    #[cfg(not(target_os = "windows"))]
    let anti_afk_disabled = ui.begin_disabled();
    ui.checkbox("##AntiAFKToggle", &mut state.toggles.anti_afk_enabled);
    #[cfg(not(target_os = "windows"))]
    drop(anti_afk_disabled);

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

    ui.same_line_with_pos(window_width - SETTINGS_FROM_RIGHT);
    let _ = ui.button("Settings ->");
    ui.same_line_with_pos(window_width - THEME_FROM_RIGHT);
    let _ = ui.button("Theme Editor ->");
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
    use super::{GlobalSettingsState, parse_cpp_fps_buffer};

    #[test]
    fn production_defaults_keep_cpp_display_and_runtime_fps_split() {
        let state = GlobalSettingsState::default();
        assert_eq!(state.sensitivity(), "0.5");
        assert_eq!(state.fps, "60");
        assert_eq!(state.fps_runtime(), 120);
        assert!(!state.cam_fix_enabled());
        assert_eq!(state.shift_key(), 0x10);
        assert!(!state.global_zoom());
        assert!(!state.zoom_reverse());
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
}

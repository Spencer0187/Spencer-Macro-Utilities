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
            sensitivity: "0.6".to_owned(),
            fps: "200".to_owned(),
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
    ui.input_text("##Sens", &mut state.sensitivity)
        .chars_decimal(true)
        .chars_no_blank(true)
        .build();
    ui.same_line();
    ui.text_wrapped("Your Roblox FPS:");
    ui.same_line();
    ui.set_next_item_width(FPS_INPUT_WIDTH);
    ui.input_text("##FPS", &mut state.fps).chars_decimal(true).chars_no_blank(true).build();
    ui.same_line();
    ui.text("Game Uses Cam-Fix:");
    ui.same_line();
    ui.checkbox("##CamfixToggle", &mut state.toggles.cam_fix_enabled);

    ui.same_line_with_pos(window_width - SETTINGS_FROM_RIGHT);
    let _ = ui.button("Settings ->");
    ui.same_line_with_pos(window_width - THEME_FROM_RIGHT);
    let _ = ui.button("Theme Editor ->");
}

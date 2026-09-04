use dear_imgui_rs::{Condition, StyleVar, Ui, WindowFlags};

use crate::{
    global_settings::{self, GlobalSettingsState},
    lag_switch::{self, LagSwitchAction, LagSwitchUiState},
    sidebar,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellAction {
    ToggleLagSwitchBackend,
    #[cfg(target_os = "linux")]
    StartLagSwitchHelper,
    #[cfg(target_os = "linux")]
    StartFreezeHelper,
    #[cfg(target_os = "linux")]
    CancelFreezeHelper,
}

const ROOT_WINDOW: &str = "Main SMU Window";
const GLOBAL_SETTINGS_CHILD: &str = "GlobalSettings";
const LEFT_SCROLL_CHILD: &str = "LeftScrollSection";
const RIGHT_SECTION_CHILD: &str = "RightSection";
const BOTTOM_CONTROLS_CHILD: &str = "BottomControls";

const SETTINGS_HORIZONTAL_MARGIN: f32 = 16.0;
const SETTINGS_TEXT_ROWS: f32 = 4.0;
const SETTINGS_PADDING_ROWS: f32 = 3.0;
const SETTINGS_EXTRA_HEIGHT: f32 = 12.0;
const LEFT_WIDTH_FRACTION: f32 = 0.30;
const MAIN_HORIZONTAL_ADJUSTMENT: f32 = 23.0;
const MAIN_VERTICAL_ADJUSTMENT: f32 = 20.0;
const FOOTER_HEIGHT: f32 = 30.0;
const FOOTER_BORDER_COMPENSATION: f32 = 1.0;
const FOOTER_CONTROLS_FROM_RIGHT: f32 = 602.0;
const FOOTER_CONTROL_Y_OFFSET: f32 = 3.0;
const OPACITY_SLIDER_WIDTH: f32 = 100.0;
const PROFILES_BUTTON_WIDTH: f32 = 125.0;

#[derive(Debug)]
pub struct ShellState {
    always_on_top: bool,
    opacity_percent: f32,
    global: GlobalSettingsState,
    lag_switch: LagSwitchUiState,
    #[cfg(target_os = "linux")]
    open_admin_popup: bool,
    #[cfg(target_os = "linux")]
    admin_error: String,
    #[cfg(target_os = "linux")]
    open_freeze_popup: bool,
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            always_on_top: false,
            opacity_percent: 100.0,
            global: GlobalSettingsState::default(),
            lag_switch: LagSwitchUiState::default(),
            #[cfg(target_os = "linux")]
            open_admin_popup: false,
            #[cfg(target_os = "linux")]
            admin_error: String::new(),
            #[cfg(target_os = "linux")]
            open_freeze_popup: false,
        }
    }
}

impl ShellState {
    #[must_use]
    pub fn for_parity_case(case: Option<&str>) -> Self {
        let mut state = Self::default();
        if case == Some("lag-switch-user-reference") {
            state.lag_switch = LagSwitchUiState::user_reference();
        }
        state
    }

    #[cfg(target_os = "linux")]
    pub const fn set_lag_switch_backend(&mut self, backend: crate::lag_switch::BackendState) {
        self.lag_switch.backend = backend;
    }

    #[cfg(target_os = "linux")]
    pub const fn request_lag_switch_helper_popup(&mut self) {
        self.open_admin_popup = true;
    }

    #[cfg(target_os = "linux")]
    pub fn set_lag_switch_helper_error(&mut self, error: impl Into<String>) {
        self.admin_error = error.into();
    }

    #[cfg(target_os = "linux")]
    pub const fn request_freeze_helper_popup(&mut self) {
        self.open_freeze_popup = true;
    }
}

pub fn render(ui: &Ui, state: &mut ShellState) -> Option<ShellAction> {
    let display_size = ui.io().display_size();
    let flags = WindowFlags::NO_RESIZE
        | WindowFlags::NO_MOVE
        | WindowFlags::NO_TITLE_BAR
        | WindowFlags::NO_SCROLLBAR
        | WindowFlags::NO_SCROLL_WITH_MOUSE
        | WindowFlags::NO_BRING_TO_FRONT_ON_FOCUS;

    let mut action = None;
    let _window_rounding = ui.push_style_var(StyleVar::WindowRounding(0.0));
    ui.window(ROOT_WINDOW)
        .size(display_size, Condition::Always)
        .position([0.0, 0.0], Condition::Always)
        .flags(flags)
        .build(|| {
            action = render_contents(ui, state, display_size);
        });

    #[cfg(target_os = "linux")]
    {
        if state.open_admin_popup {
            let _ = ui.open_popup("Administrator Required");
            state.open_admin_popup = false;
        }
        if let Some(_popup) = ui.begin_modal_popup("Administrator Required") {
            ui.text_wrapped("Linux lagswitch features require a privileged nethelper process.");
            ui.text_wrapped("This process involves:");
            ui.bullet_text("Starting the bundled nethelper with pkexec.");
            ui.bullet_text("Keeping the SMU GUI running as your normal desktop user.");
            if !state.admin_error.is_empty() {
                ui.text_colored([1.0, 0.20, 0.40, 1.0], &state.admin_error);
            }
            crate::parity::separator(ui);
            if ui.button_with_size("Start Helper", [180.0, 0.0]) {
                action = Some(ShellAction::StartLagSwitchHelper);
                ui.close_current_popup();
            }
            ui.same_line();
            if ui.button_with_size("Cancel", [120.0, 0.0]) {
                ui.close_current_popup();
            }
        }

        if state.open_freeze_popup {
            let _ = ui.open_popup("Additional Freeze Permission Required");
            state.open_freeze_popup = false;
        }
        if let Some(_popup) = ui.begin_modal_popup("Additional Freeze Permission Required") {
            ui.text_wrapped(
                "SMU cannot move Sober's game process into the temporary cgroup used by Freeze with your current Linux permissions.",
            );
            ui.new_line();
            ui.text_wrapped(
                "To keep Sober-compatible freezing working, SMU can start its bundled privileged Linux helper with pkexec.",
            );
            ui.text_wrapped("This grants the helper root access for this SMU session.");
            ui.new_line();
            ui.bullet_text("The SMU GUI remains running as your normal desktop user.");
            ui.bullet_text("The helper only accepts commands from this exact SMU process.");
            ui.bullet_text("The helper exits when SMU exits; nothing is installed permanently.");
            ui.bullet_text(
                "This helper is bundled with SMU and is dedicated to the Sober freeze workaround.",
            );
            crate::parity::separator(ui);
            if ui.button_with_size("Start Privileged Helper", [220.0, 0.0]) {
                action = Some(ShellAction::StartFreezeHelper);
                ui.close_current_popup();
            }
            ui.same_line();
            if ui.button_with_size("Cancel##FreezeHelper", [120.0, 0.0]) {
                action = Some(ShellAction::CancelFreezeHelper);
                ui.close_current_popup();
            }
        }
    }

    action
}

#[allow(clippy::suboptimal_flops)] // Preserve the C++ oracle's expression order for pixel parity.
fn render_contents(ui: &Ui, state: &mut ShellState, display_size: [f32; 2]) -> Option<ShellAction> {
    let style = ui.clone_style();
    let settings_panel_height = ui.text_line_height_with_spacing() * SETTINGS_TEXT_ROWS
        + style.window_padding()[1] * SETTINGS_PADDING_ROWS
        + SETTINGS_EXTRA_HEIGHT;

    ui.child_window(GLOBAL_SETTINGS_CHILD)
        .size([display_size[0] - SETTINGS_HORIZONTAL_MARGIN, settings_panel_height])
        .border(true)
        .build(ui, || global_settings::render(ui, &mut state.global));

    let left_panel_width = ui.window_size()[0] * LEFT_WIDTH_FRACTION - MAIN_HORIZONTAL_ADJUSTMENT;
    let left_panel_height = ui.window_size()[1] - settings_panel_height - MAIN_VERTICAL_ADJUSTMENT;
    ui.child_window(LEFT_SCROLL_CHILD)
        .size([left_panel_width, left_panel_height])
        .border(true)
        .build(ui, || sidebar::render(ui, left_panel_width));

    ui.same_line();
    let right_section_size = [
        display_size[0] - MAIN_HORIZONTAL_ADJUSTMENT - left_panel_width,
        display_size[1] - settings_panel_height - MAIN_VERTICAL_ADJUSTMENT - FOOTER_HEIGHT,
    ];
    let mut lag_action = None;
    ui.child_window(RIGHT_SECTION_CHILD).size(right_section_size).border(true).build(ui, || {
        lag_action = lag_switch::render(ui, &mut state.lag_switch);
    });

    let right_child_min = ui.item_rect_min();
    let right_child_size = ui.item_rect_size();
    let window_size = ui.window_size();
    ui.set_cursor_pos_y(window_size[1] - FOOTER_HEIGHT - style.window_padding()[1]);
    ui.set_cursor_pos_x(right_child_min[0]);

    let _square_footer = ui.push_style_var(StyleVar::ChildRounding(0.0));
    ui.child_window(BOTTOM_CONTROLS_CHILD)
        .size([right_child_size[0] - FOOTER_BORDER_COMPENSATION, FOOTER_HEIGHT])
        .flags(WindowFlags::NO_SCROLLBAR | WindowFlags::NO_SCROLL_WITH_MOUSE)
        .build(ui, || render_footer(ui, right_child_size[0], state));

    lag_action.map(|LagSwitchAction::ToggleBackend| ShellAction::ToggleLagSwitchBackend)
}

#[allow(clippy::suboptimal_flops)] // Preserve the C++ oracle's expression order for pixel parity.
fn render_footer(ui: &Ui, right_child_width: f32, state: &mut ShellState) {
    ui.same_line_with_pos((right_child_width - FOOTER_CONTROLS_FROM_RIGHT).max(0.0));
    ui.align_text_to_frame_padding();
    nudge_footer_y(ui);
    ui.text("Always On-Top");
    ui.same_line();
    nudge_footer_y(ui);
    ui.checkbox("##OnTopToggle", &mut state.always_on_top);
    ui.same_line();
    nudge_footer_y(ui);
    ui.text("Opacity");
    ui.same_line();
    ui.set_next_item_width(OPACITY_SLIDER_WIDTH);
    nudge_footer_y(ui);
    if let Ok(slider) =
        ui.slider_config("##OpacitySlider", 20.0_f32, 100.0_f32).try_display_format("%.0f%%")
    {
        slider.build(&mut state.opacity_percent);
    }

    let style = ui.clone_style();
    let revert_width = ui.calc_text_size("Revert Module")[0] + style.frame_padding()[0] * 2.0;
    ui.same_line_with_pos(
        right_child_width - PROFILES_BUTTON_WIDTH - style.item_spacing()[0] - revert_width,
    );
    nudge_footer_y(ui);
    let _ = ui.button("Revert Module");

    ui.same_line_with_pos(right_child_width - PROFILES_BUTTON_WIDTH);
    nudge_footer_y(ui);
    let _ = ui.button_with_size("Profiles ->", [PROFILES_BUTTON_WIDTH, 0.0]);
}

fn nudge_footer_y(ui: &Ui) {
    ui.set_cursor_pos_y(ui.cursor_pos_y() + FOOTER_CONTROL_Y_OFFSET);
}

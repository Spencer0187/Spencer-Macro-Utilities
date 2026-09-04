use dear_imgui_rs::{Condition, StyleVar, Ui, WindowFlags};

use crate::{
    bunnyhop::{self, BunnyhopUiState},
    floor_bounce::{self, FloorBounceUiState},
    freeze::{self, FreezeUiState},
    global_settings::{self, GlobalSettingsAction, GlobalSettingsState},
    hhj::{self, HhjGlobals, HhjUiState},
    item_clip::{self, ItemClipUiState},
    item_desync::{self, ItemDesyncUiState},
    item_unequip::{self, ItemUnequipGlobals, ItemUnequipUiState},
    keybind,
    lag_switch::{self, LagSwitchAction, LagSwitchUiState},
    laugh_clip::{self, LaughClipGlobals, LaughClipUiState},
    ledge_bounce::{self, LedgeBounceUiState},
    press_key::{self, PressKeyUiState},
    profile_browser::{self, ProfileAction, ProfileBrowserState},
    settings_menu, sidebar,
    spam_key::{self, SpamKeyUiState},
    speedglitch::{self, SpeedglitchUiState},
    wall_walk::{self, WallWalkUiState},
    walless_lhj::{self, WallessLhjGlobals, WallessLhjUiState},
    wallhop::{self, WallhopUiState},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShellAction {
    ToggleLagSwitchBackend,
    Profile(ProfileAction),
    StartWaylandScreenCaptureFromSettings,
    StopWaylandScreenCaptureFromSettings,
    StartWaylandRemoteDesktopFromSettings,
    #[cfg(target_os = "linux")]
    StartLagSwitchHelper,
    #[cfg(target_os = "linux")]
    StartFreezeHelper,
    #[cfg(target_os = "linux")]
    CancelFreezeHelper,
    #[cfg(target_os = "linux")]
    ApproveWaylandScreenCapture,
    #[cfg(target_os = "linux")]
    DeclineWaylandScreenCapture,
    #[cfg(target_os = "linux")]
    ApproveWaylandRemoteDesktop,
    #[cfg(target_os = "linux")]
    DeclineWaylandRemoteDesktop,
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

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum LinuxModal {
    LagSwitchHelper,
    FreezeHelper,
    WaylandScreenCapture,
    WaylandRemoteDesktop,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Default)]
struct LinuxModalState {
    pending_open: std::collections::BTreeSet<LinuxModal>,
    admin_error: String,
}

#[derive(Debug)]
pub struct ShellState {
    always_on_top: bool,
    opacity_percent: f32,
    global: GlobalSettingsState,
    settings_open: bool,
    keybind_capture: keybind::CaptureState,
    settings_platform: settings_menu::PlatformStatus,
    profile_browser: ProfileBrowserState,
    selected_section: usize,
    freeze: FreezeUiState,
    floor_bounce: FloorBounceUiState,
    bunnyhop: BunnyhopUiState,
    hhj: HhjUiState,
    item_desync: ItemDesyncUiState,
    item_clip: ItemClipUiState,
    item_unequip: ItemUnequipUiState,
    laugh_clip: LaughClipUiState,
    press_key: PressKeyUiState,
    ledge_bounce: LedgeBounceUiState,
    spam_key: SpamKeyUiState,
    speedglitch: SpeedglitchUiState,
    wall_walk: WallWalkUiState,
    walless_lhj: WallessLhjUiState,
    wallhop: WallhopUiState,
    lag_switch: LagSwitchUiState,
    #[cfg(target_os = "linux")]
    linux_modals: LinuxModalState,
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            always_on_top: false,
            opacity_percent: 100.0,
            global: GlobalSettingsState::default(),
            settings_open: false,
            keybind_capture: keybind::CaptureState::default(),
            settings_platform: settings_menu::PlatformStatus::default(),
            profile_browser: ProfileBrowserState::default(),
            selected_section: 15,
            freeze: FreezeUiState::default(),
            floor_bounce: FloorBounceUiState::default(),
            bunnyhop: BunnyhopUiState::default(),
            hhj: HhjUiState::default(),
            item_desync: ItemDesyncUiState::default(),
            item_clip: ItemClipUiState::default(),
            item_unequip: ItemUnequipUiState::default(),
            laugh_clip: LaughClipUiState::default(),
            press_key: PressKeyUiState::default(),
            ledge_bounce: LedgeBounceUiState::default(),
            spam_key: SpamKeyUiState::default(),
            speedglitch: SpeedglitchUiState::default(),
            wall_walk: WallWalkUiState::default(),
            walless_lhj: WallessLhjUiState::default(),
            wallhop: WallhopUiState::default(),
            lag_switch: LagSwitchUiState::default(),
            #[cfg(target_os = "linux")]
            linux_modals: LinuxModalState::default(),
        }
    }
}

impl ShellState {
    #[must_use]
    pub fn for_parity_case(case: Option<&str>) -> Self {
        let mut state = Self::default();
        if case == Some("lag-switch-user-reference") {
            state.global.apply_lag_switch_reference_globals();
            state.selected_section = 15;
            state.freeze.enabled = false;
            state.lag_switch = LagSwitchUiState::user_reference();
        }
        state
    }

    #[must_use]
    const fn macro_runtime_enabled(&self) -> bool {
        self.global.macro_enabled() && self.keybind_capture.runtime_allowed()
    }

    #[must_use]
    pub const fn keybind_input_poll_needed(&self) -> bool {
        self.keybind_capture.capture_active() || self.keybind_capture.suppressing_hotkeys()
    }

    #[must_use]
    pub const fn keybind_capture_active(&self) -> bool {
        self.keybind_capture.capture_active()
    }

    pub const fn set_keybind_input(&mut self, input: keybind::InputSnapshot) {
        self.keybind_capture.set_input(input);
    }

    #[cfg(target_os = "linux")]
    pub fn set_settings_platform_status(&mut self, status: settings_menu::PlatformStatus) {
        self.settings_platform = status;
    }

    pub const fn profile_browser_mut(&mut self) -> &mut ProfileBrowserState {
        &mut self.profile_browser
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn process_selector(&self) -> String {
        self.global.process_selector()
    }

    #[cfg(target_os = "linux")]
    pub const fn set_process_found(&mut self, found: bool) {
        self.global.set_process_found(found);
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn freeze_controller_settings(&self) -> crate::freeze_runtime::FreezeControllerSettings {
        self.freeze
            .controller_settings(self.macro_runtime_enabled(), self.global.process_selector())
    }

    #[must_use]
    fn section_enabled(&self) -> [bool; 16] {
        let mut enabled = [false; 16];
        enabled[0] = self.freeze.enabled;
        enabled[1] = self.item_desync.enabled;
        enabled[2] = self.hhj.enabled;
        enabled[3] = self.speedglitch.enabled;
        enabled[4] = self.item_unequip.enabled;
        enabled[5] = self.press_key.first_enabled();
        enabled[6] = self.wallhop.first_enabled();
        enabled[7] = self.walless_lhj.enabled;
        enabled[8] = self.item_clip.enabled;
        enabled[9] = self.laugh_clip.enabled;
        enabled[10] = self.wall_walk.enabled;
        enabled[11] = self.spam_key.first_enabled();
        enabled[12] = self.ledge_bounce.enabled;
        enabled[13] = self.bunnyhop.enabled;
        enabled[14] = self.floor_bounce.enabled;
        enabled[15] = self.lag_switch.enabled;
        enabled
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn floor_bounce_controller_settings(
        &self,
    ) -> crate::floor_bounce_runtime::FloorBounceControllerSettings {
        self.floor_bounce.controller_settings(
            self.macro_runtime_enabled(),
            self.global.process_selector(),
            self.freeze.take_all_process_ids,
            self.global.shift_key(),
        )
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn hhj_controller_settings(&self) -> crate::hhj_runtime::HhjControllerSettings {
        let (strength_x, strength_y) = self.speedglitch.motion_strengths();
        self.hhj.controller_settings(HhjGlobals {
            macro_enabled: self.macro_runtime_enabled(),
            process_selector: self.global.process_selector(),
            take_all_process_ids: self.freeze.take_all_process_ids,
            global_zoom: self.global.global_zoom(),
            shift_key: self.global.shift_key(),
            zoom_reverse: self.global.zoom_reverse(),
            fps: self.global.fps_runtime(),
            strength_x,
            strength_y,
        })
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn item_unequip_controller_settings(
        &self,
    ) -> crate::item_unequip_runtime::ItemUnequipControllerSettings {
        self.item_unequip.controller_settings(ItemUnequipGlobals {
            macro_enabled: self.macro_runtime_enabled(),
            process_selector: self.global.process_selector(),
            chat_override: self.global.chat_override(),
            chat_key: self.global.chat_key(),
            enter_key: self.global.enter_key(),
            paste_delay_ms: self.global.paste_delay_ms(),
        })
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn item_desync_controller_settings(
        &self,
    ) -> crate::item_desync_runtime::ItemDesyncControllerSettings {
        self.item_desync
            .controller_settings(self.macro_runtime_enabled(), self.global.process_selector())
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn item_clip_controller_settings(
        &self,
    ) -> crate::item_clip_runtime::ItemClipControllerSettings {
        self.item_clip
            .controller_settings(self.macro_runtime_enabled(), self.global.process_selector())
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn laugh_clip_controller_settings(
        &self,
    ) -> crate::laugh_clip_runtime::LaughClipControllerSettings {
        self.laugh_clip.controller_settings(LaughClipGlobals {
            macro_enabled: self.macro_runtime_enabled(),
            process_selector: self.global.process_selector(),
            chat_override: self.global.chat_override(),
            chat_key: self.global.chat_key(),
            enter_key: self.global.enter_key(),
            paste_delay_ms: self.global.paste_delay_ms(),
            global_zoom: self.global.global_zoom(),
            shift_key: self.global.shift_key(),
            zoom_reverse: self.global.zoom_reverse(),
        })
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn spam_key_controller_settings(
        &self,
    ) -> crate::spam_key_runtime::SpamKeyControllerSettings {
        self.spam_key
            .controller_settings(self.macro_runtime_enabled(), self.global.process_selector())
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn bunnyhop_controller_settings(
        &self,
    ) -> crate::bunnyhop_runtime::BunnyhopControllerSettings {
        self.bunnyhop.controller_settings(
            self.macro_runtime_enabled(),
            self.global.chat_key(),
            self.global.process_selector(),
        )
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn ledge_bounce_controller_settings(
        &self,
    ) -> crate::ledge_bounce_runtime::LedgeBounceControllerSettings {
        self.ledge_bounce.controller_settings(
            self.macro_runtime_enabled(),
            self.global.sensitivity().to_owned(),
            self.global.cam_fix_enabled(),
            self.global.process_selector(),
        )
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn speedglitch_controller_settings(
        &self,
    ) -> crate::speedglitch_runtime::SpeedglitchControllerSettings {
        self.speedglitch.controller_settings(
            self.macro_runtime_enabled(),
            self.global.fps_runtime(),
            self.global.process_selector(),
        )
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn press_key_controller_settings(
        &self,
    ) -> crate::press_key_runtime::PressKeyControllerSettings {
        self.press_key
            .controller_settings(self.macro_runtime_enabled(), self.global.process_selector())
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn wallhop_controller_settings(&self) -> crate::wallhop_runtime::WallhopControllerSettings {
        self.wallhop
            .controller_settings(self.macro_runtime_enabled(), self.global.process_selector())
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn walless_lhj_controller_settings(
        &self,
    ) -> crate::walless_lhj_runtime::WallessLhjControllerSettings {
        self.walless_lhj.controller_settings(WallessLhjGlobals {
            macro_enabled: self.macro_runtime_enabled(),
            process_selector: self.global.process_selector(),
            global_zoom: self.global.global_zoom(),
            shift_key: self.global.shift_key(),
            zoom_reverse: self.global.zoom_reverse(),
            take_all_process_ids: self.freeze.take_all_process_ids,
        })
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn wall_walk_controller_settings(
        &self,
    ) -> crate::wall_walk_runtime::WallWalkControllerSettings {
        self.wall_walk.controller_settings(
            self.macro_runtime_enabled(),
            self.global.fps_runtime(),
            self.global.process_selector(),
        )
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn lag_switch_controller_settings(
        &self,
    ) -> crate::lag_switch_runtime::LagSwitchControllerSettings {
        self.lag_switch.controller_settings(self.macro_runtime_enabled())
    }

    #[cfg(target_os = "linux")]
    pub const fn set_lag_switch_backend(&mut self, backend: crate::lag_switch::BackendState) {
        self.lag_switch.backend = backend;
    }

    #[cfg(target_os = "linux")]
    pub fn request_lag_switch_helper_popup(&mut self) {
        self.linux_modals.pending_open.insert(LinuxModal::LagSwitchHelper);
    }

    #[cfg(target_os = "linux")]
    pub fn set_lag_switch_helper_error(&mut self, error: impl Into<String>) {
        self.linux_modals.admin_error = error.into();
    }

    #[cfg(target_os = "linux")]
    pub fn request_freeze_helper_popup(&mut self) {
        self.linux_modals.pending_open.insert(LinuxModal::FreezeHelper);
    }

    #[cfg(target_os = "linux")]
    pub fn request_wayland_screen_capture_popup(&mut self) {
        self.linux_modals.pending_open.insert(LinuxModal::WaylandScreenCapture);
    }

    #[cfg(target_os = "linux")]
    pub fn request_wayland_remote_desktop_popup(&mut self) {
        self.linux_modals.pending_open.insert(LinuxModal::WaylandRemoteDesktop);
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
    if let Some(modal_action) = render_linux_modals(ui, &mut state.linux_modals) {
        action = Some(modal_action);
    }

    if let Some(settings_action) = settings_menu::render(
        ui,
        &mut state.settings_open,
        &mut state.global,
        &mut state.keybind_capture,
        &state.settings_platform,
    ) {
        action = Some(match settings_action {
            settings_menu::SettingsAction::StartWaylandScreenCapture => {
                ShellAction::StartWaylandScreenCaptureFromSettings
            }
            settings_menu::SettingsAction::StopWaylandScreenCapture => {
                ShellAction::StopWaylandScreenCaptureFromSettings
            }
            settings_menu::SettingsAction::StartWaylandRemoteDesktop => {
                ShellAction::StartWaylandRemoteDesktopFromSettings
            }
        });
    }

    action
}

#[cfg(target_os = "linux")]
fn open_linux_modal(ui: &Ui, state: &mut LinuxModalState, modal: LinuxModal, name: &str) {
    if state.pending_open.remove(&modal) {
        let _ = ui.open_popup(name);
    }
}

#[cfg(target_os = "linux")]
fn render_linux_modals(ui: &Ui, state: &mut LinuxModalState) -> Option<ShellAction> {
    if let Some(action) = render_lag_switch_helper_modal(ui, state) {
        return Some(action);
    }
    if let Some(action) = render_freeze_helper_modal(ui, state) {
        return Some(action);
    }
    if let Some(action) = render_wayland_screen_capture_modal(ui, state) {
        return Some(action);
    }
    render_wayland_remote_desktop_modal(ui, state)
}

#[cfg(target_os = "linux")]
fn render_lag_switch_helper_modal(ui: &Ui, state: &mut LinuxModalState) -> Option<ShellAction> {
    open_linux_modal(ui, state, LinuxModal::LagSwitchHelper, "Administrator Required");
    let _popup = ui.begin_modal_popup("Administrator Required")?;
    ui.text_wrapped("Linux lagswitch features require a privileged nethelper process.");
    ui.text_wrapped("This process involves:");
    ui.bullet_text("Starting the bundled nethelper with pkexec.");
    ui.bullet_text("Keeping the SMU GUI running as your normal desktop user.");
    if !state.admin_error.is_empty() {
        ui.text_colored([1.0, 0.20, 0.40, 1.0], &state.admin_error);
    }
    crate::parity::separator(ui);
    if ui.button_with_size("Start Helper", [180.0, 0.0]) {
        ui.close_current_popup();
        return Some(ShellAction::StartLagSwitchHelper);
    }
    ui.same_line();
    if ui.button_with_size("Cancel", [120.0, 0.0]) {
        ui.close_current_popup();
    }
    None
}

#[cfg(target_os = "linux")]
fn render_freeze_helper_modal(ui: &Ui, state: &mut LinuxModalState) -> Option<ShellAction> {
    open_linux_modal(ui, state, LinuxModal::FreezeHelper, "Additional Freeze Permission Required");
    let _popup = ui.begin_modal_popup("Additional Freeze Permission Required")?;
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
        ui.close_current_popup();
        return Some(ShellAction::StartFreezeHelper);
    }
    ui.same_line();
    if ui.button_with_size("Cancel##FreezeHelper", [120.0, 0.0]) {
        ui.close_current_popup();
        return Some(ShellAction::CancelFreezeHelper);
    }
    None
}

#[cfg(target_os = "linux")]
fn render_wayland_screen_capture_modal(
    ui: &Ui,
    state: &mut LinuxModalState,
) -> Option<ShellAction> {
    open_linux_modal(ui, state, LinuxModal::WaylandScreenCapture, "Enable Wayland Screen Capture?");
    let _popup = ui.begin_modal_popup("Enable Wayland Screen Capture?")?;
    ui.text_wrapped(
        "A running Lua script called getPixelColor() or getPixelRect(). On Wayland, SMU must ask the desktop portal for permission to capture one monitor.",
    );
    ui.text_wrapped(
        "Enable will open your desktop's monitor picker. The script stays paused until a monitor is selected and the first frame is ready.",
    );
    crate::parity::separator(ui);
    if ui.button("Enable and Select Monitor") {
        ui.close_current_popup();
        return Some(ShellAction::ApproveWaylandScreenCapture);
    }
    ui.same_line();
    if ui.button("Don't Enable##WaylandScreenCapture") {
        ui.close_current_popup();
        return Some(ShellAction::DeclineWaylandScreenCapture);
    }
    None
}

#[cfg(target_os = "linux")]
fn render_wayland_remote_desktop_modal(
    ui: &Ui,
    state: &mut LinuxModalState,
) -> Option<ShellAction> {
    open_linux_modal(
        ui,
        state,
        LinuxModal::WaylandRemoteDesktop,
        "Enable Wayland Absolute Mouse Control?",
    );
    let _popup = ui.begin_modal_popup("Enable Wayland Absolute Mouse Control?")?;
    ui.text_wrapped(
        "A running Lua script called moveMouseAbs(). On Wayland, SMU needs explicit permission to control the pointer and capture one monitor through the desktop portal.",
    );
    ui.text_wrapped(
        "Enable opens your desktop's permission dialog. The script remains paused until pointer control and the selected monitor's first frame are ready. This does not let SMU read the current global cursor position.",
    );
    crate::parity::separator(ui);
    if ui.button("Enable Pointer Control and Select Monitor") {
        ui.close_current_popup();
        return Some(ShellAction::ApproveWaylandRemoteDesktop);
    }
    ui.same_line();
    if ui.button("Don't Enable##WaylandRemoteDesktop") {
        ui.close_current_popup();
        return Some(ShellAction::DeclineWaylandRemoteDesktop);
    }
    None
}

fn render_macro_sidebar(
    ui: &Ui,
    state: &mut ShellState,
    left_panel_width: f32,
    left_panel_height: f32,
) {
    let delta_time = ui.io().delta_time();
    state.press_key.tick_remove_confirmation(delta_time, state.selected_section == 5);
    state.wallhop.tick_remove_confirmation(delta_time, state.selected_section == 6);
    state.spam_key.tick_remove_confirmation(delta_time, state.selected_section == 11);
    let section_enabled = state.section_enabled();
    let multi_instances = [
        sidebar::MultiInstanceSummary {
            section_index: 5,
            enabled: state.press_key.enabled_flags(),
            selected_instance: state.press_key.selected_index(),
        },
        sidebar::MultiInstanceSummary {
            section_index: 6,
            enabled: state.wallhop.enabled_flags(),
            selected_instance: state.wallhop.selected_index(),
        },
        sidebar::MultiInstanceSummary {
            section_index: 11,
            enabled: state.spam_key.enabled_flags(),
            selected_instance: state.spam_key.selected_index(),
        },
    ];
    let mut sidebar_action = None;
    ui.child_window(LEFT_SCROLL_CHILD)
        .size([left_panel_width, left_panel_height])
        .border(true)
        .build(ui, || {
            sidebar_action = sidebar::render(
                ui,
                left_panel_width,
                &mut state.selected_section,
                &section_enabled,
                &multi_instances,
                state.global.short_descriptions(),
            );
        });
    match sidebar_action {
        Some(sidebar::SidebarAction::SelectInstance { section_index: 5, instance_index }) => {
            state.press_key.select(instance_index);
        }
        Some(sidebar::SidebarAction::AddInstance { section_index: 5 }) => {
            state.press_key.add_default();
        }
        Some(sidebar::SidebarAction::SelectInstance { section_index: 6, instance_index }) => {
            state.wallhop.select(instance_index);
        }
        Some(sidebar::SidebarAction::AddInstance { section_index: 6 }) => {
            state.wallhop.add_default();
        }
        Some(sidebar::SidebarAction::SelectInstance { section_index: 11, instance_index }) => {
            state.spam_key.select(instance_index);
        }
        Some(sidebar::SidebarAction::AddInstance { section_index: 11 }) => {
            state.spam_key.add_default();
        }
        _ => {}
    }
}

#[allow(clippy::suboptimal_flops)] // Preserve the C++ oracle's expression order for pixel parity.
fn render_contents(ui: &Ui, state: &mut ShellState, display_size: [f32; 2]) -> Option<ShellAction> {
    let style = ui.clone_style();
    let settings_panel_height = ui.text_line_height_with_spacing() * SETTINGS_TEXT_ROWS
        + style.window_padding()[1] * SETTINGS_PADDING_ROWS
        + SETTINGS_EXTRA_HEIGHT;

    let mut global_action = None;
    ui.child_window(GLOBAL_SETTINGS_CHILD)
        .size([display_size[0] - SETTINGS_HORIZONTAL_MARGIN, settings_panel_height])
        .border(true)
        .build(ui, || {
            global_action = global_settings::render(ui, &mut state.global);
        });
    if global_action == Some(GlobalSettingsAction::ToggleSettings) {
        state.settings_open = !state.settings_open;
    }
    state.speedglitch.sync_global_calibration(
        state.global.sensitivity(),
        state.global.cam_fix_enabled(),
        state.global.speed_calibration_revision(),
    );
    state.wall_walk.sync_global_calibration(
        state.global.sensitivity(),
        state.global.cam_fix_enabled(),
        state.global.speed_calibration_revision(),
    );
    state.wallhop.sync_sensitivity_edit(
        state.global.sensitivity(),
        state.global.cam_fix_enabled(),
        state.global.wallhop_sensitivity_revision(),
    );

    let left_panel_width = ui.window_size()[0] * LEFT_WIDTH_FRACTION - MAIN_HORIZONTAL_ADJUSTMENT;
    let left_panel_height = ui.window_size()[1] - settings_panel_height - MAIN_VERTICAL_ADJUSTMENT;
    render_macro_sidebar(ui, state, left_panel_width, left_panel_height);

    ui.same_line();
    let right_section_size = [
        display_size[0] - MAIN_HORIZONTAL_ADJUSTMENT - left_panel_width,
        display_size[1] - settings_panel_height - MAIN_VERTICAL_ADJUSTMENT - FOOTER_HEIGHT,
    ];
    let mut lag_action = None;
    ui.child_window(RIGHT_SECTION_CHILD).size(right_section_size).border(true).build(ui, || {
        match state.selected_section {
            0 => freeze::render(ui, &mut state.freeze),
            1 => item_desync::render(ui, &mut state.item_desync),
            2 => hhj::render(ui, &mut state.hhj),
            3 => speedglitch::render(ui, &mut state.speedglitch),
            4 => item_unequip::render(ui, &mut state.item_unequip),
            5 => press_key::render(ui, &mut state.press_key),
            6 => wallhop::render(
                ui,
                &mut state.wallhop,
                state.global.sensitivity(),
                state.global.cam_fix_enabled(),
            ),
            7 => walless_lhj::render(ui, &mut state.walless_lhj),
            8 => item_clip::render(ui, &mut state.item_clip),
            9 => laugh_clip::render(ui, &mut state.laugh_clip),
            10 => wall_walk::render(
                ui,
                &mut state.wall_walk,
                state.global.sensitivity(),
                state.global.cam_fix_enabled(),
            ),
            11 => spam_key::render(ui, &mut state.spam_key),
            12 => ledge_bounce::render(ui, &mut state.ledge_bounce),
            13 => bunnyhop::render(ui, &mut state.bunnyhop),
            14 => floor_bounce::render(ui, &mut state.floor_bounce),
            15 => lag_action = lag_switch::render(ui, &mut state.lag_switch),
            section => {
                ui.text_wrapped(format!("Settings for {}", sidebar::section_title(section)));
                crate::parity::separator(ui);
                ui.new_line();
                ui.text_wrapped("This section has not been cut over to the Rust application yet.");
            }
        }
    });

    let right_child_min = ui.item_rect_min();
    let right_child_size = ui.item_rect_size();
    let window_size = ui.window_size();
    ui.set_cursor_pos_y(window_size[1] - FOOTER_HEIGHT - style.window_padding()[1]);
    ui.set_cursor_pos_x(right_child_min[0]);

    let _square_footer = ui.push_style_var(StyleVar::ChildRounding(0.0));
    let mut profile_action = None;
    ui.child_window(BOTTOM_CONTROLS_CHILD)
        .size([right_child_size[0] - FOOTER_BORDER_COMPENSATION, FOOTER_HEIGHT])
        .flags(WindowFlags::NO_SCROLLBAR | WindowFlags::NO_SCROLL_WITH_MOUSE)
        .build(ui, || {
            profile_action = render_footer(ui, right_child_size[0], state);
        });

    lag_action
        .map(|LagSwitchAction::ToggleBackend| ShellAction::ToggleLagSwitchBackend)
        .or_else(|| profile_action.map(ShellAction::Profile))
}

#[allow(clippy::suboptimal_flops)] // Preserve the C++ oracle's expression order for pixel parity.
fn render_footer(ui: &Ui, right_child_width: f32, state: &mut ShellState) -> Option<ProfileAction> {
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
    profile_browser::render(ui, &mut state.profile_browser, PROFILES_BUTTON_WIDTH)
}

fn nudge_footer_y(ui: &Ui) {
    ui.set_cursor_pos_y(ui.cursor_pos_y() + FOOTER_CONTROL_Y_OFFSET);
}

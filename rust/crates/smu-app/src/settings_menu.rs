use crate::{global_settings::GlobalSettingsState, keybind, theme::ThemeDefinition};
use dear_imgui_rs::{Condition, MouseCursor, TreeNodeFlags, Ui, WindowFlags, sys};

pub const UPDATE_AUTO_PARITY_CASE: &str = "modal-update-auto-user-reference";
pub const UPDATE_MANUAL_PARITY_CASE: &str = "modal-update-manual-user-reference";

pub const SPONSOR_URL: &str = "https://github.com/sponsors/Spencer0187";

#[cfg(target_os = "macos")]
const MANUAL_UPDATE_GUIDANCE: &str = "Automatic installation is not yet enabled for the Rust macOS app. Open the official release, download the macOS package, and replace this app manually.";
#[cfg(target_os = "windows")]
const MANUAL_UPDATE_GUIDANCE: &str = "Automatic installation is not yet enabled for the Rust Windows app. Open the official release, download the Windows package, and replace this installation manually.";
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const MANUAL_UPDATE_GUIDANCE: &str = "This Linux package cannot replace itself. Download the release and reinstall it with the same AppImage, Debian, RPM, portable, or Nix method you used before.";

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PlatformStatus {
    pub show_wayland: bool,
    pub wayland_supported: bool,
    pub wayland_active: bool,
    pub remote_desktop_active: bool,
    pub wayland_busy: bool,
    pub wayland_status: String,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UpdaterPanelStatus {
    pub enabled: bool,
    pub checked_once: bool,
    pub checking: bool,
    pub applying: bool,
    pub apply_failed: bool,
    pub update_confirm_open: bool,
    pub check_succeeded: bool,
    pub update_available: bool,
    pub auto_apply_supported: bool,
    pub local_version: String,
    pub latest_version: String,
    pub selected_package: String,
    pub status_message: String,
    pub action_message: String,
}

impl UpdaterPanelStatus {
    #[must_use]
    pub const fn user_outdated(&self) -> bool {
        self.checked_once && self.update_available
    }
}

#[must_use]
pub fn parity_update_status(case: Option<&str>) -> Option<UpdaterPanelStatus> {
    let auto_apply_supported = match case {
        Some(UPDATE_AUTO_PARITY_CASE) => true,
        Some(UPDATE_MANUAL_PARITY_CASE) => false,
        _ => return None,
    };
    Some(UpdaterPanelStatus {
        enabled: true,
        checked_once: true,
        update_confirm_open: true,
        check_succeeded: true,
        update_available: true,
        auto_apply_supported,
        local_version: "3.4.0".to_owned(),
        latest_version: "9.9.9".to_owned(),
        selected_package: "Spencer-Macro-Utilities-x86_64.AppImage".to_owned(),
        status_message: "An update is available.".to_owned(),
        action_message: "An update is available.".to_owned(),
        ..UpdaterPanelStatus::default()
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsAction {
    CheckUpdates,
    RequestInstallUpdate,
    ConfirmInstallUpdate,
    SkipUpdate,
    CancelUpdate,
    OpenLatestRelease,
    OpenLatestReleaseAndDismiss,
    OpenSponsor,
    StartWaylandScreenCapture,
    StopWaylandScreenCapture,
    StartWaylandRemoteDesktop,
}

pub fn render(
    ui: &Ui,
    open: &mut bool,
    globals: &mut GlobalSettingsState,
    capture: &mut keybind::CaptureState,
    platform: &PlatformStatus,
    updater: &UpdaterPanelStatus,
    theme: &ThemeDefinition,
) -> Option<SettingsAction> {
    if !*open {
        return None;
    }
    let display = ui.io().display_size();
    let size = [display[0] * 0.5, display[1] * 0.5];
    let fallback_position = [display[0] * 0.4, (display[1] - size[1] - 90.0) * 0.5];
    let viewport = ui.main_viewport();
    let viewport_position = viewport.pos();
    let viewport_size = viewport.size();
    let position = clamp_window_position(fallback_position, size, viewport_position, viewport_size);
    let mut action = None;
    ui.window("Settings Menu")
        .opened(open)
        .size(size, Condition::Always)
        .position(position, Condition::Appearing)
        .flags(WindowFlags::NO_RESIZE | WindowFlags::NO_COLLAPSE)
        .build(|| {
            clamp_current_window(ui, viewport_position, viewport_size);
            ui.child_window("SettingsList").border(true).build(ui, || {
                render_updater(ui, updater, &mut action, theme);
                render_wayland(ui, platform, &mut action, theme);
                render_scalar_settings(ui, globals, capture, &mut action, theme);
            });
        });
    action
}

fn clamp_current_window(ui: &Ui, viewport_position: [f32; 2], viewport_size: [f32; 2]) {
    let current_position = ui.window_pos();
    let current_size = ui.window_size();
    let clamped =
        clamp_window_position(current_position, current_size, viewport_position, viewport_size);
    if clamped[0].to_bits() != current_position[0].to_bits()
        || clamped[1].to_bits() != current_position[1].to_bits()
    {
        ui.set_window_pos(clamped);
    }
}

fn clamp_window_position(
    position: [f32; 2],
    size: [f32; 2],
    viewport_position: [f32; 2],
    viewport_size: [f32; 2],
) -> [f32; 2] {
    let max_position = [
        viewport_position[0] + (viewport_size[0] - size[0]).max(0.0),
        viewport_position[1] + (viewport_size[1] - size[1]).max(0.0),
    ];
    [
        position[0].clamp(viewport_position[0], max_position[0]),
        position[1].clamp(viewport_position[1], max_position[1]),
    ]
}

fn render_updater(
    ui: &Ui,
    updater: &UpdaterPanelStatus,
    action: &mut Option<SettingsAction>,
    theme: &ThemeDefinition,
) {
    if !ui.collapsing_header("Updates", TreeNodeFlags::DEFAULT_OPEN) {
        return;
    }

    ui.text(format!("Local version: {}", updater.local_version));
    if updater.checked_once && !updater.latest_version.is_empty() {
        ui.text(format!("Latest version: {}", updater.latest_version));
    } else {
        ui.text("Latest version: unknown");
    }

    if updater.checking {
        ui.text("Status: checking...");
    } else if updater.checked_once {
        ui.text_wrapped(format!("Status: {}", updater.status_message));
    } else if updater.enabled {
        ui.text("Status: not checked yet");
    } else {
        ui.text("Status: update checks disabled in this session");
    }

    if !updater.selected_package.is_empty() {
        ui.text_wrapped(format!("Selected package: {}", updater.selected_package));
    }

    let check_label = if updater.checked_once && !updater.check_succeeded {
        "Retry update check"
    } else {
        "Check for updates"
    };
    let check_disabled = (!updater.enabled || updater.checking).then(|| ui.begin_disabled());
    if ui.button(check_label) {
        *action = Some(SettingsAction::CheckUpdates);
    }
    drop(check_disabled);

    let can_apply = updater.checked_once
        && updater.update_available
        && updater.auto_apply_supported
        && !updater.selected_package.is_empty()
        && !updater.checking
        && !updater.applying;
    ui.same_line();
    let apply_disabled = (!can_apply).then(|| ui.begin_disabled());
    let apply_label = if updater.applying {
        "Applying update..."
    } else if updater.apply_failed {
        "Retry update"
    } else {
        "Download and install update"
    };
    if ui.button(apply_label) {
        *action = Some(SettingsAction::RequestInstallUpdate);
    }
    drop(apply_disabled);

    if updater.checked_once && updater.update_available && !updater.auto_apply_supported {
        ui.text_colored(theme.warning_color, MANUAL_UPDATE_GUIDANCE);
    }

    if ui.button("Open latest release") {
        *action = Some(SettingsAction::OpenLatestRelease);
    }
    if !updater.action_message.is_empty() {
        ui.text_wrapped(&updater.action_message);
    }
    crate::parity::separator(ui);
}

fn set_update_modal_layout(ui: &Ui) {
    let viewport = ui.main_viewport();
    let pos = viewport.pos();
    let size = viewport.size();
    let center = [size[0].mul_add(0.5, pos[0]), size[1].mul_add(0.5, pos[1])];
    // SAFETY: these configure the immediately following modal Begin while `ui` owns the active
    // Dear ImGui context for this frame.
    unsafe {
        sys::igSetNextWindowPos(
            sys::ImVec2_c { x: center[0], y: center[1] },
            Condition::Appearing as i32,
            sys::ImVec2_c { x: 0.5, y: 0.5 },
        );
        sys::igSetNextWindowSize(sys::ImVec2_c { x: 520.0, y: 0.0 }, Condition::Appearing as i32);
    }
}

pub fn render_update_confirmation(ui: &Ui, updater: &UpdaterPanelStatus) -> Option<SettingsAction> {
    if updater.update_confirm_open {
        let _ = ui.open_popup("Update Available");
    }
    set_update_modal_layout(ui);
    let _popup = ui
        .begin_modal_popup_config("Update Available")
        .flags(WindowFlags::ALWAYS_AUTO_RESIZE)
        .begin()?;

    let latest =
        if updater.latest_version.is_empty() { "unknown" } else { &updater.latest_version };
    let package = if updater.selected_package.is_empty() {
        "No matching asset"
    } else {
        &updater.selected_package
    };
    let can_auto_apply = updater.auto_apply_supported && !updater.selected_package.is_empty();
    ui.text_wrapped("A new version of Spencer Macro Utilities is available.");
    ui.spacing();
    ui.text(format!("Current version: {}", updater.local_version));
    ui.text(format!("Latest version: {latest}"));
    ui.text(format!("Package: {package}"));
    ui.spacing();
    if can_auto_apply {
        ui.text_wrapped("Do you want to download and install this update now?");
    } else {
        ui.text_wrapped(MANUAL_UPDATE_GUIDANCE);
    }
    crate::parity::separator(ui);

    if updater.applying {
        ui.text("Downloading update...");
        return None;
    }
    if can_auto_apply {
        if ui.button_with_size("Yes", [90.0, 0.0]) {
            ui.close_current_popup();
            return Some(SettingsAction::ConfirmInstallUpdate);
        }
        ui.same_line();
        if ui.button_with_size("No", [90.0, 0.0]) {
            ui.close_current_popup();
            return Some(SettingsAction::SkipUpdate);
        }
        ui.same_line();
        let cancel = ui.button_with_size("Cancel", [90.0, 0.0]);
        ui.set_item_default_focus();
        if cancel {
            ui.close_current_popup();
            return Some(SettingsAction::CancelUpdate);
        }
        return None;
    }

    if ui.button_with_size("Open latest release", [170.0, 0.0]) {
        ui.close_current_popup();
        return Some(SettingsAction::OpenLatestReleaseAndDismiss);
    }
    ui.same_line();
    let not_now = ui.button_with_size("Not now", [100.0, 0.0]);
    ui.set_item_default_focus();
    if not_now {
        ui.close_current_popup();
        return Some(SettingsAction::SkipUpdate);
    }
    None
}

fn render_wayland(
    ui: &Ui,
    platform: &PlatformStatus,
    action: &mut Option<SettingsAction>,
    theme: &ThemeDefinition,
) {
    if !platform.show_wayland {
        return;
    }
    ui.text("Wayland Screen Capture");
    ui.text_wrapped(
        "Pixel scripts need explicit permission to capture one monitor. The selected monitor becomes the coordinate space for getPixelColor() and getPixelRect() while capture is active.",
    );
    if platform.wayland_supported {
        if platform.wayland_active {
            ui.text_colored(theme.success_color, format!("Status: {}", platform.wayland_status));
        } else {
            ui.text_wrapped(format!("Status: {}", platform.wayland_status));
        }
        let disabled = platform.wayland_busy.then(|| ui.begin_disabled());
        if platform.wayland_active {
            if ui.button("Stop Wayland Screen Capture") {
                *action = Some(SettingsAction::StopWaylandScreenCapture);
            }
        } else if ui.button("Enable Wayland Screen Capture") {
            *action = Some(SettingsAction::StartWaylandScreenCapture);
        }
        drop(disabled);
    } else {
        ui.text_colored(
            theme.error_color,
            "This build does not include PipeWire ScreenCast support.",
        );
    }
    crate::parity::separator(ui);

    ui.text("Wayland Absolute Mouse Control");
    ui.text_wrapped(
        "Allow moveMouseAbs() to place the pointer on the selected monitor. This replaces a screen-capture-only session with a RemoteDesktop + ScreenCast session and needs separate pointer-control permission.",
    );
    if platform.remote_desktop_active {
        ui.text_colored(theme.success_color, format!("Status: {}", platform.wayland_status));
    } else {
        ui.text_wrapped(format!("Status: {}", platform.wayland_status));
    }
    if !platform.remote_desktop_active {
        let disabled = platform.wayland_busy.then(|| ui.begin_disabled());
        if ui.button("Enable Wayland Absolute Mouse Control") {
            *action = Some(SettingsAction::StartWaylandRemoteDesktop);
        }
        drop(disabled);
    }
    crate::parity::separator(ui);
}

fn render_scalar_settings(
    ui: &Ui,
    globals: &mut GlobalSettingsState,
    capture: &mut keybind::CaptureState,
    action: &mut Option<SettingsAction>,
    theme: &ThemeDefinition,
) {
    ui.text("Your Current Windows Display Scale Value (10-500%):");
    ui.set_next_item_width(150.0);
    if ui.input_int("##DisplayScale", globals.display_scale_mut()) {
        *globals.display_scale_mut() = (*globals.display_scale_mut()).clamp(10, 500);
    }
    ui.same_line();
    ui.text("%");
    crate::parity::separator(ui);

    ui.text_wrapped("Custom Shiftlock Key:");
    keybind::render_control(ui, capture, "ShiftKey", globals.shift_key_mut(), 150.0, 50.0, true);
    crate::parity::separator(ui);

    ui.checkbox("Force-Set Chat Open Key to \"/\" (Most Stable)", globals.chat_override_mut());
    crate::parity::separator(ui);

    ui.text_wrapped("Custom Chat Key (Must disable Force-Set):");
    keybind::render_control(ui, capture, "ChatKey", globals.chat_key_mut(), 150.0, 50.0, true);
    crate::parity::separator(ui);

    ui.checkbox("##Oldpaste", globals.use_old_paste_mut());
    ui.same_line();
    ui.set_cursor_pos_x(ui.cursor_pos()[0] - 4.0);
    ui.text_wrapped(
        "Old Unicode Chat-Typing (Works across languages but may be blocked by some anticheats)",
    );
    crate::parity::separator(ui);

    ui.align_text_to_frame_padding();
    ui.text_wrapped("Delay between every key press in chat (ms):");
    ui.same_line();
    ui.set_next_item_width(30.0);
    let (paste_text, paste_value) = globals.paste_delay_parts();
    if ui.input_text("##PasteDelay", paste_text).chars_decimal(true).chars_no_blank(true).build()
        && let Ok(parsed) = paste_text.parse::<i32>()
    {
        *paste_value = parsed;
    }
    crate::parity::separator(ui);

    ui.text_wrapped("Custom Anti-AFK Key (That the macro uses):");
    keybind::render_control(ui, capture, "AfkKey", globals.afk_key_mut(), 150.0, 50.0, true);
    crate::parity::separator(ui);

    ui.align_text_to_frame_padding();
    ui.text("Amount of Minutes Between Anti-AFK Runs:");
    ui.same_line();
    ui.set_next_item_width(30.0);
    let (anti_afk_text, anti_afk_value) = globals.anti_afk_minutes_parts();
    if ui
        .input_text("##AntiAFKTime", anti_afk_text)
        .chars_decimal(true)
        .chars_no_blank(true)
        .build()
        && let Ok(parsed) = anti_afk_text.parse::<i32>()
    {
        *anti_afk_value = parsed;
    }
    crate::parity::separator(ui);

    ui.checkbox("Replace shiftlock with zooming in", globals.global_zoom_mut());
    ui.same_line();
    ui.checkbox("Reverse Direction?", globals.zoom_reverse_mut());
    crate::parity::separator(ui);
    ui.checkbox("Double-Press AFK keybind during Anti-AFK", globals.double_press_afk_key_mut());
    crate::parity::separator(ui);
    ui.checkbox("Remove Side-Bar Macro Descriptions", globals.short_descriptions_mut());
    crate::parity::separator(ui);

    ui.new_line();
    ui.text_colored(theme.accent_primary, "Want to Donate directly to my Github?");
    if ui.is_item_hovered() {
        ui.set_mouse_cursor(Some(MouseCursor::Hand));
        if ui.is_item_clicked() {
            *action = Some(SettingsAction::OpenSponsor);
        }
    }
    ui.text("Github Doesn't take any of the profits.");
}

#[cfg(test)]
mod tests {
    use super::{
        PlatformStatus, SPONSOR_URL, UPDATE_AUTO_PARITY_CASE, UPDATE_MANUAL_PARITY_CASE,
        UpdaterPanelStatus, clamp_window_position, parity_update_status,
    };

    fn assert_position_eq(actual: [f32; 2], expected: [f32; 2]) {
        assert_eq!(actual[0].to_bits(), expected[0].to_bits());
        assert_eq!(actual[1].to_bits(), expected[1].to_bits());
    }

    #[test]
    fn updater_status_defaults_are_inert() {
        let status = UpdaterPanelStatus::default();
        assert!(!status.enabled);
        assert!(!status.checked_once);
        assert!(!status.checking);
        assert!(!status.update_available);
        assert!(!status.user_outdated());
    }

    #[test]
    fn update_modal_parity_statuses_share_versions_and_package() {
        let auto = parity_update_status(Some(UPDATE_AUTO_PARITY_CASE)).unwrap_or_default();
        let manual = parity_update_status(Some(UPDATE_MANUAL_PARITY_CASE)).unwrap_or_default();
        assert!(auto.update_confirm_open && manual.update_confirm_open);
        assert!(auto.auto_apply_supported);
        assert!(!manual.auto_apply_supported);
        assert_eq!(auto.local_version, "3.4.0");
        assert_eq!(auto.latest_version, "9.9.9");
        assert_eq!(auto.selected_package, "Spencer-Macro-Utilities-x86_64.AppImage");
        assert_eq!(manual.local_version, auto.local_version);
        assert_eq!(manual.latest_version, auto.latest_version);
        assert_eq!(manual.selected_package, auto.selected_package);
    }

    #[test]
    fn updater_only_marks_user_outdated_after_completed_check() {
        let mut status = UpdaterPanelStatus { update_available: true, ..Default::default() };
        assert!(!status.user_outdated());
        status.checked_once = true;
        assert!(status.user_outdated());
        status.update_available = false;
        assert!(!status.user_outdated());
    }

    #[test]
    fn platform_status_defaults_hide_optional_wayland_panel() {
        let status = PlatformStatus::default();
        assert!(!status.show_wayland);
        assert!(!status.wayland_active);
        assert!(!status.remote_desktop_active);
        assert!(!status.wayland_busy);
    }

    #[test]
    fn sponsor_url_matches_cpp_settings_link() {
        assert_eq!(SPONSOR_URL, "https://github.com/sponsors/Spencer0187");
    }

    #[test]
    fn clamp_window_position_keeps_in_bounds_position() {
        assert_position_eq(
            clamp_window_position([120.0, 80.0], [400.0, 300.0], [0.0, 0.0], [1180.0, 700.0]),
            [120.0, 80.0],
        );
    }

    #[test]
    fn clamp_window_position_limits_each_viewport_edge() {
        assert_position_eq(
            clamp_window_position([-50.0, 500.0], [400.0, 300.0], [0.0, 0.0], [1180.0, 700.0]),
            [0.0, 400.0],
        );
    }

    #[test]
    fn clamp_window_position_respects_offset_viewport_origin() {
        assert_position_eq(
            clamp_window_position([80.0, 900.0], [400.0, 300.0], [100.0, 200.0], [1180.0, 700.0]),
            [100.0, 600.0],
        );
    }

    #[test]
    fn clamp_window_position_anchors_oversized_window_to_viewport_origin() {
        assert_position_eq(
            clamp_window_position([500.0, 500.0], [1400.0, 900.0], [100.0, 200.0], [1180.0, 700.0]),
            [100.0, 200.0],
        );
    }
}

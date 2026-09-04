use crate::{global_settings::GlobalSettingsState, keybind, theme::ThemeDefinition};
use dear_imgui_rs::{Condition, TreeNodeFlags, Ui, WindowFlags};

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsAction {
    CheckUpdates,
    RequestInstallUpdate,
    ConfirmInstallUpdate,
    SkipUpdate,
    CancelUpdate,
    OpenLatestRelease,
    OpenLatestReleaseAndDismiss,
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
    let position = [display[0] * 0.4, (display[1] - size[1] - 90.0) * 0.5];
    let mut action = None;
    ui.window("Settings Menu")
        .opened(open)
        .size(size, Condition::Always)
        .position(position, Condition::Appearing)
        .flags(WindowFlags::NO_RESIZE | WindowFlags::NO_COLLAPSE)
        .build(|| {
            ui.child_window("SettingsList").border(true).build(ui, || {
                render_updater(ui, updater, &mut action, theme);
                render_wayland(ui, platform, &mut action, theme);
                render_scalar_settings(ui, globals, capture, theme);
            });
        });
    action
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
        ui.text_colored(
            theme.warning_color,
            "This Linux package cannot replace itself. Download the release and reinstall it with the same AppImage, Debian, RPM, portable, or Nix method you used before.",
        );
    }

    if ui.button("Open latest release") {
        *action = Some(SettingsAction::OpenLatestRelease);
    }
    if !updater.action_message.is_empty() {
        ui.text_wrapped(&updater.action_message);
    }
    crate::parity::separator(ui);
}

pub fn render_update_confirmation(ui: &Ui, updater: &UpdaterPanelStatus) -> Option<SettingsAction> {
    if updater.update_confirm_open {
        let _ = ui.open_popup("Update Available");
    }
    let _popup = ui.begin_modal_popup("Update Available")?;

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
        ui.text_wrapped(
            "This Linux installation cannot safely replace itself. Open the official release and update with the same AppImage, Debian, RPM, portable, or Nix method you used before.",
        );
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
        if ui.button_with_size("Cancel", [90.0, 0.0]) {
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
    if ui.button_with_size("Not now", [100.0, 0.0]) {
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
    let disabled =
        (platform.wayland_busy || platform.remote_desktop_active).then(|| ui.begin_disabled());
    if ui.button("Enable Wayland Absolute Mouse Control") {
        *action = Some(SettingsAction::StartWaylandRemoteDesktop);
    }
    drop(disabled);
    crate::parity::separator(ui);
}

fn render_scalar_settings(
    ui: &Ui,
    globals: &mut GlobalSettingsState,
    capture: &mut keybind::CaptureState,
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
    ui.text("Github Doesn't take any of the profits.");
}

#[cfg(test)]
mod tests {
    use super::{PlatformStatus, UpdaterPanelStatus};

    #[test]
    fn updater_status_defaults_are_inert() {
        let status = UpdaterPanelStatus::default();
        assert!(!status.enabled);
        assert!(!status.checked_once);
        assert!(!status.checking);
        assert!(!status.update_available);
    }

    #[test]
    fn platform_status_defaults_hide_optional_wayland_panel() {
        let status = PlatformStatus::default();
        assert!(!status.show_wayland);
        assert!(!status.wayland_active);
        assert!(!status.remote_desktop_active);
        assert!(!status.wayland_busy);
    }
}

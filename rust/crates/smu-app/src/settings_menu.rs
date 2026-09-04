use crate::{global_settings::GlobalSettingsState, keybind};
use dear_imgui_rs::{Condition, Ui, WindowFlags};

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsAction {
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
                render_wayland(ui, platform, &mut action);
                render_scalar_settings(ui, globals, capture);
            });
        });
    action
}

fn render_wayland(ui: &Ui, platform: &PlatformStatus, action: &mut Option<SettingsAction>) {
    if !platform.show_wayland {
        return;
    }
    ui.text("Wayland Screen Capture");
    ui.text_wrapped(
        "Pixel scripts need explicit permission to capture one monitor. The selected monitor becomes the coordinate space for getPixelColor() and getPixelRect() while capture is active.",
    );
    if platform.wayland_supported {
        ui.text_wrapped(format!("Status: {}", platform.wayland_status));
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
            [1.0, 0.20, 0.40, 1.0],
            "This build does not include PipeWire ScreenCast support.",
        );
    }
    crate::parity::separator(ui);

    ui.text("Wayland Absolute Mouse Control");
    ui.text_wrapped(
        "Allow moveMouseAbs() to place the pointer on the selected monitor. This replaces a screen-capture-only session with a RemoteDesktop + ScreenCast session and needs separate pointer-control permission.",
    );
    ui.text_wrapped(format!("Status: {}", platform.wayland_status));
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
    ui.text_colored([0.0, 0.49, 0.62, 1.0], "Want to Donate directly to my Github?");
    ui.text("Github Doesn't take any of the profits.");
}

#[cfg(test)]
mod tests {
    use super::PlatformStatus;

    #[test]
    fn platform_status_defaults_hide_optional_wayland_panel() {
        let status = PlatformStatus::default();
        assert!(!status.show_wayland);
        assert!(!status.wayland_active);
        assert!(!status.remote_desktop_active);
        assert!(!status.wayland_busy);
    }
}

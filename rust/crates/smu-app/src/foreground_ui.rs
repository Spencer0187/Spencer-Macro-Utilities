use dear_imgui_rs::{ItemHoveredFlags, Ui};

const FOREGROUND_FALLBACK_TOOLTIP: &str = "Foreground Roblox window detection is unavailable on this display server. This usually happens on Wayland because apps cannot reliably inspect other apps' active windows. This macro has been forced to always-active mode.";

#[cfg(target_os = "linux")]
#[must_use]
fn fallback_active() -> bool {
    !smu_process::linux::foreground_detection_available()
}

#[cfg(not(target_os = "linux"))]
#[must_use]
const fn fallback_active() -> bool {
    false
}

pub fn render_checkbox(ui: &Ui, label: &str, id: &str, value: &mut bool) -> bool {
    let fallback = fallback_active();
    let mut forced_value = false;
    let disabled = fallback.then(|| ui.begin_disabled());
    ui.text_wrapped(label);
    ui.same_line();
    let checkbox_value = if fallback { &mut forced_value } else { value };
    let changed = ui.checkbox(id, checkbox_value);
    drop(disabled);
    if fallback && ui.is_item_hovered_with_flags(ItemHoveredFlags::ALLOW_WHEN_DISABLED) {
        ui.tooltip_text(FOREGROUND_FALLBACK_TOOLTIP);
    }
    changed
}

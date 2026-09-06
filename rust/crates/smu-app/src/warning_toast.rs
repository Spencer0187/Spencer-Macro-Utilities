use std::collections::VecDeque;

use dear_imgui_rs::{
    Condition, InputTextMultilineFlags, StyleColor, StyleVar, Ui, WindowFlags, sys,
};

pub const PARITY_WARNING_MESSAGE: &str = "Deterministic platform warning parity fixture.";
pub const FOREGROUND_FALLBACK_WARNING: &str = "Foreground-detection-dependent macros are in always-active mode because foreground Roblox window detection is unavailable on Wayland or this specific display server.";

#[derive(Debug, Default)]
pub struct WarningToastState {
    messages: VecDeque<String>,
}

impl WarningToastState {
    pub fn push(&mut self, message: impl Into<String>) {
        self.messages.push_back(message.into());
    }

    #[must_use]
    pub fn front(&self) -> Option<&str> {
        self.messages.front().map(String::as_str)
    }

    pub fn dismiss(&mut self) {
        let _ = self.messages.pop_front();
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.messages.len()
    }
}

pub fn render(ui: &Ui, state: &mut WarningToastState) {
    let Some(message) = state.front().map(str::to_owned) else {
        return;
    };

    let viewport = ui.main_viewport();
    let work_pos = viewport.work_pos();
    let work_size = viewport.work_size();
    let pos = [work_pos[0] + work_size[0] - 24.0, work_pos[1] + 24.0];
    // SAFETY: this sets the position for the immediately following window Begin while `ui` owns
    // the active Dear ImGui context for this frame.
    unsafe {
        sys::igSetNextWindowPos(
            sys::ImVec2_c { x: pos[0], y: pos[1] },
            Condition::Always as i32,
            sys::ImVec2_c { x: 1.0, y: 0.0 },
        );
    }

    let flags = WindowFlags::NO_DECORATION
        | WindowFlags::NO_SAVED_SETTINGS
        | WindowFlags::ALWAYS_AUTO_RESIZE
        | WindowFlags::NO_FOCUS_ON_APPEARING
        | WindowFlags::NO_NAV;

    ui.window("Platform Warnings")
        .size([420.0, 0.0], Condition::Always)
        .bg_alpha(0.96)
        .flags(flags)
        .build(|| {
            ui.text("Warning");
            crate::parity::separator(ui);
            render_selectable_message(ui, &message);
            if ui.button_with_size("Dismiss", [100.0, 0.0]) {
                state.dismiss();
            }
            if state.len() > 1 {
                ui.same_line();
                ui.text(format!("{} more", state.len() - 1));
            }
        });
}

fn render_selectable_message(ui: &Ui, message: &str) {
    let available_width = ui.content_region_avail()[0].max(1.0);
    let text_size = ui.calc_text_size_with_opts(message, false, available_width);
    let style = ui.clone_style();
    let height = style.frame_padding()[1].mul_add(2.0, text_size[1]).clamp(56.0, 220.0);

    let frame_padding = ui.push_style_var(StyleVar::FramePadding([0.0, 0.0]));
    let frame_border = ui.push_style_var(StyleVar::FrameBorderSize(0.0));
    let frame_bg = ui.push_style_color(StyleColor::FrameBg, [0.0, 0.0, 0.0, 0.0]);
    let frame_bg_hovered = ui.push_style_color(StyleColor::FrameBgHovered, [0.0, 0.0, 0.0, 0.0]);
    let frame_bg_active = ui.push_style_color(StyleColor::FrameBgActive, [0.0, 0.0, 0.0, 0.0]);
    let border = ui.push_style_color(StyleColor::Border, [0.0, 0.0, 0.0, 0.0]);

    let mut buffer = message.to_owned();
    let flags = InputTextMultilineFlags::READ_ONLY
        | InputTextMultilineFlags::NO_HORIZONTAL_SCROLL
        | InputTextMultilineFlags::WORD_WRAP;
    let _ = ui
        .input_text_multiline("##toast_message", &mut buffer, [-f32::MIN_POSITIVE, height])
        .flags(flags)
        .build();

    drop(border);
    drop(frame_bg_active);
    drop(frame_bg_hovered);
    drop(frame_bg);
    drop(frame_border);
    drop(frame_padding);
}

#[cfg(test)]
mod tests {
    use super::WarningToastState;

    #[test]
    fn fifo_preserves_warning_order_and_dismissal() {
        let mut state = WarningToastState::default();
        state.push("first");
        state.push("second");
        assert_eq!(state.front(), Some("first"));
        assert_eq!(state.len(), 2);
        state.dismiss();
        assert_eq!(state.front(), Some("second"));
        assert_eq!(state.len(), 1);
    }
}

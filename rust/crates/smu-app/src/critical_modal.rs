use dear_imgui_rs::{Condition, Ui, WindowFlags, sys};
use std::collections::VecDeque;

pub const PARITY_CASE: &str = "critical-platform-error-user-reference";
pub const PARITY_MESSAGE: &str = "Parity critical platform failure.";
pub const SUPPORT_TEXT: &str =
    "Join the Discord for help: https://discord.gg/roblox-glitching-community-998572881892094012";

#[derive(Debug, Default)]
pub struct CriticalModalState {
    messages: VecDeque<String>,
}

impl CriticalModalState {
    pub fn push(&mut self, message: impl Into<String>) {
        let message = message.into();
        if message.contains(SUPPORT_TEXT) {
            self.messages.push_back(message);
        } else {
            self.messages.push_back(format!("{message}\n\n{SUPPORT_TEXT}"));
        }
    }

    #[cfg(test)]
    pub fn front(&self) -> Option<&str> {
        self.messages.front().map(String::as_str)
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.messages.len()
    }
}

fn set_next_modal_layout(ui: &Ui) {
    let viewport = ui.main_viewport();
    let pos = viewport.pos();
    let size = viewport.size();
    let center = [size[0].mul_add(0.5, pos[0]), size[1].mul_add(0.5, pos[1])];
    // SAFETY: these calls configure the immediately following modal Begin while `ui` owns the
    // active Dear ImGui context for this frame.
    unsafe {
        sys::igSetNextWindowPos(
            sys::ImVec2_c { x: center[0], y: center[1] },
            Condition::Appearing as i32,
            sys::ImVec2_c { x: 0.5, y: 0.5 },
        );
        sys::igSetNextWindowSize(sys::ImVec2_c { x: 560.0, y: 0.0 }, Condition::Appearing as i32);
    }
}

pub fn render(ui: &Ui, state: &mut CriticalModalState) {
    if state.messages.is_empty() {
        return;
    }

    let _ = ui.open_popup("Critical Platform Error");
    set_next_modal_layout(ui);
    let Some(_popup) = ui
        .begin_modal_popup_config("Critical Platform Error")
        .flags(WindowFlags::ALWAYS_AUTO_RESIZE)
        .begin()
    else {
        return;
    };

    ui.text_wrapped("A required platform feature failed to initialize.");
    crate::parity::separator(ui);
    {
        let _wrap = ui.push_text_wrap_pos(ui.cursor_pos()[0] + 520.0);
        if let Some(message) = state.messages.front() {
            ui.text(message);
        }
    }
    crate::parity::separator(ui);
    let label = if state.messages.len() > 1 { "Next" } else { "Dismiss" };
    if ui.button_with_size(label, [120.0, 0.0]) {
        state.messages.pop_front();
        if state.messages.is_empty() {
            ui.close_current_popup();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CriticalModalState, PARITY_MESSAGE, SUPPORT_TEXT};

    #[test]
    fn critical_messages_append_support_text_once_and_preserve_fifo_order() {
        let mut state = CriticalModalState::default();
        state.push(PARITY_MESSAGE);
        state.push(format!("second\n\n{SUPPORT_TEXT}"));
        assert_eq!(state.len(), 2);
        let expected = format!("{PARITY_MESSAGE}\n\n{SUPPORT_TEXT}");
        assert_eq!(state.front(), Some(expected.as_str()));
    }
}

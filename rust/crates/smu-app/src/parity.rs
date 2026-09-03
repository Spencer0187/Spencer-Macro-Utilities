use dear_imgui_rs::Ui;

/// Current `dear-imgui-rs` (Dear `ImGui` 1.92.9b) advances one logical pixel farther around a
/// separator than SMU's C++ Dear `ImGui` 1.92.5 oracle in the same SDL/OpenGL framebuffer.
///
/// Keep this compensation centralized and covered by screenshot tests. It is compatibility data,
/// not a general layout preference, and should be re-measured whenever either Dear `ImGui` version
/// changes. Callers must submit another item after this helper; a terminal separator should use
/// `Ui::separator` directly so Dear `ImGui`'s parent-boundary safety check remains satisfied.
const SEPARATOR_VERTICAL_COMPENSATION: f32 = 1.0;

pub fn separator(ui: &Ui) {
    ui.separator();
    ui.set_cursor_pos_y(ui.cursor_pos_y() - SEPARATOR_VERTICAL_COMPENSATION);
}

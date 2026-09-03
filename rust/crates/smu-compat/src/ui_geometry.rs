//! Exact shell-layout equations extracted from the current C++ `ImGui` oracle.
//!
//! The Rust UI can implement the layout differently, but these equations define geometry that must
//! remain visually compatible during the rewrite.

/// Current C++ shell constants, named here instead of repeating magic numbers.
pub mod oracle {
    pub const SETTINGS_HORIZONTAL_MARGIN: f32 = 16.0;
    pub const SETTINGS_EXTRA_HEIGHT: f32 = 12.0;
    pub const MAIN_HORIZONTAL_ADJUSTMENT: f32 = 23.0;
    pub const MAIN_VERTICAL_ADJUSTMENT: f32 = 20.0;
    pub const LEFT_WIDTH_FRACTION: f32 = 0.30;
    pub const FOOTER_HEIGHT: f32 = 30.0;
    pub const FOOTER_BORDER_INSET: f32 = 1.0;
    pub const FOOTER_CONTROLS_FROM_RIGHT: f32 = 602.0;
    pub const FOOTER_CONTROL_Y_OFFSET: f32 = 3.0;
    pub const PROFILES_WIDTH: f32 = 125.0;
    pub const SETTINGS_TEXT_ROWS: f32 = 4.0;
    pub const SETTINGS_PADDING_ROWS: f32 = 3.0;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShellInput {
    pub display_width: f32,
    pub display_height: f32,
    pub text_line_height_with_spacing: f32,
    pub window_padding_y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShellGeometry {
    pub settings_width: f32,
    pub settings_height: f32,
    pub left_width: f32,
    pub left_height: f32,
    pub right_width: f32,
    pub right_height: f32,
    pub footer_height: f32,
    pub footer_y: f32,
    pub footer_width: f32,
    pub footer_controls_x: f32,
    pub replacement_bottom_border_offset: f32,
}

/// Reproduces the arithmetic in the current C++ main-shell layout.
///
/// The arithmetic intentionally mirrors the oracle expression order instead of using fused
/// multiply-add operations, because one-unit layout parity is more important here than a possible
/// floating-point micro-optimization.
#[must_use]
#[allow(clippy::suboptimal_flops)]
pub fn cpp_shell_geometry(input: ShellInput) -> ShellGeometry {
    let settings_height = oracle::SETTINGS_TEXT_ROWS * input.text_line_height_with_spacing
        + oracle::SETTINGS_PADDING_ROWS * input.window_padding_y
        + oracle::SETTINGS_EXTRA_HEIGHT;
    let left_width =
        oracle::LEFT_WIDTH_FRACTION * input.display_width - oracle::MAIN_HORIZONTAL_ADJUSTMENT;
    let left_height = input.display_height - settings_height - oracle::MAIN_VERTICAL_ADJUSTMENT;
    let right_width = input.display_width - oracle::MAIN_HORIZONTAL_ADJUSTMENT - left_width;
    let right_height = left_height - oracle::FOOTER_HEIGHT;
    let footer_width = right_width - oracle::FOOTER_BORDER_INSET;

    ShellGeometry {
        settings_width: input.display_width - oracle::SETTINGS_HORIZONTAL_MARGIN,
        settings_height,
        left_width,
        left_height,
        right_width,
        right_height,
        footer_height: oracle::FOOTER_HEIGHT,
        footer_y: input.display_height - oracle::FOOTER_HEIGHT - input.window_padding_y,
        footer_width,
        footer_controls_x: footer_width - oracle::FOOTER_CONTROLS_FROM_RIGHT,
        replacement_bottom_border_offset: oracle::FOOTER_HEIGHT - oracle::FOOTER_BORDER_INSET,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)]

    use super::{ShellInput, cpp_shell_geometry, oracle};

    #[test]
    fn footer_replacement_border_is_29_units_below_child_bottom() {
        let geometry = cpp_shell_geometry(ShellInput {
            display_width: 1280.0,
            display_height: 800.0,
            text_line_height_with_spacing: 24.0,
            window_padding_y: 8.0,
        });
        assert_eq!(geometry.replacement_bottom_border_offset, 29.0);
    }

    #[test]
    fn right_width_equation_preserves_cpp_one_unit_border_compensation() {
        let geometry = cpp_shell_geometry(ShellInput {
            display_width: 1280.0,
            display_height: 800.0,
            text_line_height_with_spacing: 24.0,
            window_padding_y: 8.0,
        });
        assert_eq!(geometry.left_width, 361.0);
        assert_eq!(geometry.right_width, 896.0);
        assert_eq!(geometry.footer_width, 895.0);
        assert_eq!(
            geometry.replacement_bottom_border_offset,
            oracle::FOOTER_HEIGHT - oracle::FOOTER_BORDER_INSET
        );
    }
}

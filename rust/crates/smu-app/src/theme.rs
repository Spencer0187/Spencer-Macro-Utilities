use std::{error::Error, path::PathBuf};

use dear_imgui_rs::{
    ColorOverride, Context, FontConfig, FontSource, StbTrueTypeFontData, StyleColor, StyleTweaks,
    Theme, ThemePreset,
};

const FONT_SIZE: f32 = 20.0;
const FONT_OVERSAMPLE_H: i8 = 3;
const FONT_OVERSAMPLE_V: i8 = 1;

const CYBERPUNK_BG_DARK: [f32; 4] = [0.05, 0.05, 0.10, 1.0];
const CYBERPUNK_BG_MEDIUM: [f32; 4] = [0.10, 0.10, 0.15, 1.0];
const CYBERPUNK_BG_LIGHT: [f32; 4] = [0.15, 0.15, 0.20, 1.0];
const CYBERPUNK_ACCENT_PRIMARY: [f32; 4] = [0.0, 0.49, 0.62, 1.0];
const CYBERPUNK_ACCENT_SECONDARY: [f32; 4] = [0.0, 0.44, 0.60, 1.0];
const CYBERPUNK_TEXT_PRIMARY: [f32; 4] = [0.90, 0.90, 1.0, 1.0];
const CYBERPUNK_TEXT_SECONDARY: [f32; 4] = [0.60, 0.60, 0.80, 1.0];
const CYBERPUNK_SUCCESS: [f32; 4] = [0.0, 1.0, 0.50, 1.0];
const CYBERPUNK_BORDER: [f32; 4] = [0.0, 0.40, 0.60, 1.0];

pub fn configure(context: &mut Context) -> Result<(), Box<dyn Error>> {
    load_lsans(context)?;
    cyberpunk_theme().apply_to_context(context);
    Ok(())
}

fn load_lsans(context: &Context) -> Result<(), Box<dyn Error>> {
    let path = source_font_path();
    let font = StbTrueTypeFontData::from_file(&path)?;
    let config = FontConfig::new()
        .oversample_h(FONT_OVERSAMPLE_H)
        .oversample_v(FONT_OVERSAMPLE_V)
        .pixel_snap_h(false);
    let source = FontSource::stb_truetype_with_size(font, FONT_SIZE).with_config(config);
    context.font_atlas().add_font(&[source]);
    Ok(())
}

fn source_font_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../assets/LSANS.TTF")
}

fn cyberpunk_theme() -> Theme {
    let frame_hovered = [
        (CYBERPUNK_BG_LIGHT[0] + 0.1).min(1.0),
        (CYBERPUNK_BG_LIGHT[1] + 0.1).min(1.0),
        (CYBERPUNK_BG_LIGHT[2] + 0.1).min(1.0),
        CYBERPUNK_BG_LIGHT[3],
    ];

    Theme {
        preset: ThemePreset::Dark,
        colors: vec![
            color(StyleColor::WindowBg, CYBERPUNK_BG_DARK),
            color(StyleColor::ChildBg, CYBERPUNK_BG_MEDIUM),
            color(StyleColor::PopupBg, CYBERPUNK_BG_MEDIUM),
            color(StyleColor::Border, CYBERPUNK_BORDER),
            color(StyleColor::BorderShadow, [0.0, 0.0, 0.0, 0.0]),
            color(StyleColor::FrameBg, CYBERPUNK_BG_LIGHT),
            color(StyleColor::FrameBgHovered, frame_hovered),
            color(StyleColor::FrameBgActive, CYBERPUNK_ACCENT_PRIMARY),
            color(StyleColor::TitleBg, CYBERPUNK_BG_DARK),
            color(StyleColor::TitleBgActive, CYBERPUNK_BG_DARK),
            color(StyleColor::TitleBgCollapsed, CYBERPUNK_BG_DARK),
            color(StyleColor::MenuBarBg, CYBERPUNK_BG_MEDIUM),
            color(StyleColor::ScrollbarBg, CYBERPUNK_BG_MEDIUM),
            color(StyleColor::ScrollbarGrab, CYBERPUNK_BG_LIGHT),
            color(StyleColor::ScrollbarGrabHovered, CYBERPUNK_ACCENT_PRIMARY),
            color(StyleColor::ScrollbarGrabActive, CYBERPUNK_ACCENT_SECONDARY),
            color(StyleColor::CheckMark, CYBERPUNK_SUCCESS),
            // Dear ImGui 1.92.9 introduced a separate checked-checkbox background. SMU's 1.92.5
            // rendered checked boxes with FrameBg, so pin the new slot to that same color.
            color(StyleColor::CheckboxSelectedBg, CYBERPUNK_BG_LIGHT),
            color(StyleColor::SliderGrab, CYBERPUNK_ACCENT_PRIMARY),
            color(StyleColor::SliderGrabActive, CYBERPUNK_ACCENT_SECONDARY),
            color(StyleColor::Button, CYBERPUNK_ACCENT_PRIMARY),
            color(StyleColor::ButtonHovered, CYBERPUNK_ACCENT_SECONDARY),
            color(StyleColor::ButtonActive, CYBERPUNK_ACCENT_SECONDARY),
            color(StyleColor::Header, CYBERPUNK_ACCENT_PRIMARY),
            color(StyleColor::HeaderHovered, CYBERPUNK_ACCENT_SECONDARY),
            color(StyleColor::HeaderActive, CYBERPUNK_ACCENT_SECONDARY),
            color(StyleColor::Separator, CYBERPUNK_BORDER),
            color(StyleColor::SeparatorHovered, CYBERPUNK_ACCENT_PRIMARY),
            color(StyleColor::SeparatorActive, CYBERPUNK_ACCENT_SECONDARY),
            color(StyleColor::ResizeGrip, CYBERPUNK_ACCENT_PRIMARY),
            color(StyleColor::ResizeGripHovered, CYBERPUNK_ACCENT_SECONDARY),
            color(StyleColor::ResizeGripActive, CYBERPUNK_ACCENT_SECONDARY),
            color(StyleColor::Tab, CYBERPUNK_BG_LIGHT),
            color(StyleColor::TabHovered, CYBERPUNK_ACCENT_PRIMARY),
            color(StyleColor::TabSelected, CYBERPUNK_ACCENT_PRIMARY),
            color(StyleColor::TabDimmed, CYBERPUNK_BG_LIGHT),
            color(StyleColor::TabDimmedSelected, CYBERPUNK_ACCENT_PRIMARY),
            color(StyleColor::Text, CYBERPUNK_TEXT_PRIMARY),
            color(StyleColor::TextDisabled, CYBERPUNK_TEXT_SECONDARY),
            color(StyleColor::NavCursor, CYBERPUNK_ACCENT_PRIMARY),
            color(StyleColor::ModalWindowDimBg, [0.0, 0.0, 0.0, 0.60]),
        ],
        style: StyleTweaks {
            window_rounding: Some(6.0),
            child_rounding: Some(4.5),
            frame_rounding: Some(3.0),
            popup_rounding: Some(4.5),
            scrollbar_rounding: Some(3.0),
            grab_rounding: Some(3.0),
            tab_rounding: Some(3.0),
            ..StyleTweaks::default()
        },
        ..Theme::default()
    }
}

const fn color(id: StyleColor, rgba: [f32; 4]) -> ColorOverride {
    ColorOverride { id, rgba }
}

#[cfg(test)]
mod tests {
    use super::source_font_path;

    #[test]
    fn source_font_fixture_exists() {
        assert!(source_font_path().is_file());
    }
}

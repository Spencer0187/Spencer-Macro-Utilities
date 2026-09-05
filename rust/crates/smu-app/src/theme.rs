use std::{
    env,
    error::Error,
    path::{Path, PathBuf},
};

use dear_imgui_rs::{
    ColorOverride, Condition, Context, FontConfig, FontSource, NumericFormat, StbTrueTypeFontData,
    StyleColor, StyleTweaks, Theme, ThemePreset, TreeNodeFlags, Ui, WindowFlags,
};
use serde_json::{Map, Value, json};

const FONT_SIZE: f32 = 20.0;
const FONT_OVERSAMPLE_H: i8 = 3;
const FONT_OVERSAMPLE_V: i8 = 1;
const CYBERPUNK_INDEX: usize = 2;
const CUSTOM_THEME_NAME: &str = "Custom Theme";

#[derive(Clone, Debug, PartialEq)]
pub struct ThemeDefinition {
    pub name: String,
    pub bg_dark: [f32; 4],
    pub bg_medium: [f32; 4],
    pub bg_light: [f32; 4],
    pub accent_primary: [f32; 4],
    pub accent_secondary: [f32; 4],
    pub text_primary: [f32; 4],
    pub text_secondary: [f32; 4],
    pub success_color: [f32; 4],
    pub warning_color: [f32; 4],
    pub error_color: [f32; 4],
    pub border_color: [f32; 4],
    pub disabled_color: [f32; 4],
    pub window_rounding: f32,
    pub frame_rounding: f32,
    pub button_rounding: f32,
}

#[derive(Clone, Debug)]
pub struct ThemeState {
    defaults: Vec<ThemeDefinition>,
    themes: Vec<ThemeDefinition>,
    custom_theme: ThemeDefinition,
    current_index: usize,
    editor_open: bool,
    editor_position: Option<[f32; 2]>,
}

impl Default for ThemeState {
    fn default() -> Self {
        let defaults = default_themes();
        let mut custom_theme = defaults[0].clone();
        CUSTOM_THEME_NAME.clone_into(&mut custom_theme.name);
        Self {
            themes: defaults.clone(),
            defaults,
            custom_theme,
            current_index: CYBERPUNK_INDEX,
            editor_open: false,
            editor_position: None,
        }
    }
}

impl ThemeState {
    #[must_use]
    pub fn active(&self) -> &ThemeDefinition {
        self.themes.get(self.current_index).unwrap_or(&self.custom_theme)
    }

    #[must_use]
    pub const fn editor_open(&self) -> bool {
        self.editor_open
    }

    pub const fn toggle_editor(&mut self) {
        self.editor_open = !self.editor_open;
        if !self.editor_open {
            self.editor_position = None;
        }
    }

    pub fn apply_to_context(&self, context: &mut Context) {
        imgui_theme(self.active()).apply_to_context(context);
    }

    pub fn apply_metadata(&mut self, metadata: &Map<String, Value>) {
        if let Some(index) = metadata.get("current_theme_index").and_then(cpp_theme_index) {
            self.current_index = index;
        }

        if let Some(custom) = metadata.get("custom_theme") {
            self.custom_theme = deserialize_theme(custom, CUSTOM_THEME_NAME);
        }

        if let Some(saved_themes) = metadata.get("themes").and_then(Value::as_object) {
            for (fallback_name, saved_theme) in saved_themes {
                let loaded = deserialize_theme(saved_theme, fallback_name);
                if let Some(existing) =
                    self.themes.iter_mut().find(|theme| theme.name == loaded.name)
                {
                    *existing = loaded;
                } else {
                    self.themes.push(loaded);
                }
            }
        }

        if self.current_index > self.themes.len() {
            self.current_index =
                self.themes.iter().position(|theme| theme.name == "Cyberpunk").unwrap_or(0);
        }
        // C++ saves `show_theme_menu` but never reads it. Keep the editor closed on load.
        self.editor_open = false;
        self.editor_position = None;
    }

    #[must_use]
    pub fn metadata_overlay(&self) -> Map<String, Value> {
        let mut metadata = Map::new();
        metadata.insert("current_theme_index".to_owned(), json!(self.current_index));
        metadata.insert("show_theme_menu".to_owned(), json!(self.editor_open));
        metadata.insert("custom_theme".to_owned(), serialize_theme(&self.custom_theme));
        let themes = self
            .themes
            .iter()
            .map(|theme| (theme.name.clone(), serialize_theme(theme)))
            .collect::<Map<String, Value>>();
        metadata.insert("themes".to_owned(), Value::Object(themes));
        metadata
    }

    pub fn render_editor(&mut self, ui: &Ui) {
        if !self.editor_open {
            self.editor_position = None;
            return;
        }

        let display = ui.io().display_size();
        let size = [display[0] * 0.5, display[1] * 0.7];
        let fallback_position = [display[0] * 0.3, display[1] - size[1] - 90.0];
        let requested =
            clamp_window_position(self.editor_position.unwrap_or(fallback_position), size, display);
        let mut open = self.editor_open;
        ui.window("Theme Editor")
            .opened(&mut open)
            .position(requested, Condition::Appearing)
            .size(size, Condition::Always)
            .flags(WindowFlags::NO_RESIZE | WindowFlags::NO_COLLAPSE)
            .build(|| self.render_editor_contents(ui));
        self.editor_open = open;
        if !open {
            self.editor_position = None;
        }
    }

    fn render_editor_contents(&mut self, ui: &Ui) {
        let current_position = ui.window_pos();
        let current_size = ui.window_size();
        let clamped = clamp_window_position(current_position, current_size, ui.io().display_size());
        if clamped[0].to_bits() != current_position[0].to_bits()
            || clamped[1].to_bits() != current_position[1].to_bits()
        {
            ui.set_window_pos(clamped);
        }
        self.editor_position = Some(clamped);

        ui.text_colored(self.active().text_primary, "SELECT THEME");
        crate::parity::separator(ui);
        let mut names = self.themes.iter().map(|theme| theme.name.clone()).collect::<Vec<_>>();
        names.push(self.custom_theme.name.clone());
        let _ = ui.combo_simple_string("Theme", &mut self.current_index, &names);

        ui.new_line();
        ui.text_colored(self.active().text_primary, "CUSTOMIZE COLORS");
        crate::parity::separator(ui);

        let is_custom = self.current_index >= self.themes.len();
        let current_index = self.current_index;
        ui.child_window("ColorEditor").size([0.0, -35.0]).border(true).build(ui, || {
            let editing =
                if is_custom { &mut self.custom_theme } else { &mut self.themes[current_index] };
            render_theme_fields(ui, editing, is_custom);
        });

        if ui.button_with_size("Reset to Default", [150.0, 30.0]) {
            if is_custom {
                self.custom_theme = self.defaults[0].clone();
                CUSTOM_THEME_NAME.clone_into(&mut self.custom_theme.name);
            } else if let Some(default) = self.defaults.get(current_index) {
                self.themes[current_index] = default.clone();
            }
        }
    }
}

pub fn load_fonts(context: &Context) -> Result<(), Box<dyn Error>> {
    load_lsans(context)
}

fn render_theme_fields(ui: &Ui, theme: &mut ThemeDefinition, is_custom: bool) {
    if ui.collapsing_header("Background Colors", TreeNodeFlags::DEFAULT_OPEN) {
        let _ = ui.color_edit4("Dark Background", &mut theme.bg_dark);
        let _ = ui.color_edit4("Medium Background", &mut theme.bg_medium);
        let _ = ui.color_edit4("Light Background", &mut theme.bg_light);
    }
    if ui.collapsing_header("Accent Colors", TreeNodeFlags::DEFAULT_OPEN) {
        let _ = ui.color_edit4("Primary Accent", &mut theme.accent_primary);
        let _ = ui.color_edit4("Secondary Accent", &mut theme.accent_secondary);
    }
    if ui.collapsing_header("Text & Status", TreeNodeFlags::NONE) {
        let _ = ui.color_edit4("Primary Text", &mut theme.text_primary);
        let _ = ui.color_edit4("Secondary Text", &mut theme.text_secondary);
        let _ = ui.color_edit4("Success", &mut theme.success_color);
        let _ = ui.color_edit4("Warning", &mut theme.warning_color);
        let _ = ui.color_edit4("Error", &mut theme.error_color);
        let _ = ui.color_edit4("Border", &mut theme.border_color);
        let _ = ui.color_edit4("Disabled", &mut theme.disabled_color);
    }
    if ui.collapsing_header("Rounding", TreeNodeFlags::DEFAULT_OPEN) {
        rounding_slider(ui, "Window Rounding", &mut theme.window_rounding);
        rounding_slider(ui, "Frame Rounding", &mut theme.frame_rounding);
        rounding_slider(ui, "Button Rounding", &mut theme.button_rounding);
    }
    ui.new_line();
    if is_custom {
        ui.text("Custom Theme Name");
        let _ = ui.input_text("##ThemeName", &mut theme.name).build();
    }
}

fn rounding_slider(ui: &Ui, label: &str, value: &mut f32) {
    let slider = ui.slider_config(label, 0.0_f32, 20.0_f32);
    match NumericFormat::new("%.1f") {
        Ok(format) => {
            let _ = slider.display_format(format).build(value);
        }
        Err(_) => {
            let _ = slider.build(value);
        }
    }
}

fn load_lsans(context: &Context) -> Result<(), Box<dyn Error>> {
    let path = font_path();
    let font = StbTrueTypeFontData::from_file(&path)?;
    let config = FontConfig::new()
        .oversample_h(FONT_OVERSAMPLE_H)
        .oversample_v(FONT_OVERSAMPLE_V)
        .pixel_snap_h(false);
    let source = FontSource::stb_truetype_with_size(font, FONT_SIZE).with_config(config);
    context.font_atlas().add_font(&[source]);
    Ok(())
}

fn font_path() -> PathBuf {
    let executable_directory =
        env::current_exe().ok().and_then(|path| path.parent().map(Path::to_path_buf));
    let app_directories = ["SMU_APPDIR", "APPDIR"]
        .into_iter()
        .filter_map(env::var_os)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    resolve_font_path_from(
        executable_directory.as_deref(),
        &app_directories,
        &source_root(),
        &env::current_dir().unwrap_or_default(),
    )
}

fn source_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn resolve_font_path_from(
    executable_directory: Option<&Path>,
    app_directories: &[PathBuf],
    source_root: &Path,
    current_directory: &Path,
) -> PathBuf {
    let mut candidates = Vec::new();
    if let Some(executable_directory) = executable_directory {
        candidates.push(executable_directory.join("assets/LSANS.TTF"));
    }
    for app_directory in app_directories {
        candidates.push(app_directory.join("usr/bin/assets/LSANS.TTF"));
        candidates.push(app_directory.join("assets/LSANS.TTF"));
    }
    candidates.push(source_root.join("assets/LSANS.TTF"));
    candidates.push(current_directory.join("assets/LSANS.TTF"));
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| source_root.join("assets/LSANS.TTF"))
}

fn imgui_theme(theme: &ThemeDefinition) -> Theme {
    let frame_hovered = [
        (theme.bg_light[0] + 0.1).min(1.0),
        (theme.bg_light[1] + 0.1).min(1.0),
        (theme.bg_light[2] + 0.1).min(1.0),
        theme.bg_light[3],
    ];
    Theme {
        preset: ThemePreset::Dark,
        colors: vec![
            color(StyleColor::WindowBg, theme.bg_dark),
            color(StyleColor::ChildBg, theme.bg_medium),
            color(StyleColor::PopupBg, theme.bg_medium),
            color(StyleColor::Border, theme.border_color),
            color(StyleColor::BorderShadow, [0.0, 0.0, 0.0, 0.0]),
            color(StyleColor::FrameBg, theme.bg_light),
            color(StyleColor::FrameBgHovered, frame_hovered),
            color(StyleColor::FrameBgActive, theme.accent_primary),
            color(StyleColor::TitleBg, theme.bg_dark),
            color(StyleColor::TitleBgActive, theme.bg_dark),
            color(StyleColor::TitleBgCollapsed, theme.bg_dark),
            color(StyleColor::MenuBarBg, theme.bg_medium),
            color(StyleColor::ScrollbarBg, theme.bg_medium),
            color(StyleColor::ScrollbarGrab, theme.bg_light),
            color(StyleColor::ScrollbarGrabHovered, theme.accent_primary),
            color(StyleColor::ScrollbarGrabActive, theme.accent_secondary),
            color(StyleColor::CheckMark, theme.success_color),
            // Dear ImGui 1.92.9 introduced a separate checked-checkbox background. SMU 1.92.5
            // rendered checked boxes with FrameBg, so pin the new slot to the same theme color.
            color(StyleColor::CheckboxSelectedBg, theme.bg_light),
            color(StyleColor::SliderGrab, theme.accent_primary),
            color(StyleColor::SliderGrabActive, theme.accent_secondary),
            color(StyleColor::Button, theme.accent_primary),
            color(StyleColor::ButtonHovered, theme.accent_secondary),
            color(StyleColor::ButtonActive, theme.accent_secondary),
            color(StyleColor::Header, theme.accent_primary),
            color(StyleColor::HeaderHovered, theme.accent_secondary),
            color(StyleColor::HeaderActive, theme.accent_secondary),
            color(StyleColor::Separator, theme.border_color),
            color(StyleColor::SeparatorHovered, theme.accent_primary),
            color(StyleColor::SeparatorActive, theme.accent_secondary),
            color(StyleColor::ResizeGrip, theme.accent_primary),
            color(StyleColor::ResizeGripHovered, theme.accent_secondary),
            color(StyleColor::ResizeGripActive, theme.accent_secondary),
            color(StyleColor::Tab, theme.bg_light),
            color(StyleColor::TabHovered, theme.accent_primary),
            color(StyleColor::TabSelected, theme.accent_primary),
            color(StyleColor::TabDimmed, theme.bg_light),
            color(StyleColor::TabDimmedSelected, theme.accent_primary),
            color(StyleColor::Text, theme.text_primary),
            color(StyleColor::TextDisabled, theme.text_secondary),
            color(StyleColor::NavCursor, theme.accent_primary),
            color(StyleColor::ModalWindowDimBg, [0.0, 0.0, 0.0, 0.60]),
        ],
        style: StyleTweaks {
            window_rounding: Some(theme.window_rounding),
            child_rounding: Some(theme.window_rounding * 0.75),
            frame_rounding: Some(theme.frame_rounding),
            popup_rounding: Some(theme.window_rounding * 0.75),
            scrollbar_rounding: Some(theme.frame_rounding),
            grab_rounding: Some(theme.button_rounding),
            tab_rounding: Some(theme.frame_rounding),
            ..StyleTweaks::default()
        },
        ..Theme::default()
    }
}

const fn color(id: StyleColor, rgba: [f32; 4]) -> ColorOverride {
    ColorOverride { id, rgba }
}

fn serialize_theme(theme: &ThemeDefinition) -> Value {
    json!({
        "name": theme.name,
        "bg_dark": color_json(theme.bg_dark),
        "bg_medium": color_json(theme.bg_medium),
        "bg_light": color_json(theme.bg_light),
        "accent_primary": color_json(theme.accent_primary),
        "accent_secondary": color_json(theme.accent_secondary),
        "text_primary": color_json(theme.text_primary),
        "text_secondary": color_json(theme.text_secondary),
        "success_color": color_json(theme.success_color),
        "warning_color": color_json(theme.warning_color),
        "error_color": color_json(theme.error_color),
        "border_color": color_json(theme.border_color),
        "disabled_color": color_json(theme.disabled_color),
        "window_rounding": round_cpp(theme.window_rounding),
        "frame_rounding": round_cpp(theme.frame_rounding),
        "button_rounding": round_cpp(theme.button_rounding),
    })
}

fn deserialize_theme(value: &Value, fallback_name: &str) -> ThemeDefinition {
    let object = value.as_object();
    let name = object
        .and_then(|object| object.get("name"))
        .and_then(Value::as_str)
        .unwrap_or(fallback_name)
        .to_owned();
    ThemeDefinition {
        name,
        bg_dark: json_color(object, "bg_dark"),
        bg_medium: json_color(object, "bg_medium"),
        bg_light: json_color(object, "bg_light"),
        accent_primary: json_color(object, "accent_primary"),
        accent_secondary: json_color(object, "accent_secondary"),
        text_primary: json_color(object, "text_primary"),
        text_secondary: json_color(object, "text_secondary"),
        success_color: json_color(object, "success_color"),
        warning_color: json_color(object, "warning_color"),
        error_color: json_color(object, "error_color"),
        border_color: json_color(object, "border_color"),
        disabled_color: json_color(object, "disabled_color"),
        window_rounding: json_f32(object, "window_rounding"),
        frame_rounding: json_f32(object, "frame_rounding"),
        button_rounding: json_f32(object, "button_rounding"),
    }
}

fn json_color(object: Option<&Map<String, Value>>, key: &str) -> [f32; 4] {
    let Some(values) = object
        .and_then(|object| object.get(key))
        .and_then(Value::as_array)
        .filter(|values| values.len() == 4)
    else {
        return [0.0, 0.0, 0.0, 1.0];
    };
    let mut result = [0.0_f32; 4];
    for (destination, value) in result.iter_mut().zip(values) {
        let Some(value) = value.as_f64() else {
            return [0.0, 0.0, 0.0, 1.0];
        };
        *destination = f64_to_f32(value);
    }
    result
}

fn json_f32(object: Option<&Map<String, Value>>, key: &str) -> f32 {
    object.and_then(|object| object.get(key)).and_then(Value::as_f64).map_or(0.0, f64_to_f32)
}

fn color_json(color: [f32; 4]) -> Value {
    Value::Array(color.into_iter().map(|component| json!(round_cpp(component))).collect())
}

fn round_cpp(value: f32) -> f64 {
    let scaled = value.mul_add(1000.0, 0.5);
    f64::from(scaled).floor() / 1000.0
}

#[allow(clippy::cast_possible_truncation)]
const fn f64_to_f32(value: f64) -> f32 {
    value as f32
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn cpp_theme_index(value: &Value) -> Option<usize> {
    if let Some(index) = value.as_i64() {
        return Some(usize::try_from(index).unwrap_or(usize::MAX));
    }
    if let Some(index) = value.as_u64() {
        return Some(usize::try_from(index).unwrap_or(usize::MAX));
    }
    let index = value.as_f64()?;
    if !index.is_finite() || index < 0.0 {
        return Some(usize::MAX);
    }
    Some(index.trunc() as usize)
}

fn clamp_window_position(position: [f32; 2], size: [f32; 2], display: [f32; 2]) -> [f32; 2] {
    [
        position[0].clamp(0.0, (display[0] - size[0]).max(0.0)),
        position[1].clamp(0.0, (display[1] - size[1]).max(0.0)),
    ]
}

fn default_themes() -> Vec<ThemeDefinition> {
    vec![
        theme_definition(
            "Default Dark",
            [21.0 / 255.0, 22.0 / 255.0, 23.0 / 255.0, 1.0],
            [21.0 / 255.0, 22.0 / 255.0, 23.0 / 255.0, 1.0],
            [0.16, 0.29, 0.48, 0.54],
            [0.26, 0.59, 0.98, 0.40],
            [0.26, 0.59, 0.98, 1.0],
            [1.0, 1.0, 1.0, 1.0],
            [0.50, 0.50, 0.50, 1.0],
            [0.0, 1.0, 0.0, 1.0],
            [0.90, 0.60, 0.20, 1.0],
            [1.0, 0.0, 0.0, 1.0],
            [0.43, 0.43, 0.50, 0.50],
            [0.45, 0.29, 0.15, 1.0],
            [0.0, 0.0, 0.0],
        ),
        theme_definition(
            "Modern Dark",
            [0.08, 0.08, 0.12, 1.0],
            [0.12, 0.12, 0.16, 1.0],
            [0.16, 0.16, 0.20, 1.0],
            [0.11, 0.36, 0.60, 1.0],
            [0.06, 0.26, 0.46, 1.0],
            [0.95, 0.95, 0.95, 1.0],
            [0.70, 0.70, 0.70, 1.0],
            [0.20, 0.80, 0.40, 1.0],
            [0.90, 0.60, 0.20, 1.0],
            [0.90, 0.30, 0.30, 1.0],
            [0.20, 0.20, 0.25, 1.0],
            [0.45, 0.29, 0.15, 1.0],
            [8.0, 4.0, 4.0],
        ),
        theme_definition(
            "Cyberpunk",
            [0.05, 0.05, 0.10, 1.0],
            [0.10, 0.10, 0.15, 1.0],
            [0.15, 0.15, 0.20, 1.0],
            [0.0, 0.49, 0.62, 1.0],
            [0.0, 0.44, 0.60, 1.0],
            [0.90, 0.90, 1.0, 1.0],
            [0.60, 0.60, 0.80, 1.0],
            [0.0, 1.0, 0.50, 1.0],
            [1.0, 0.50, 0.0, 1.0],
            [1.0, 0.20, 0.40, 1.0],
            [0.0, 0.40, 0.60, 1.0],
            [0.41, 0.02, 0.79, 1.0],
            [6.0, 3.0, 3.0],
        ),
        theme_definition(
            "Forest Green",
            [0.05, 0.12, 0.08, 1.0],
            [0.08, 0.16, 0.12, 1.0],
            [0.12, 0.20, 0.16, 1.0],
            [0.11, 0.55, 0.26, 1.0],
            [0.11, 0.43, 0.21, 1.0],
            [0.90, 0.95, 0.90, 1.0],
            [0.70, 0.80, 0.70, 1.0],
            [0.30, 0.90, 0.50, 1.0],
            [0.90, 0.70, 0.20, 1.0],
            [0.80, 0.30, 0.30, 1.0],
            [0.15, 0.30, 0.20, 1.0],
            [0.45, 0.50, 0.0, 1.0],
            [10.0, 5.0, 5.0],
        ),
        theme_definition(
            "Sunset Orange",
            [0.12, 0.08, 0.05, 1.0],
            [0.16, 0.12, 0.08, 1.0],
            [0.20, 0.16, 0.12, 1.0],
            [1.0, 0.50, 0.20, 1.0],
            [0.80, 0.40, 0.15, 1.0],
            [1.0, 0.95, 0.90, 1.0],
            [0.80, 0.70, 0.60, 1.0],
            [0.40, 0.80, 0.30, 1.0],
            [1.0, 0.70, 0.20, 1.0],
            [0.90, 0.30, 0.30, 1.0],
            [0.30, 0.20, 0.15, 1.0],
            [0.45, 0.29, 0.15, 1.0],
            [12.0, 6.0, 6.0],
        ),
        theme_definition(
            "Purple Haze",
            [0.10, 0.05, 0.15, 1.0],
            [0.15, 0.10, 0.20, 1.0],
            [0.20, 0.15, 0.25, 1.0],
            [0.58, 0.32, 0.70, 1.0],
            [0.43, 0.20, 0.58, 1.0],
            [0.95, 0.90, 1.0, 1.0],
            [0.70, 0.60, 0.80, 1.0],
            [0.40, 0.80, 0.60, 1.0],
            [1.0, 0.60, 0.40, 1.0],
            [0.90, 0.30, 0.50, 1.0],
            [0.25, 0.15, 0.30, 1.0],
            [0.35, 0.40, 0.0, 1.0],
            [8.0, 4.0, 4.0],
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
fn theme_definition(
    name: &str,
    bg_dark: [f32; 4],
    bg_medium: [f32; 4],
    bg_light: [f32; 4],
    accent_primary: [f32; 4],
    accent_secondary: [f32; 4],
    text_primary: [f32; 4],
    text_secondary: [f32; 4],
    success_color: [f32; 4],
    warning_color: [f32; 4],
    error_color: [f32; 4],
    border_color: [f32; 4],
    disabled_color: [f32; 4],
    rounding: [f32; 3],
) -> ThemeDefinition {
    ThemeDefinition {
        name: name.to_owned(),
        bg_dark,
        bg_medium,
        bg_light,
        accent_primary,
        accent_secondary,
        text_primary,
        text_secondary,
        success_color,
        warning_color,
        error_color,
        border_color,
        disabled_color,
        window_rounding: rounding[0],
        frame_rounding: rounding[1],
        button_rounding: rounding[2],
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn assert_close(left: f64, right: f64) {
        assert!((left - right).abs() < 1.0e-6, "left={left}, right={right}");
    }

    fn assert_color_close(left: [f32; 4], right: [f32; 4]) {
        for (left, right) in left.into_iter().zip(right) {
            assert_close(f64::from(left), f64::from(right));
        }
    }

    #[test]
    fn source_font_fixture_exists() {
        assert!(source_root().join("assets/LSANS.TTF").is_file());
    }

    #[test]
    fn packaged_font_precedes_source_tree_and_appdir_fallbacks() {
        let temporary =
            std::env::temp_dir().join(format!("smu-font-resolution-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temporary);
        let executable = temporary.join("portable");
        let appdir = temporary.join("appdir");
        let source = temporary.join("source");
        let current = temporary.join("current");
        for path in [
            executable.join("assets"),
            appdir.join("usr/bin/assets"),
            source.join("assets"),
            current.join("assets"),
        ] {
            std::fs::create_dir_all(path).expect("font fixture directory");
        }
        std::fs::write(source.join("assets/LSANS.TTF"), b"source").expect("source font");
        std::fs::write(appdir.join("usr/bin/assets/LSANS.TTF"), b"appdir").expect("appdir font");
        std::fs::write(executable.join("assets/LSANS.TTF"), b"packaged").expect("packaged font");

        assert_eq!(
            resolve_font_path_from(
                Some(&executable),
                std::slice::from_ref(&appdir),
                &source,
                &current
            ),
            executable.join("assets/LSANS.TTF")
        );
        std::fs::remove_file(executable.join("assets/LSANS.TTF")).expect("remove packaged font");
        assert_eq!(
            resolve_font_path_from(
                Some(&executable),
                std::slice::from_ref(&appdir),
                &source,
                &current
            ),
            appdir.join("usr/bin/assets/LSANS.TTF")
        );
        let _ = std::fs::remove_dir_all(&temporary);
    }

    #[test]
    fn defaults_select_cpp_cyberpunk_and_custom_starts_from_default_dark() {
        let state = ThemeState::default();
        assert_eq!(state.current_index, 2);
        assert_eq!(state.active().name, "Cyberpunk");
        assert_eq!(state.themes.len(), 6);
        assert_eq!(state.custom_theme.name, CUSTOM_THEME_NAME);
        assert_color_close(state.custom_theme.bg_dark, state.defaults[0].bg_dark);
    }

    #[test]
    fn metadata_rounds_like_cpp_and_saves_editor_visibility_without_loading_it() {
        let mut state = ThemeState { editor_open: true, ..ThemeState::default() };
        state.custom_theme.bg_dark[0] = 0.123_56;
        state.custom_theme.window_rounding = 1.234_56;
        let metadata = state.metadata_overlay();
        assert_eq!(metadata["show_theme_menu"], json!(true));
        assert_eq!(metadata["custom_theme"]["bg_dark"][0], json!(0.124));
        assert_eq!(metadata["custom_theme"]["window_rounding"], json!(1.235));

        let mut loaded = ThemeState::default();
        loaded.apply_metadata(&metadata);
        assert!(!loaded.editor_open);
        assert_close(f64::from(loaded.custom_theme.bg_dark[0]), 0.124);
    }

    #[test]
    fn metadata_replaces_named_presets_appends_unknown_and_allows_custom_index() {
        let mut metadata = Map::new();
        metadata.insert("current_theme_index".to_owned(), json!(7));
        metadata.insert(
            "themes".to_owned(),
            json!({
                "Cyberpunk": {
                    "name": "Cyberpunk",
                    "bg_dark": [0.9,0.8,0.7,1.0]
                },
                "Extra": {
                    "name": "Extra",
                    "bg_dark": [0.1,0.2,0.3,1.0]
                }
            }),
        );
        let mut state = ThemeState::default();
        state.apply_metadata(&metadata);
        assert_eq!(state.themes.len(), 7);
        // Index 7 equals themes.len() and therefore selects the custom theme, as in C++.
        assert_eq!(state.current_index, 7);
        assert_color_close(state.themes[2].bg_dark, [0.9, 0.8, 0.7, 1.0]);
        assert_eq!(state.themes[6].name, "Extra");
        assert_color_close(state.themes[6].bg_medium, [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn numeric_metadata_index_matches_cpp_truncation_and_invalid_fallback() {
        let mut state = ThemeState { current_index: 4, ..ThemeState::default() };
        state.apply_metadata(&Map::from_iter([("current_theme_index".to_owned(), json!(1.9))]));
        assert_eq!(state.current_index, 1);

        state.current_index = 4;
        state.apply_metadata(&Map::from_iter([("current_theme_index".to_owned(), json!(-1))]));
        assert_eq!(state.current_index, CYBERPUNK_INDEX);
    }

    #[test]
    fn invalid_metadata_index_falls_back_to_cyberpunk_by_name() {
        let mut state = ThemeState::default();
        state.apply_metadata(&Map::from_iter([("current_theme_index".to_owned(), json!(999))]));
        assert_eq!(state.current_index, CYBERPUNK_INDEX);
        assert_eq!(state.active().name, "Cyberpunk");
    }

    #[test]
    fn custom_reset_uses_default_dark_values_and_name() {
        let mut state = ThemeState::default();
        state.custom_theme.name = "Changed".to_owned();
        state.custom_theme.bg_dark = [1.0; 4];
        state.custom_theme = state.defaults[0].clone();
        CUSTOM_THEME_NAME.clone_into(&mut state.custom_theme.name);
        assert_eq!(state.custom_theme.name, CUSTOM_THEME_NAME);
        assert_color_close(state.custom_theme.bg_dark, state.defaults[0].bg_dark);
    }

    #[test]
    fn cplusplus_negative_rounding_expression_is_preserved() {
        assert_close(round_cpp(-0.123_6), -0.124);
        assert_close(round_cpp(-0.123_4), -0.123);
    }
}

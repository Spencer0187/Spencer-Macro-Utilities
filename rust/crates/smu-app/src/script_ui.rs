//! App-side imported-script UI state and rendering.
//!
//! The Lua manager remains the owner of records/workers. This layer only renders immutable
//! snapshots, queues typed actions, and owns transient selection/import-preview state.

use crate::{keybind, theme::ThemeDefinition};
use dear_imgui_rs::{
    Condition, ListClipper, MouseButton, SelectableFlags, StyleColor, StyleVar, Ui, WindowFlags,
    sys,
};
use serde_json::{Value, json};
use smu_scripting::{ScriptRecordSnapshot, SettingsUiControl, SettingsUiControlKind};
use std::fs;
use std::path::{Path, PathBuf};

pub const FILE_DIALOG_FALLBACK_SMOKE_CASE: &str = "script-file-dialog-fallback-rust-smoke";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScriptAction {
    OpenImportDialog,
    ConfirmImport { path: PathBuf },
    SetHotkey { index: usize, hotkey: u32 },
    SetEnabled { index: usize, enabled: bool },
    SetDisableOutside { index: usize, disabled: bool },
    Execute { index: usize },
    Reload { index: usize },
    Remove { index: usize },
    ForceStop { index: usize },
    Reset { index: usize },
    SettingValue { index: usize, id: String, value: Value },
    SettingButton { index: usize, id: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ScriptFileDialogEntry {
    path: PathBuf,
    label: String,
    directory: bool,
}

#[derive(Debug, Default)]
struct ScriptFileDialogFallbackState {
    open: bool,
    directory: PathBuf,
    selected: Option<PathBuf>,
    path_text: String,
    error: String,
}

#[derive(Debug, Default)]
pub struct ScriptUiState {
    snapshots: Vec<ScriptRecordSnapshot>,
    selected: Option<usize>,
    controls: Vec<SettingsUiControl>,
    import_error: String,
    import_notice: String,
    pending_import: Option<PathBuf>,
    preview: String,
    preview_error: String,
    open_trust_modal: bool,
    file_dialog_fallback: ScriptFileDialogFallbackState,
}

impl ScriptUiState {
    pub fn apply_import_preview_parity_reference(&mut self) {
        self.pending_import = Some(PathBuf::from("/tmp/smu-parity-preview.lua"));
        concat!(
            "-- @name: Preview Fixture\n",
            "local message = \"hello\"\n",
            "print(message)\n",
            "pressKey(0x41)\n",
            "-- review before import\n"
        )
        .clone_into(&mut self.preview);
        self.preview_error.clear();
        self.open_trust_modal = true;
        self.import_error.clear();
        self.import_notice.clear();
    }

    pub fn apply_import_preview_error_parity_reference(&mut self) {
        self.pending_import = Some(PathBuf::from("/tmp/smu-parity-preview.lua"));
        self.preview.clear();
        "Preview unavailable: the selected file contains embedded NUL bytes."
            .clone_into(&mut self.preview_error);
        self.open_trust_modal = true;
        self.import_error.clear();
        self.import_notice.clear();
    }

    pub fn apply_parity_reference(&mut self) {
        self.snapshots = vec![ScriptRecordSnapshot {
            path: PathBuf::from("/tmp/smu-parity-script.lua"),
            metadata: smu_scripting::ImportedScriptMetadata {
                name: "Parity Script".to_owned(),
                description: "Deterministic imported-script parity fixture.".to_owned(),
                author: "SMU".to_owned(),
                version: "1.0".to_owned(),
                memory_limit_mb: None,
            },
            hotkey: smu_core::Hotkey::new(smu_scripting::SCRIPT_UNBOUND_HOTKEY),
            enabled: false,
            disable_outside_roblox: true,
            loaded: true,
            missing: false,
            running: false,
            has_settings_callback: false,
            last_error: String::new(),
            last_warning: String::new(),
            ui_state: json!({}),
        }];
        self.selected = Some(0);
        self.controls.clear();
        self.import_error.clear();
        self.import_notice.clear();
        self.pending_import = None;
        self.preview.clear();
        self.preview_error.clear();
        self.open_trust_modal = false;
    }

    pub fn set_snapshots(&mut self, snapshots: Vec<ScriptRecordSnapshot>) {
        self.snapshots = snapshots;
        if self.selected.is_some_and(|index| index >= self.snapshots.len()) {
            self.selected = None;
            self.controls.clear();
        }
    }

    pub fn set_controls(&mut self, controls: Vec<SettingsUiControl>) {
        self.controls = controls;
    }

    #[must_use]
    pub const fn selected(&self) -> Option<usize> {
        self.selected
    }

    #[must_use]
    pub fn selected_snapshot(&self) -> Option<&ScriptRecordSnapshot> {
        self.selected.and_then(|index| self.snapshots.get(index))
    }

    #[cfg(test)]
    pub fn select(&mut self, index: usize) {
        if index < self.snapshots.len() {
            self.selected = Some(index);
            self.import_error.clear();
        }
    }

    pub fn clear_selection(&mut self) {
        self.selected = None;
        self.controls.clear();
    }

    pub fn set_import_error(&mut self, error: impl Into<String>) {
        self.import_error = error.into();
    }

    pub fn set_import_notice(&mut self, notice: impl Into<String>) {
        self.import_notice = notice.into();
    }

    pub fn open_file_dialog_fallback(&mut self) {
        self.file_dialog_fallback.open = true;
        self.file_dialog_fallback.directory =
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        self.file_dialog_fallback.selected = None;
        self.file_dialog_fallback.path_text.clear();
        self.file_dialog_fallback.error.clear();
        self.import_error.clear();
    }

    #[must_use]
    pub const fn file_dialog_fallback_open(&self) -> bool {
        self.file_dialog_fallback.open
    }

    pub fn queue_dropped_import_candidate(&mut self, path: PathBuf) -> bool {
        if !is_supported_script_path(&path) || !path.is_file() {
            return false;
        }
        self.queue_import_candidate(path);
        true
    }

    pub fn queue_import_candidate(&mut self, path: PathBuf) {
        self.preview.clear();
        self.preview_error.clear();
        match fs::read(&path) {
            Ok(bytes) if bytes.contains(&0) => {
                "Preview unavailable: the selected file contains embedded NUL bytes."
                    .clone_into(&mut self.preview_error);
            }
            Ok(bytes) => {
                self.preview = String::from_utf8_lossy(&bytes).into_owned();
            }
            Err(_) => {
                "Preview unavailable: could not read the selected file."
                    .clone_into(&mut self.preview_error);
            }
        }
        self.pending_import = Some(path);
        self.open_trust_modal = true;
        self.import_error.clear();
    }

    pub fn cancel_import(&mut self) {
        self.pending_import = None;
        self.preview.clear();
        self.preview_error.clear();
        self.open_trust_modal = false;
    }

    pub const fn select_last_script(&mut self) {
        self.selected = self.snapshots.len().checked_sub(1);
    }

    pub fn remove_completed(&mut self, removed_index: usize) {
        self.selected = match self.selected {
            Some(index) if index == removed_index => None,
            Some(index) if index > removed_index => Some(index - 1),
            other => other,
        };
        self.controls.clear();
    }
}

fn is_supported_script_path(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| {
        matches!(extension.to_ascii_lowercase().as_str(), "smus" | "hss" | "lua" | "txt")
    })
}

fn fallback_directory_entries(directory: &Path) -> Result<Vec<ScriptFileDialogEntry>, String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("Could not read {}: {error}", directory.display()))?;
    let mut entries = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let directory = path.is_dir();
            if !(directory || path.is_file() && is_supported_script_path(&path)) {
                return None;
            }
            Some(ScriptFileDialogEntry {
                label: entry.file_name().to_string_lossy().into_owned(),
                path,
                directory,
            })
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        right.directory.cmp(&left.directory).then_with(|| {
            left.label
                .to_ascii_lowercase()
                .cmp(&right.label.to_ascii_lowercase())
                .then_with(|| left.label.cmp(&right.label))
        })
    });
    Ok(entries)
}

fn fallback_selected_path(state: &ScriptFileDialogFallbackState) -> Result<PathBuf, String> {
    let raw = state.path_text.trim();
    let path = if raw.is_empty() {
        state.selected.clone().ok_or_else(|| "Select a script file first.".to_owned())?
    } else {
        let typed = PathBuf::from(raw);
        if typed.is_absolute() { typed } else { state.directory.join(typed) }
    };
    if path.is_dir() {
        return Err("The selected path is a directory.".to_owned());
    }
    if !path.is_file() {
        return Err("The selected script file does not exist.".to_owned());
    }
    if !is_supported_script_path(&path) {
        return Err("Unsupported script extension.".to_owned());
    }
    Ok(path)
}

pub fn render_sidebar(
    ui: &Ui,
    state: &mut ScriptUiState,
    left_panel_width: f32,
    theme: &ThemeDefinition,
) -> Option<ScriptAction> {
    ui.spacing();
    ui.text_wrapped("Imported Scripts");
    let style = ui.clone_style();
    let button_width = style.frame_padding()[0].mul_add(-2.0, left_panel_width);
    let item_width = (button_width - if ui.scroll_max_y() == 0.0 { 7.0 } else { 18.0 }).max(1.0);
    if ui.button_with_size("+ Import Script", [item_width, 0.0]) {
        return Some(ScriptAction::OpenImportDialog);
    }

    for (index, script) in state.snapshots.iter().enumerate() {
        let selected = state.selected == Some(index);
        let attention = script.missing || (!script.loaded && !script.last_error.is_empty());
        let active = script.enabled && script.loaded && !script.missing;
        let (base, selected_factor, hovered_factor, active_factor) = if attention {
            (theme.error_color, 1.3, 1.4, 0.9)
        } else if active {
            (theme.accent_primary, 1.4, 1.3, 0.8)
        } else {
            (theme.disabled_color, 1.6, 1.3, 1.1)
        };
        let _button = ui.push_style_color(
            StyleColor::Button,
            if selected { brighten(base, selected_factor) } else { base },
        );
        let _hovered =
            ui.push_style_color(StyleColor::ButtonHovered, brighten(base, hovered_factor));
        let _active = ui.push_style_color(StyleColor::ButtonActive, brighten(base, active_factor));
        let mut label = script_label(script);
        if script.running {
            label.push_str(" (Running)");
        } else if script.missing {
            label.push_str(" (Missing)");
        } else if !script.loaded {
            label.push_str(" (Error)");
        }
        if ui.button_with_size(&label, [item_width, 0.0]) {
            state.selected = Some(index);
        }
    }

    if !state.import_error.is_empty() {
        ui.text_colored(theme.error_color, &state.import_error);
    }
    if !state.import_notice.is_empty() {
        ui.text_colored(theme.warning_color, &state.import_notice);
    }
    // Terminal item in the sidebar child: do not use the compensated parity separator here,
    // because it intentionally rewinds the cursor and requires a following item before EndChild.
    ui.separator();
    None
}

pub fn render_selected(
    ui: &Ui,
    state: &ScriptUiState,
    capture: &mut keybind::CaptureState,
    theme: &ThemeDefinition,
) -> Option<ScriptAction> {
    let index = state.selected?;
    let script = state.snapshots.get(index)?.clone();
    ui.text_wrapped(format!("Settings for {}", script_label(&script)));
    crate::parity::separator(ui);
    if let Some(action) = render_script_controls(ui, index, &script, capture, theme) {
        return Some(action);
    }
    render_script_details(ui, &script, theme);
    crate::parity::separator(ui);
    ui.text_wrapped("Custom Settings");
    if !script.has_settings_callback {
        ui.text_wrapped("This script does not define onSettings().");
        return None;
    }
    if script.running {
        ui.text_wrapped(
            "Settings are read-only while the script is running. Dynamic text stays live and copyable.",
        );
    }
    if state.controls.is_empty() {
        return None;
    }
    render_custom_controls(ui, index, &script, &state.controls, capture)
}

fn render_script_controls(
    ui: &Ui,
    index: usize,
    script: &ScriptRecordSnapshot,
    capture: &mut keybind::CaptureState,
    theme: &ThemeDefinition,
) -> Option<ScriptAction> {
    ui.text_wrapped("Keybind:");
    ui.same_line();
    let mut hotkey = script.hotkey.raw();
    keybind::render_control(
        ui,
        capture,
        &format!("ImportedScriptKey{index}"),
        &mut hotkey,
        170.0,
        130.0,
        false,
    );
    if hotkey != script.hotkey.raw() {
        return Some(ScriptAction::SetHotkey { index, hotkey });
    }

    let color = ui.push_style_color(
        StyleColor::Text,
        if script.enabled { theme.success_color } else { theme.error_color },
    );
    ui.text_wrapped("Enable This Script:");
    drop(color);
    ui.same_line();
    let mut enabled = script.enabled;
    if ui.checkbox(format!("##ImportedScriptEnabled{index}"), &mut enabled) {
        return Some(ScriptAction::SetEnabled { index, enabled });
    }
    ui.same_line();
    let mut disable_outside = script.disable_outside_roblox;
    if crate::foreground_ui::render_checkbox(
        ui,
        "Disable outside of Target Application:",
        &format!("##ImportedScriptDisableOutside{index}"),
        &mut disable_outside,
    ) {
        return Some(ScriptAction::SetDisableOutside { index, disabled: disable_outside });
    }

    let disabled = script.running.then(|| ui.begin_disabled());
    let execute = ui.button("Execute Now");
    ui.same_line();
    let reload = ui.button("Reload");
    ui.same_line();
    let remove_color = ui.push_style_color(StyleColor::Button, theme.error_color);
    let remove_hovered =
        ui.push_style_color(StyleColor::ButtonHovered, brighten(theme.error_color, 1.2));
    let remove_active =
        ui.push_style_color(StyleColor::ButtonActive, brighten(theme.error_color, 0.8));
    let remove = ui.button("Remove");
    drop(remove_active);
    drop(remove_hovered);
    drop(remove_color);
    drop(disabled);
    if execute {
        return Some(ScriptAction::Execute { index });
    }
    if reload {
        return Some(ScriptAction::Reload { index });
    }
    if remove {
        return Some(ScriptAction::Remove { index });
    }

    ui.same_line();
    let stop_disabled = (!script.running).then(|| ui.begin_disabled());
    let stop = ui.button("Force Stop");
    drop(stop_disabled);
    if stop {
        return Some(ScriptAction::ForceStop { index });
    }
    ui.same_line();
    let reset_disabled = script.running.then(|| ui.begin_disabled());
    let reset = ui.button("Revert Module");
    drop(reset_disabled);
    reset.then_some(ScriptAction::Reset { index })
}

fn render_script_details(ui: &Ui, script: &ScriptRecordSnapshot, theme: &ThemeDefinition) {
    crate::parity::separator(ui);
    let (status, status_color) = if script.running {
        ("Running", theme.accent_primary)
    } else if script.missing {
        ("Missing", theme.error_color)
    } else if !script.loaded {
        ("Error", theme.error_color)
    } else {
        ("Loaded", theme.success_color)
    };
    ui.text_colored(status_color, format!("Status: {status}"));
    if !script.last_error.is_empty() {
        ui.text_colored(theme.error_color, &script.last_error);
    }
    if !script.last_warning.is_empty() {
        ui.text_colored(theme.warning_color, &script.last_warning);
    }
    if !script.metadata.description.is_empty() {
        ui.text_wrapped(&script.metadata.description);
        ui.spacing();
    }
    if !script.metadata.author.is_empty() {
        ui.text_wrapped(format!("Author: {}", script.metadata.author));
    }
    if !script.metadata.version.is_empty() {
        ui.text_wrapped(format!("Version: {}", script.metadata.version));
    }
    ui.text_wrapped(format!("File: {}", sanitize_path(&script.path)));
}

fn render_custom_controls(
    ui: &Ui,
    index: usize,
    script: &ScriptRecordSnapshot,
    controls: &[SettingsUiControl],
    capture: &mut keybind::CaptureState,
) -> Option<ScriptAction> {
    for control in controls {
        let id = format!("ScriptSetting{index}:{}", control.id);
        let _id = ui.push_id(&id);
        let interactive = !matches!(
            control.kind,
            SettingsUiControlKind::Text
                | SettingsUiControlKind::Separator
                | SettingsUiControlKind::DynamicTextbox
        );
        let disabled = (script.running && interactive).then(|| ui.begin_disabled());
        let action = render_control(ui, index, script, control, capture);
        drop(disabled);
        if action.is_some() {
            return action;
        }
    }
    None
}

fn render_control(
    ui: &Ui,
    index: usize,
    script: &ScriptRecordSnapshot,
    control: &SettingsUiControl,
    capture: &mut keybind::CaptureState,
) -> Option<ScriptAction> {
    let value = script.ui_state.get(&control.id);
    match control.kind {
        SettingsUiControlKind::Text => ui.text_wrapped(&control.text),
        SettingsUiControlKind::Separator => crate::parity::separator(ui),
        SettingsUiControlKind::Checkbox => {
            let mut current = value.and_then(Value::as_bool).unwrap_or(control.default_bool);
            if ui.checkbox(&control.label, &mut current) {
                return Some(setting_value(index, control, json!(current)));
            }
        }
        SettingsUiControlKind::SliderInt => {
            let mut current = value
                .and_then(Value::as_i64)
                .and_then(|value| i32::try_from(value).ok())
                .unwrap_or(control.default_int);
            if ui.slider(&control.label, control.min_int, control.max_int, &mut current) {
                return Some(setting_value(index, control, json!(current)));
            }
        }
        SettingsUiControlKind::SliderFloat => {
            let mut current = value.and_then(Value::as_f64).unwrap_or(control.default_float);
            if ui.slider(&control.label, control.min_float, control.max_float, &mut current) {
                return Some(setting_value(index, control, json!(current)));
            }
        }
        SettingsUiControlKind::Dropdown => {
            let current = value.and_then(Value::as_str).unwrap_or(&control.default_text).to_owned();
            let mut selected = control.items.iter().position(|item| item == &current).unwrap_or(0);
            let items = control.items.iter().map(String::as_str).collect::<Vec<_>>();
            if ui.combo_simple_string(&control.label, &mut selected, &items)
                && let Some(value) = control.items.get(selected)
            {
                return Some(setting_value(index, control, json!(value)));
            }
        }
        SettingsUiControlKind::Textbox => {
            return render_textbox_control(ui, index, script, control, value);
        }
        SettingsUiControlKind::DynamicTextbox => {
            render_dynamic_textbox(ui, control, value);
        }
        SettingsUiControlKind::Keybind => {
            let mut current = value
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or_else(|| control.default_keybind.raw());
            keybind::render_control(
                ui,
                capture,
                &format!("ScriptCustomKey:{}", control.id),
                &mut current,
                control.width.max(100.0),
                50.0,
                true,
            );
            if current
                != value
                    .and_then(Value::as_u64)
                    .and_then(|v| u32::try_from(v).ok())
                    .unwrap_or_else(|| control.default_keybind.raw())
            {
                return Some(setting_value(index, control, json!(current)));
            }
        }
        SettingsUiControlKind::Button => {
            if ui
                .button_with_size(&control.label, [control.width.max(0.0), control.height.max(0.0)])
                && !script.running
            {
                return Some(ScriptAction::SettingButton { index, id: control.id.clone() });
            }
        }
    }
    None
}

fn render_textbox_control(
    ui: &Ui,
    index: usize,
    script: &ScriptRecordSnapshot,
    control: &SettingsUiControl,
    value: Option<&Value>,
) -> Option<ScriptAction> {
    let mut current = value.and_then(Value::as_str).unwrap_or(&control.default_text).to_owned();
    let changed = if control.height > 0.0 {
        let width = if control.width > 0.0 { control.width } else { -f32::MIN_POSITIVE };
        ui.input_text_multiline(&control.label, &mut current, [width, control.height]).build()
    } else {
        if control.width > 0.0 {
            ui.set_next_item_width(control.width);
        }
        ui.input_text(&control.label, &mut current).build()
    };
    (changed && !script.running).then(|| setting_value(index, control, json!(current)))
}

fn render_dynamic_textbox(ui: &Ui, control: &SettingsUiControl, value: Option<&Value>) {
    let mut current = value.and_then(Value::as_str).unwrap_or(&control.default_text).to_owned();
    ui.text_wrapped(&control.label);
    let height = if control.height > 0.0 { control.height } else { ui.text_line_height() * 4.0 };
    let width = if control.width > 0.0 { control.width } else { -f32::MIN_POSITIVE };
    let _ =
        ui.input_text_multiline("##dynamic", &mut current, [width, height]).read_only(true).build();
}

fn setting_value(index: usize, control: &SettingsUiControl, value: Value) -> ScriptAction {
    ScriptAction::SettingValue { index, id: control.id.clone(), value }
}

const LUA_KEYWORDS: [&str; 22] = [
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in",
    "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];

const RISKY_LUA_FUNCTIONS: [&str; 16] = [
    "pressKey",
    "clickMouse",
    "holdKey",
    "releaseKey",
    "typeText",
    "moveMouse",
    "moveMouseAbs",
    "moveDegrees",
    "mouseWheel",
    "freeze",
    "lagSwitch",
    "setLagSwitchConfig",
    "clearLagSwitchConfig",
    "robloxFreeze",
    "roblox_freeze",
    "lagswitch",
];

#[derive(Clone, Copy)]
struct ImportPreviewPalette {
    normal: [f32; 4],
    keyword: [f32; 4],
    string_literal: [f32; 4],
    comment: [f32; 4],
    function_call: [f32; 4],
    risky_function_call: [f32; 4],
    metadata_tag: [f32; 4],
}

fn import_preview_palette(theme: &ThemeDefinition) -> ImportPreviewPalette {
    ImportPreviewPalette {
        normal: theme.text_primary,
        keyword: brighten(theme.accent_primary, 1.15),
        string_literal: theme.success_color,
        comment: theme.disabled_color,
        function_call: brighten(theme.accent_secondary, 1.15),
        risky_function_call: brighten(theme.warning_color, 1.1),
        metadata_tag: brighten(theme.accent_primary, 1.35),
    }
}

const fn is_lua_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

const fn is_lua_identifier_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn advance_char_boundary(text: &str, index: usize) -> usize {
    index + text[index..].chars().next().map_or(1, char::len_utf8)
}

fn draw_import_preview_segment(ui: &Ui, x: f32, y: f32, color: [f32; 4], text: &str) -> f32 {
    if text.is_empty() {
        return x;
    }
    let width = ui.calc_text_size(text)[0];
    ui.get_window_draw_list().add_text([x, y], color, text);
    x + width
}

fn finish_import_preview_line(ui: &Ui, cursor_x: f32, x: f32) {
    ui.dummy([(x - cursor_x).max(1.0), ui.text_line_height_with_spacing()]);
}

fn render_import_preview_comment(
    ui: &Ui,
    line: &str,
    current: usize,
    palette: ImportPreviewPalette,
    mut x: f32,
    y: f32,
    cursor_x: f32,
) {
    let bytes = line.as_bytes();
    let mut after_prefix = current + 2;
    while after_prefix < bytes.len() && matches!(bytes[after_prefix], b' ' | b'\t') {
        after_prefix += 1;
    }
    if after_prefix < bytes.len() && bytes[after_prefix] == b'@' {
        let mut tag_end = after_prefix + 1;
        while tag_end < bytes.len()
            && (is_lua_identifier_char(bytes[tag_end]) || bytes[tag_end] == b'-')
        {
            tag_end += 1;
        }
        if tag_end < bytes.len() && bytes[tag_end] == b':' {
            x = draw_import_preview_segment(
                ui,
                x,
                y,
                palette.comment,
                &line[current..after_prefix],
            );
            x = draw_import_preview_segment(
                ui,
                x,
                y,
                palette.metadata_tag,
                &line[after_prefix..=tag_end],
            );
            x = draw_import_preview_segment(ui, x, y, palette.comment, &line[tag_end + 1..]);
            finish_import_preview_line(ui, cursor_x, x);
            return;
        }
    }
    x = draw_import_preview_segment(ui, x, y, palette.comment, &line[current..]);
    finish_import_preview_line(ui, cursor_x, x);
}

fn render_import_preview_line(ui: &Ui, line: &str, palette: ImportPreviewPalette) {
    let cursor = ui.cursor_screen_pos();
    let mut x = cursor[0];
    let y = cursor[1];
    let bytes = line.as_bytes();
    let mut current = 0_usize;

    while current < bytes.len() {
        if current + 1 < bytes.len() && bytes[current] == b'-' && bytes[current + 1] == b'-' {
            render_import_preview_comment(ui, line, current, palette, x, y, cursor[0]);
            return;
        }

        if matches!(bytes[current], b'"' | b'\'') {
            let quote = bytes[current];
            let token_start = current;
            current += 1;
            let mut escaped = false;
            while current < bytes.len() {
                if escaped {
                    escaped = false;
                    current = advance_char_boundary(line, current);
                    continue;
                }
                if bytes[current] == b'\\' {
                    escaped = true;
                    current += 1;
                    continue;
                }
                if bytes[current] == quote {
                    current += 1;
                    break;
                }
                current = advance_char_boundary(line, current);
            }
            x = draw_import_preview_segment(
                ui,
                x,
                y,
                palette.string_literal,
                &line[token_start..current],
            );
            continue;
        }

        if is_lua_identifier_start(bytes[current]) {
            let token_start = current;
            current += 1;
            while current < bytes.len() && is_lua_identifier_char(bytes[current]) {
                current += 1;
            }
            let token = &line[token_start..current];
            let mut lookahead = current;
            while lookahead < bytes.len() && matches!(bytes[lookahead], b' ' | b'\t') {
                lookahead += 1;
            }
            let color = if LUA_KEYWORDS.contains(&token) {
                palette.keyword
            } else if lookahead < bytes.len() && bytes[lookahead] == b'(' {
                if RISKY_LUA_FUNCTIONS.contains(&token) {
                    palette.risky_function_call
                } else {
                    palette.function_call
                }
            } else {
                palette.normal
            };
            x = draw_import_preview_segment(ui, x, y, color, token);
            continue;
        }

        let token_start = current;
        current = advance_char_boundary(line, current);
        while current < bytes.len() {
            if current + 1 < bytes.len() && bytes[current] == b'-' && bytes[current + 1] == b'-'
                || matches!(bytes[current], b'"' | b'\'')
                || is_lua_identifier_start(bytes[current])
            {
                break;
            }
            current = advance_char_boundary(line, current);
        }
        x = draw_import_preview_segment(ui, x, y, palette.normal, &line[token_start..current]);
    }

    finish_import_preview_line(ui, cursor[0], x);
}

fn render_import_preview(ui: &Ui, state: &ScriptUiState, theme: &ThemeDefinition) {
    if !state.preview_error.is_empty() {
        ui.text_colored(theme.error_color, &state.preview_error);
        return;
    }
    if state.preview.is_empty() {
        ui.text_disabled("The selected file is empty.");
    }

    let palette = import_preview_palette(theme);
    let lines = state
        .preview
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect::<Vec<_>>();
    let preview_height = ui.content_region_avail()[1].max(0.0);
    ui.child_window("ImportScriptPreview")
        .size([0.0, preview_height])
        .border(true)
        .flags(WindowFlags::HORIZONTAL_SCROLLBAR)
        .build(ui, || {
            let item_spacing = ui.push_style_var(StyleVar::ItemSpacing([0.0, 0.0]));
            let mut clipper = ListClipper::new(lines.len())
                .items_height(ui.text_line_height_with_spacing())
                .begin(ui);
            while clipper.step() {
                for line_index in clipper.display_range() {
                    render_import_preview_line(ui, lines[line_index], palette);
                }
            }
            drop(item_spacing);
        });
}

fn set_file_dialog_fallback_layout(ui: &Ui) {
    let viewport = ui.main_viewport();
    let viewport_pos = viewport.pos();
    let viewport_size = viewport.size();
    let center = [
        viewport_size[0].mul_add(0.5, viewport_pos[0]),
        viewport_size[1].mul_add(0.5, viewport_pos[1]),
    ];
    let min_size = [(viewport_size[0] * 0.5).max(320.0), (viewport_size[1] * 0.5).max(240.0)];
    let max_size =
        [(viewport_size[0] - 32.0).max(min_size[0]), (viewport_size[1] - 32.0).max(min_size[1])];
    // SAFETY: these configure the immediately following modal Begin while `ui` owns the active
    // Dear ImGui context for this frame.
    unsafe {
        sys::igSetNextWindowPos(
            sys::ImVec2_c { x: center[0], y: center[1] },
            Condition::Appearing as i32,
            sys::ImVec2_c { x: 0.5, y: 0.5 },
        );
        sys::igSetNextWindowSizeConstraints(
            sys::ImVec2_c { x: min_size[0], y: min_size[1] },
            sys::ImVec2_c { x: max_size[0], y: max_size[1] },
            None,
            std::ptr::null_mut(),
        );
        sys::igSetNextWindowSize(
            sys::ImVec2_c { x: min_size[0], y: min_size[1] },
            Condition::Appearing as i32,
        );
    }
}

fn render_fallback_entry_list(
    ui: &Ui,
    state: &mut ScriptUiState,
) -> (Option<PathBuf>, Option<PathBuf>) {
    let entries = match fallback_directory_entries(&state.file_dialog_fallback.directory) {
        Ok(entries) => entries,
        Err(error) => {
            error.clone_into(&mut state.file_dialog_fallback.error);
            return (None, None);
        }
    };

    let selected = state.file_dialog_fallback.selected.clone();
    let available = ui.content_region_avail()[1];
    let list_height = (available - 92.0).max(80.0);
    let mut clicked = None;
    let mut activated = None;
    ui.child_window("##ScriptFileDialogFallbackList")
        .size([0.0, list_height])
        .border(true)
        .flags(WindowFlags::HORIZONTAL_SCROLLBAR)
        .build(ui, || {
            for entry in &entries {
                let label = if entry.directory {
                    format!("[DIR] {}", entry.label)
                } else {
                    entry.label.clone()
                };
                if ui
                    .selectable_config(&label)
                    .selected(selected.as_ref() == Some(&entry.path))
                    .flags(SelectableFlags::ALLOW_DOUBLE_CLICK)
                    .build()
                {
                    clicked = Some(entry.clone());
                    if ui.is_mouse_double_clicked(MouseButton::Left) {
                        activated = Some(entry.clone());
                    }
                }
            }
        });
    if let Some(entry) = clicked {
        state.file_dialog_fallback.selected = Some(entry.path.clone());
        entry.label.clone_into(&mut state.file_dialog_fallback.path_text);
        state.file_dialog_fallback.error.clear();
    }
    match activated {
        Some(entry) if entry.directory => (Some(entry.path), None),
        Some(entry) => (None, Some(entry.path)),
        None => (None, None),
    }
}

pub fn render_file_dialog_fallback(ui: &Ui, state: &mut ScriptUiState) {
    if !state.file_dialog_fallback.open {
        return;
    }

    let _ = ui.open_popup("Import SMU Script");
    set_file_dialog_fallback_layout(ui);
    let Some(_popup) =
        ui.begin_modal_popup_config("Import SMU Script").flags(WindowFlags::NO_COLLAPSE).begin()
    else {
        return;
    };

    let up_destination = if ui.button("Up") {
        state.file_dialog_fallback.directory.parent().map(Path::to_path_buf)
    } else {
        None
    };
    ui.same_line();
    ui.text_wrapped(format!("Folder: {}", sanitize_path(&state.file_dialog_fallback.directory)));

    let (entry_destination, activated_file) = render_fallback_entry_list(ui, state);
    let navigate_to = entry_destination.or(up_destination);

    ui.set_next_item_width(-f32::MIN_POSITIVE);
    let enter = ui
        .input_text("##ScriptFileDialogFallbackPath", &mut state.file_dialog_fallback.path_text)
        .hint("Script file name or path")
        .enter_returns_true(true)
        .build();

    let open_clicked = ui.button_with_size("Open", [90.0, 0.0]);
    ui.same_line();
    let cancel_clicked = ui.button_with_size("Cancel", [90.0, 0.0]);
    if !state.file_dialog_fallback.error.is_empty() {
        ui.same_line();
        ui.text_colored([1.0, 0.25, 0.25, 1.0], &state.file_dialog_fallback.error);
    }

    if let Some(directory) = navigate_to {
        state.file_dialog_fallback.directory = directory;
        state.file_dialog_fallback.selected = None;
        state.file_dialog_fallback.path_text.clear();
        state.file_dialog_fallback.error.clear();
        return;
    }

    if cancel_clicked {
        state.file_dialog_fallback = ScriptFileDialogFallbackState::default();
        ui.close_current_popup();
        return;
    }

    if let Some(path) = activated_file {
        state.file_dialog_fallback = ScriptFileDialogFallbackState::default();
        ui.close_current_popup();
        state.queue_import_candidate(path);
        return;
    }

    if open_clicked || enter {
        match fallback_selected_path(&state.file_dialog_fallback) {
            Ok(path) => {
                state.file_dialog_fallback = ScriptFileDialogFallbackState::default();
                ui.close_current_popup();
                state.queue_import_candidate(path);
            }
            Err(error) if error == "The selected path is a directory." => {
                let raw = state.file_dialog_fallback.path_text.trim();
                let typed = PathBuf::from(raw);
                let directory = if typed.is_absolute() {
                    typed
                } else {
                    state.file_dialog_fallback.directory.join(typed)
                };
                state.file_dialog_fallback.directory = directory;
                state.file_dialog_fallback.selected = None;
                state.file_dialog_fallback.path_text.clear();
                state.file_dialog_fallback.error.clear();
            }
            Err(error) => error.clone_into(&mut state.file_dialog_fallback.error),
        }
    }
}

pub fn render_trust_modal(
    ui: &Ui,
    state: &mut ScriptUiState,
    theme: &ThemeDefinition,
) -> Option<ScriptAction> {
    if state.open_trust_modal {
        let _ = ui.open_popup("Import Script?");
        state.open_trust_modal = false;
    }

    let viewport = ui.main_viewport();
    let viewport_pos = viewport.pos();
    let viewport_size = viewport.size();
    let center = [
        viewport_size[0].mul_add(0.5, viewport_pos[0]),
        viewport_size[1].mul_add(0.5, viewport_pos[1]),
    ];
    let initial_size =
        [760.0_f32.min(viewport_size[0] * 0.92), 560.0_f32.min(viewport_size[1] * 0.88)];
    let max_size = [viewport_size[0] * 0.96, viewport_size[1] * 0.94];
    // SAFETY: these are the Dear ImGui SetNextWindow* calls corresponding to the immediately
    // following modal Begin, while `ui` owns an active context for this frame.
    unsafe {
        sys::igSetNextWindowPos(
            sys::ImVec2_c { x: center[0], y: center[1] },
            Condition::Appearing as i32,
            sys::ImVec2_c { x: 0.5, y: 0.5 },
        );
        sys::igSetNextWindowSizeConstraints(
            sys::ImVec2_c { x: 560.0, y: 420.0 },
            sys::ImVec2_c { x: max_size[0], y: max_size[1] },
            None,
            std::ptr::null_mut(),
        );
        sys::igSetNextWindowSize(
            sys::ImVec2_c { x: initial_size[0], y: initial_size[1] },
            Condition::Appearing as i32,
        );
    }

    let _popup = ui.begin_modal_popup("Import Script?")?;
    ui.text_wrapped(
        "Imported scripts can control keyboard and mouse input and interact with Roblox-related SMU features. Only import scripts from people you trust.",
    );
    ui.spacing();
    if let Some(path) = state.pending_import.as_deref() {
        ui.text_wrapped(format!("File: {}", sanitize_path(path)));
    }
    crate::parity::separator(ui);
    if ui.button_with_size("Import", [90.0, 0.0])
        && let Some(path) = state.pending_import.clone()
    {
        ui.close_current_popup();
        return Some(ScriptAction::ConfirmImport { path });
    }
    ui.same_line();
    if ui.button_with_size("Cancel", [90.0, 0.0]) {
        state.cancel_import();
        ui.close_current_popup();
        return None;
    }
    ui.spacing();
    // SAFETY: `c"Preview"` is a valid NUL-terminated static string and `ui` owns the active ImGui
    // context for this frame.
    unsafe { sys::igSeparatorText(c"Preview".as_ptr()) };
    ui.text_disabled("Read-only source preview. If the script is malicious, it's your fault.");
    render_import_preview(ui, state, theme);
    None
}

fn brighten(color: [f32; 4], factor: f32) -> [f32; 4] {
    [
        (color[0] * factor).min(1.0),
        (color[1] * factor).min(1.0),
        (color[2] * factor).min(1.0),
        color[3],
    ]
}

fn script_label(script: &ScriptRecordSnapshot) -> String {
    if !script.metadata.name.is_empty() {
        return script.metadata.name.clone();
    }
    script
        .path
        .file_stem()
        .map_or_else(|| "Imported Script".to_owned(), |stem| stem.to_string_lossy().into_owned())
}

fn sanitize_path(path: &Path) -> String {
    let raw = path.to_string_lossy();
    if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
        let home = PathBuf::from(home).to_string_lossy().into_owned();
        if let Some(suffix) = raw.strip_prefix(&home) {
            return format!("$HOME{suffix}");
        }
    }
    raw.into_owned()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use smu_core::Hotkey;
    use smu_scripting::{ImportedScriptMetadata, SCRIPT_UNBOUND_HOTKEY};

    fn snapshot(path: &str) -> ScriptRecordSnapshot {
        ScriptRecordSnapshot {
            path: PathBuf::from(path),
            metadata: ImportedScriptMetadata::default(),
            hotkey: Hotkey::new(SCRIPT_UNBOUND_HOTKEY),
            enabled: false,
            disable_outside_roblox: true,
            loaded: true,
            missing: false,
            running: false,
            has_settings_callback: false,
            last_error: String::new(),
            last_warning: String::new(),
            ui_state: json!({}),
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn import_preview_parity_reference_is_in_memory_and_pending() {
        let mut state = ScriptUiState::default();
        state.apply_import_preview_parity_reference();
        assert_eq!(state.pending_import.as_deref(), Some(Path::new("/tmp/smu-parity-preview.lua")));
        assert!(state.preview.contains("pressKey(0x41)"));
        assert!(state.preview_error.is_empty());
        assert!(state.open_trust_modal);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn import_preview_error_parity_reference_uses_cpp_nul_message() {
        let mut state = ScriptUiState::default();
        state.apply_import_preview_error_parity_reference();
        assert_eq!(state.pending_import.as_deref(), Some(Path::new("/tmp/smu-parity-preview.lua")));
        assert!(state.preview.is_empty());
        assert_eq!(
            state.preview_error,
            "Preview unavailable: the selected file contains embedded NUL bytes."
        );
        assert!(state.open_trust_modal);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parity_reference_is_loaded_idle_selected_script() {
        let mut state = ScriptUiState::default();
        state.apply_parity_reference();
        assert_eq!(state.selected(), Some(0));
        let script = &state.snapshots[0];
        assert_eq!(script.metadata.name, "Parity Script");
        assert_eq!(script.path, PathBuf::from("/tmp/smu-parity-script.lua"));
        assert!(script.loaded);
        assert!(!script.running);
        assert!(!script.has_settings_callback);
        assert!(state.controls.is_empty());
    }

    #[test]
    fn dropped_script_requires_supported_regular_file_and_queues_same_trust_preview() {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let directory =
            std::env::temp_dir().join(format!("smu-script-drop-{}-{counter}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("create drop fixture directory");
        let script = directory.join("dropped.LUA");
        fs::write(&script, "print('dropped')").expect("write dropped script");
        let unsupported = directory.join("notes.md");
        fs::write(&unsupported, "not a script").expect("write unsupported fixture");

        let mut state = ScriptUiState::default();
        assert!(state.queue_dropped_import_candidate(script.clone()));
        assert_eq!(state.pending_import.as_deref(), Some(script.as_path()));
        assert_eq!(state.preview, "print('dropped')");
        assert!(state.open_trust_modal);

        state.cancel_import();
        assert!(!state.queue_dropped_import_candidate(unsupported));
        assert!(!state.queue_dropped_import_candidate(directory.join("missing.lua")));
        assert!(!state.queue_dropped_import_candidate(directory.clone()));
        assert!(state.pending_import.is_none());

        fs::remove_dir_all(&directory).expect("remove drop fixture directory");
    }

    #[test]
    fn snapshot_refresh_drops_out_of_range_selection() {
        let mut state = ScriptUiState::default();
        state.set_snapshots(vec![snapshot("a.lua"), snapshot("b.lua")]);
        state.select(1);
        state.set_snapshots(vec![snapshot("a.lua")]);
        assert_eq!(state.selected(), None);
    }

    #[test]
    fn remove_reindexes_selection_like_cpp_vector_erasure() {
        let mut state = ScriptUiState::default();
        state.set_snapshots(vec![snapshot("a.lua"), snapshot("b.lua"), snapshot("c.lua")]);
        state.select(2);
        state.remove_completed(1);
        assert_eq!(state.selected(), Some(1));
    }

    #[test]
    fn fallback_directory_model_filters_sorts_and_resolves_relative_script() {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let directory = std::env::temp_dir()
            .join(format!("smu-script-fallback-{}-{counter}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(directory.join("z-dir")).expect("create z-dir");
        fs::create_dir_all(directory.join("a-dir")).expect("create a-dir");
        fs::write(directory.join("b.lua"), "return true").expect("write b.lua");
        fs::write(directory.join("A.TXT"), "text").expect("write A.TXT");
        fs::write(directory.join("ignored.md"), "ignored").expect("write ignored.md");

        let entries = fallback_directory_entries(&directory).expect("read fallback directory");
        let labels = entries.iter().map(|entry| entry.label.as_str()).collect::<Vec<_>>();
        assert_eq!(labels, ["a-dir", "z-dir", "A.TXT", "b.lua"]);
        assert!(entries[0].directory && entries[1].directory);
        assert!(!entries[2].directory && !entries[3].directory);

        let mut state = ScriptFileDialogFallbackState {
            open: true,
            directory: directory.clone(),
            selected: None,
            path_text: "b.lua".to_owned(),
            error: String::new(),
        };
        assert_eq!(fallback_selected_path(&state), Ok(directory.join("b.lua")));
        state.path_text = "ignored.md".to_owned();
        assert_eq!(fallback_selected_path(&state), Err("Unsupported script extension.".to_owned()));

        fs::remove_dir_all(&directory).expect("remove fallback directory");
    }

    #[test]
    fn fallback_renderer_keeps_modal_active_in_headless_imgui_frame() {
        let mut context = dear_imgui_rs::Context::create();
        let _ = context.set_ini_filename::<PathBuf>(None);
        context
            .font_atlas()
            .try_claim_legacy_renderer()
            .expect("legacy renderer font atlas")
            .build();
        context.io_mut().set_display_size([1180.0, 700.0]);
        context.io_mut().set_delta_time(1.0 / 60.0);
        let mut state = ScriptUiState::default();
        state.open_file_dialog_fallback();
        let frame = context.begin_frame();
        render_file_dialog_fallback(frame.ui(), &mut state);
        assert!(state.file_dialog_fallback_open());
    }

    #[test]
    fn label_falls_back_to_path_stem() {
        assert_eq!(script_label(&snapshot("/tmp/example.lua")), "example");
    }
}

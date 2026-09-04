//! App-side imported-script UI state and rendering.
//!
//! The Lua manager remains the owner of records/workers. This layer only renders immutable
//! snapshots, queues typed actions, and owns transient selection/import-preview state.

use crate::{keybind, theme::ThemeDefinition};
use dear_imgui_rs::{StyleColor, Ui};
use serde_json::{Value, json};
use smu_scripting::{ScriptRecordSnapshot, SettingsUiControl, SettingsUiControlKind};
use std::fs;
use std::path::{Path, PathBuf};

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
}

impl ScriptUiState {
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

pub fn render_sidebar(
    ui: &Ui,
    state: &mut ScriptUiState,
    left_panel_width: f32,
    theme: &ThemeDefinition,
) -> Option<ScriptAction> {
    ui.spacing();
    ui.text_wrapped("Imported Scripts");
    let item_width =
        (left_panel_width - if ui.scroll_max_y() == 0.0 { 7.0 } else { 18.0 }).max(1.0);
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
    render_script_details(ui, &script, state.import_notice.is_empty(), theme);
    crate::parity::separator(ui);
    ui.text_wrapped("Custom Settings");
    if state.controls.is_empty() {
        ui.text_wrapped("This script does not define onSettings(), or no controls were captured.");
        return None;
    }
    if script.running {
        ui.text_wrapped(
            "Settings are read-only while the script is running. Dynamic text stays live and copyable.",
        );
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
        150.0,
        50.0,
        true,
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
    ui.text_wrapped("Disable outside of Target Application:");
    ui.same_line();
    let mut disable_outside = script.disable_outside_roblox;
    if ui.checkbox(format!("##ImportedScriptDisableOutside{index}"), &mut disable_outside) {
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

fn render_script_details(
    ui: &Ui,
    script: &ScriptRecordSnapshot,
    show_persistence_note: bool,
    theme: &ThemeDefinition,
) {
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
    if show_persistence_note {
        ui.text_disabled("Script persistence through Rust profiles is not attached yet.");
    }
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

pub fn render_trust_modal(
    ui: &Ui,
    state: &mut ScriptUiState,
    theme: &ThemeDefinition,
) -> Option<ScriptAction> {
    if state.open_trust_modal {
        let _ = ui.open_popup("Import Script?");
        state.open_trust_modal = false;
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
    crate::parity::separator(ui);
    ui.text_wrapped("Preview");
    ui.text_disabled("Read-only source preview. Review scripts before importing them.");
    if state.preview_error.is_empty() {
        if state.preview.is_empty() {
            ui.text_disabled("The selected file is empty.");
        }
        let mut preview = state.preview.clone();
        let height = ui.content_region_avail()[1].max(120.0);
        let _ = ui
            .input_text_multiline(
                "##ImportScriptPreview",
                &mut preview,
                [-f32::MIN_POSITIVE, height],
            )
            .read_only(true)
            .build();
    } else {
        ui.text_colored(theme.error_color, &state.preview_error);
    }
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
            last_error: String::new(),
            last_warning: String::new(),
            ui_state: json!({}),
        }
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
    fn label_falls_back_to_path_stem() {
        assert_eq!(script_label(&snapshot("/tmp/example.lua")), "example");
    }
}

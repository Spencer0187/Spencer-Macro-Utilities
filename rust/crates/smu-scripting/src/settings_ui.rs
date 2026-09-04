//! Script settings-UI state and capture model from `ScriptInstance`.

use crate::persistence::{MAX_UI_STATE_ENTRIES, MAX_UI_STATE_STRING_BYTES};
use serde_json::{Map, Value};
use smu_core::Hotkey;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

pub const MAX_UI_CONTROLS_PER_SETTINGS_CALL: usize = 512;
pub const MAX_UI_DROPDOWN_ITEMS: usize = 256;
pub const MAX_UI_DIMENSION: f32 = 10_000.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsUiControlKind {
    Text,
    Separator,
    Checkbox,
    SliderInt,
    SliderFloat,
    Dropdown,
    Textbox,
    DynamicTextbox,
    Keybind,
    Button,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettingsUiControl {
    pub kind: SettingsUiControlKind,
    pub id: String,
    pub label: String,
    pub text: String,
    pub default_text: String,
    pub items: Vec<String>,
    pub default_bool: bool,
    pub default_int: i32,
    pub min_int: i32,
    pub max_int: i32,
    pub default_float: f64,
    pub min_float: f64,
    pub max_float: f64,
    pub default_keybind: Hotkey,
    pub width: f32,
    pub height: f32,
}

impl SettingsUiControl {
    const fn new(kind: SettingsUiControlKind) -> Self {
        Self {
            kind,
            id: String::new(),
            label: String::new(),
            text: String::new(),
            default_text: String::new(),
            items: Vec::new(),
            default_bool: false,
            default_int: 0,
            min_int: 0,
            max_int: 0,
            default_float: 0.0,
            min_float: 0.0,
            max_float: 0.0,
            default_keybind: Hotkey::new(0),
            width: 0.0,
            height: 0.0,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SettingsUiCapture {
    active: bool,
    control_count: usize,
    controls: Vec<SettingsUiControl>,
    pending: Vec<SettingsUiControl>,
}

impl SettingsUiCapture {
    pub const fn reset_control_count(&mut self) {
        self.control_count = 0;
    }

    #[must_use]
    pub const fn try_consume_control(&mut self) -> bool {
        if self.control_count >= MAX_UI_CONTROLS_PER_SETTINGS_CALL {
            return false;
        }
        self.control_count += 1;
        true
    }

    pub fn begin(&mut self) {
        self.active = true;
        self.pending.clear();
    }

    pub fn finish(&mut self, commit: bool) {
        if commit {
            self.controls = std::mem::take(&mut self.pending);
        } else {
            self.pending.clear();
        }
        self.pending.clear();
        self.active = false;
    }

    #[must_use]
    pub fn controls(&self) -> &[SettingsUiControl] {
        &self.controls
    }

    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.active
    }

    fn record(&mut self, control: SettingsUiControl) {
        if self.active {
            self.pending.push(control);
        }
    }

    pub fn record_text(&mut self, text: String, width: f32) {
        let mut control = SettingsUiControl::new(SettingsUiControlKind::Text);
        control.text = text;
        control.width = width;
        self.record(control);
    }

    pub fn record_separator(&mut self, height: f32) {
        let mut control = SettingsUiControl::new(SettingsUiControlKind::Separator);
        control.height = height;
        self.record(control);
    }

    pub fn record_checkbox(&mut self, id: String, label: String, default_value: bool, width: f32) {
        let mut control = SettingsUiControl::new(SettingsUiControlKind::Checkbox);
        control.id = id;
        control.label = label;
        control.default_bool = default_value;
        control.width = width;
        self.record(control);
    }

    pub fn record_slider_int(
        &mut self,
        id: String,
        label: String,
        default_value: i32,
        min_value: i32,
        max_value: i32,
        width: f32,
    ) {
        let mut control = SettingsUiControl::new(SettingsUiControlKind::SliderInt);
        control.id = id;
        control.label = label;
        control.default_int = default_value;
        control.min_int = min_value;
        control.max_int = max_value;
        control.width = width;
        self.record(control);
    }

    pub fn record_slider_float(
        &mut self,
        id: String,
        label: String,
        default_value: f64,
        min_value: f64,
        max_value: f64,
        width: f32,
    ) {
        let mut control = SettingsUiControl::new(SettingsUiControlKind::SliderFloat);
        control.id = id;
        control.label = label;
        control.default_float = default_value;
        control.min_float = min_value;
        control.max_float = max_value;
        control.width = width;
        self.record(control);
    }

    pub fn record_dropdown(
        &mut self,
        id: String,
        label: String,
        items: Vec<String>,
        default_value: String,
        width: f32,
    ) {
        let mut control = SettingsUiControl::new(SettingsUiControlKind::Dropdown);
        control.id = id;
        control.label = label;
        control.items = items;
        control.default_text = default_value;
        control.width = width;
        self.record(control);
    }

    pub fn record_textbox(
        &mut self,
        id: String,
        label: String,
        default_value: String,
        width: f32,
        height: f32,
    ) {
        let mut control = SettingsUiControl::new(SettingsUiControlKind::Textbox);
        control.id = id;
        control.label = label;
        control.default_text = default_value;
        control.width = width;
        control.height = height;
        self.record(control);
    }

    pub fn record_dynamic_textbox(
        &mut self,
        id: String,
        label: String,
        default_value: String,
        width: f32,
        height: f32,
    ) {
        let mut control = SettingsUiControl::new(SettingsUiControlKind::DynamicTextbox);
        control.id = id;
        control.label = label;
        control.default_text = default_value;
        control.width = width;
        control.height = height;
        self.record(control);
    }

    pub fn record_keybind(&mut self, id: String, label: String, default_value: Hotkey, width: f32) {
        let mut control = SettingsUiControl::new(SettingsUiControlKind::Keybind);
        control.id = id;
        control.label = label;
        control.default_keybind = default_value;
        control.width = width;
        self.record(control);
    }

    pub fn record_button(&mut self, id: String, label: String, width: f32, height: f32) {
        let mut control = SettingsUiControl::new(SettingsUiControlKind::Button);
        control.id = id;
        control.label = label;
        control.width = width;
        control.height = height;
        self.record(control);
    }
}

#[derive(Clone, Debug, Default)]
pub struct ScriptUiCache {
    ids: HashSet<String>,
    transient: HashMap<String, String>,
    keybind_values: HashMap<String, Hotkey>,
}

impl ScriptUiCache {
    pub fn sync_ids(&mut self, ui_state: &Value) {
        self.ids.clear();
        let Some(object) = ui_state.as_object() else {
            return;
        };
        for key in object.keys() {
            if self.ids.len() >= MAX_UI_STATE_ENTRIES {
                break;
            }
            self.ids.insert(key.clone());
        }
    }

    #[must_use]
    pub fn try_register_id(&mut self, id: &str) -> bool {
        if self.ids.contains(id) {
            return true;
        }
        if self.ids.len() >= MAX_UI_STATE_ENTRIES {
            return false;
        }
        self.ids.insert(id.to_owned());
        true
    }

    pub fn set_transient(&mut self, key: &str, value: &str) -> bool {
        if !self.try_register_id(key) {
            return false;
        }
        self.transient.insert(key.to_owned(), clamp_ui_string(value));
        true
    }

    #[must_use]
    pub fn transient(&self, key: &str) -> Option<&str> {
        self.transient.get(key).map(String::as_str)
    }

    pub fn keybind_value(&mut self, id: &str, default_value: Hotkey) -> Hotkey {
        *self.keybind_values.entry(id.to_owned()).or_insert(default_value)
    }

    pub fn set_keybind_value(&mut self, id: &str, value: Hotkey) {
        self.keybind_values.insert(id.to_owned(), value);
    }

    #[must_use]
    pub fn registered_id_count(&self) -> usize {
        self.ids.len()
    }
}

fn clamp_ui_string(value: &str) -> String {
    if value.len() <= MAX_UI_STATE_STRING_BYTES {
        return value.to_owned();
    }
    let mut boundary = MAX_UI_STATE_STRING_BYTES;
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value[..boundary].to_owned()
}

#[derive(Clone, Debug, Default)]
pub struct ScriptSettingsUiContext {
    state: Arc<Mutex<ScriptSettingsUiState>>,
}

#[derive(Clone, Debug, Default)]
struct ScriptSettingsUiState {
    ui_state: Map<String, Value>,
    capture: SettingsUiCapture,
    cache: ScriptUiCache,
    pending_values: HashMap<String, Value>,
    pressed_buttons: HashSet<String>,
}

impl ScriptSettingsUiContext {
    #[must_use]
    pub fn from_ui_state(value: &Value) -> Self {
        let mut state = ScriptSettingsUiState::default();
        if let Some(object) = value.as_object() {
            state.ui_state.clone_from(object);
        }
        state.cache.sync_ids(&Value::Object(state.ui_state.clone()));
        Self { state: Arc::new(Mutex::new(state)) }
    }

    fn state(&self) -> MutexGuard<'_, ScriptSettingsUiState> {
        match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    pub fn begin_capture(&self) {
        let mut state = self.state();
        state.capture.begin();
        state.capture.reset_control_count();
    }

    pub fn finish_capture(&self, commit: bool) {
        self.state().capture.finish(commit);
    }

    #[must_use]
    pub fn controls(&self) -> Vec<SettingsUiControl> {
        self.state().capture.controls().to_vec()
    }

    #[must_use]
    pub fn ui_state(&self) -> Value {
        Value::Object(self.state().ui_state.clone())
    }

    pub fn replace_ui_state(&self, value: &Value) {
        let mut state = self.state();
        state.ui_state = value.as_object().cloned().unwrap_or_default();
        let snapshot = Value::Object(state.ui_state.clone());
        state.cache.sync_ids(&snapshot);
        state.pending_values.clear();
        state.pressed_buttons.clear();
    }

    /// Queue a value that the app renderer should present as the next user edit for this control.
    pub fn queue_value(&self, id: impl Into<String>, value: Value) {
        self.state().pending_values.insert(id.into(), value);
    }

    /// Queue one button click for the next settings render call.
    pub fn queue_button_press(&self, id: impl Into<String>) {
        self.state().pressed_buttons.insert(id.into());
    }

    pub(crate) fn try_consume_control(&self) -> bool {
        self.state().capture.try_consume_control()
    }

    pub(crate) fn try_register_id(&self, id: &str) -> bool {
        self.state().cache.try_register_id(id)
    }

    pub(crate) fn record(&self, control: impl FnOnce(&mut SettingsUiCapture)) {
        control(&mut self.state().capture);
    }

    pub(crate) fn stored(&self, id: &str) -> Option<Value> {
        self.state().ui_state.get(id).cloned()
    }

    pub(crate) fn set_stored(&self, id: &str, value: Value) {
        let mut state = self.state();
        state.ui_state.insert(id.to_owned(), value);
        let _ = state.cache.try_register_id(id);
    }

    pub(crate) fn take_pending_value(&self, id: &str) -> Option<Value> {
        self.state().pending_values.remove(id)
    }

    pub(crate) fn take_button_press(&self, id: &str) -> bool {
        self.state().pressed_buttons.remove(id)
    }

    pub(crate) fn keybind_value(&self, id: &str, default_value: Hotkey) -> Hotkey {
        self.state().cache.keybind_value(id, default_value)
    }

    pub(crate) fn set_keybind_value(&self, id: &str, value: Hotkey) {
        self.state().cache.set_keybind_value(id, value);
    }

    pub(crate) fn set_transient(&self, id: &str, value: &str) -> bool {
        self.state().cache.set_transient(id, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, json};

    #[test]
    fn control_budget_is_exactly_512_per_reset() {
        let mut capture = SettingsUiCapture::default();
        for _ in 0..MAX_UI_CONTROLS_PER_SETTINGS_CALL {
            assert!(capture.try_consume_control());
        }
        assert!(!capture.try_consume_control());
        capture.reset_control_count();
        assert!(capture.try_consume_control());
    }

    #[test]
    fn failed_capture_keeps_previous_committed_controls() {
        let mut capture = SettingsUiCapture::default();
        capture.begin();
        capture.record_text("old".to_owned(), 10.0);
        capture.finish(true);
        assert_eq!(capture.controls().len(), 1);
        assert_eq!(capture.controls()[0].text, "old");

        capture.begin();
        capture.record_text("new".to_owned(), 20.0);
        capture.finish(false);
        assert_eq!(capture.controls().len(), 1);
        assert_eq!(capture.controls()[0].text, "old");
    }

    #[test]
    fn records_are_ignored_outside_capture() {
        let mut capture = SettingsUiCapture::default();
        capture.record_button("id".to_owned(), "label".to_owned(), 10.0, 20.0);
        capture.finish(true);
        assert!(capture.controls().is_empty());
    }

    #[test]
    fn control_fields_match_cpp_record_helpers() {
        let mut capture = SettingsUiCapture::default();
        capture.begin();
        capture.record_slider_int("speed".to_owned(), "Speed".to_owned(), 5, 1, 10, 125.0);
        capture.record_keybind("bind".to_owned(), "Bind".to_owned(), Hotkey::new(0x2_0041), 90.0);
        capture.finish(true);
        assert_eq!(capture.controls()[0].kind, SettingsUiControlKind::SliderInt);
        assert_eq!(
            (
                capture.controls()[0].default_int,
                capture.controls()[0].min_int,
                capture.controls()[0].max_int
            ),
            (5, 1, 10)
        );
        assert_eq!(capture.controls()[1].default_keybind.raw(), 0x2_0041);
    }

    #[test]
    fn ui_id_cache_syncs_existing_keys_without_revalidating_them() {
        let mut cache = ScriptUiCache::default();
        cache.sync_ids(&json!({"": true, "existing": 1}));
        assert_eq!(cache.registered_id_count(), 2);
        assert!(cache.try_register_id("existing"));
        assert_eq!(cache.registered_id_count(), 2);
    }

    #[test]
    fn transient_values_share_id_limit_and_clamp_strings() {
        let mut state = Map::new();
        for index in 0..MAX_UI_STATE_ENTRIES {
            state.insert(format!("id-{index:04}"), Value::Bool(true));
        }
        let mut cache = ScriptUiCache::default();
        cache.sync_ids(&Value::Object(state));
        assert!(!cache.set_transient("overflow", "x"));

        cache.sync_ids(&json!({}));
        assert!(cache.set_transient("ok", &"x".repeat(MAX_UI_STATE_STRING_BYTES + 20)));
        assert_eq!(cache.transient("ok").map(str::len), Some(MAX_UI_STATE_STRING_BYTES));
    }

    #[test]
    fn keybind_cache_inserts_default_only_once() {
        let mut cache = ScriptUiCache::default();
        assert_eq!(cache.keybind_value("bind", Hotkey::new(1)).raw(), 1);
        assert_eq!(cache.keybind_value("bind", Hotkey::new(2)).raw(), 1);
        cache.set_keybind_value("bind", Hotkey::new(3));
        assert_eq!(cache.keybind_value("bind", Hotkey::new(4)).raw(), 3);
    }
}

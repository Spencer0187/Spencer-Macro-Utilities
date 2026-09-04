//! Imported-script persisted state from `ScriptManager::serialize` / `deserialize`.

use serde_json::{Map, Number, Value};
use smu_core::Hotkey;
use std::path::PathBuf;

pub const SCRIPT_UNBOUND_HOTKEY: u32 = 0x1000_0000;
pub const MAX_UI_ID_BYTES: usize = 128;
pub const MAX_UI_STATE_STRING_BYTES: usize = 4096;
pub const MAX_UI_STATE_ENTRIES: usize = 4096;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SavedScriptRecord {
    pub path: PathBuf,
    pub hotkey: Hotkey,
    pub enabled: bool,
    pub disable_outside_roblox: bool,
    pub ui_state: Value,
}

impl SavedScriptRecord {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            hotkey: Hotkey::new(SCRIPT_UNBOUND_HOTKEY),
            enabled: false,
            disable_outside_roblox: true,
            ui_state: Value::Object(Map::new()),
        }
    }
}

#[must_use]
pub const fn normalize_script_hotkey(hotkey: u32) -> Hotkey {
    let parsed = Hotkey::new(hotkey);
    if parsed.is_bound() { parsed } else { Hotkey::new(SCRIPT_UNBOUND_HOTKEY) }
}

#[must_use]
pub fn is_safe_ui_state_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= MAX_UI_ID_BYTES && !key.as_bytes().contains(&0)
}

fn clamp_string_bytes(value: &str) -> String {
    if value.len() <= MAX_UI_STATE_STRING_BYTES {
        return value.to_owned();
    }
    let mut boundary = MAX_UI_STATE_STRING_BYTES;
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value[..boundary].to_owned()
}

fn sanitize_number(number: &Number) -> Option<Value> {
    if let Some(value) = number.as_i64() {
        return Some(Value::Number(Number::from(value)));
    }
    if let Some(value) = number.as_u64() {
        // nlohmann::json reports unsigned integers as `is_number_integer()` too, so the C++ first
        // branch converts them through int64_t. Values above INT64_MAX therefore wrap.
        #[allow(clippy::cast_possible_wrap)]
        let wrapped = value as i64;
        return Some(Value::Number(Number::from(wrapped)));
    }
    let value = number.as_f64()?;
    Number::from_f64(value).map(Value::Number)
}

/// Sanitize imported-script UI state exactly at the persistence boundary used by C++ deserialize.
#[must_use]
pub fn sanitize_ui_state(state: &Value) -> Value {
    let Some(object) = state.as_object() else {
        return Value::Object(Map::new());
    };
    let mut sanitized = Map::new();
    for (key, value) in object {
        if sanitized.len() >= MAX_UI_STATE_ENTRIES {
            break;
        }
        if !is_safe_ui_state_key(key) {
            continue;
        }

        let accepted = match value {
            Value::Bool(value) => Some(Value::Bool(*value)),
            Value::Number(number) => sanitize_number(number),
            Value::String(text) => Some(Value::String(clamp_string_bytes(text))),
            Value::Null | Value::Array(_) | Value::Object(_) => None,
        };
        if let Some(value) = accepted {
            sanitized.insert(key.clone(), value);
        }
    }
    Value::Object(sanitized)
}

fn parse_saved_hotkey(value: Option<&Value>) -> Hotkey {
    let raw = value
        .and_then(Value::as_u64)
        .and_then(|number| u32::try_from(number).ok())
        .or_else(|| {
            value
                .and_then(Value::as_i64)
                .filter(|number| *number > 0)
                .and_then(|number| u32::try_from(number).ok())
        })
        .unwrap_or_default();
    normalize_script_hotkey(raw)
}

/// Parse saved script rows without touching the filesystem or loading Lua.
///
/// Invalid rows are skipped, matching the front half of C++ `ScriptManager::deserialize`.
#[must_use]
pub fn deserialize_saved_scripts(value: &Value) -> Vec<SavedScriptRecord> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    let mut records = Vec::new();
    for item in items {
        let Some(object) = item.as_object() else {
            continue;
        };
        let Some(path) = object.get("path").and_then(Value::as_str) else {
            continue;
        };
        let enabled = object.get("enabled").and_then(Value::as_bool).unwrap_or(false);
        let disable_outside_roblox =
            object.get("disable_outside_roblox").and_then(Value::as_bool).unwrap_or(true);
        let ui_state = object
            .get("ui_state")
            .filter(|state| state.is_object())
            .map_or_else(|| Value::Object(Map::new()), sanitize_ui_state);
        records.push(SavedScriptRecord {
            path: PathBuf::from(path),
            hotkey: parse_saved_hotkey(object.get("hotkey")),
            enabled,
            disable_outside_roblox,
            ui_state,
        });
    }
    records
}

/// Serialize the persistence fields in the same shape as C++ `ScriptManager::serialize`.
#[must_use]
pub fn serialize_saved_scripts(records: &[SavedScriptRecord]) -> Value {
    Value::Array(
        records
            .iter()
            .map(|record| {
                let mut item = Map::new();
                item.insert(
                    "path".to_owned(),
                    Value::String(record.path.to_string_lossy().into_owned()),
                );
                item.insert("hotkey".to_owned(), Value::Number(Number::from(record.hotkey.raw())));
                item.insert("enabled".to_owned(), Value::Bool(record.enabled));
                item.insert(
                    "disable_outside_roblox".to_owned(),
                    Value::Bool(record.disable_outside_roblox),
                );
                item.insert("ui_state".to_owned(), record.ui_state.clone());
                Value::Object(item)
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use serde_json::json;

    #[test]
    fn unbound_hotkeys_use_script_sentinel() {
        assert_eq!(normalize_script_hotkey(0).raw(), SCRIPT_UNBOUND_HOTKEY);
        assert_eq!(normalize_script_hotkey(0x3_0000).raw(), SCRIPT_UNBOUND_HOTKEY);
        assert_eq!(normalize_script_hotkey(0x2_0041).raw(), 0x2_0041);
    }

    #[test]
    fn ui_state_accepts_only_cpp_scalar_types() {
        assert_eq!(
            sanitize_ui_state(&json!({
                "bool": true,
                "int": -5,
                "uint": 9,
                "float": 1.25,
                "text": "hello",
                "null": null,
                "array": [1],
                "object": {"nested": true}
            })),
            json!({"bool": true, "float": 1.25, "int": -5, "text": "hello", "uint": 9})
        );
    }

    #[test]
    fn unsigned_values_above_i64_max_preserve_cpp_wrap_quirk() {
        let mut state = Map::new();
        state.insert("max".to_owned(), Value::Number(Number::from(u64::MAX)));
        assert_eq!(sanitize_ui_state(&Value::Object(state)), json!({"max": -1}));
    }

    #[test]
    fn invalid_ui_keys_are_skipped_and_strings_are_byte_capped() {
        let long_key = "k".repeat(MAX_UI_ID_BYTES + 1);
        let long_text = "x".repeat(MAX_UI_STATE_STRING_BYTES + 10);
        let mut state = Map::new();
        state.insert(String::new(), json!(true));
        state.insert(long_key, json!(true));
        state.insert("has\0nul".to_owned(), json!(true));
        state.insert("ok".to_owned(), Value::String(long_text));
        let sanitized = sanitize_ui_state(&Value::Object(state));
        let object = sanitized.as_object().expect("sanitized UI state should be an object");
        assert_eq!(object.len(), 1);
        assert_eq!(object["ok"].as_str().map(str::len), Some(MAX_UI_STATE_STRING_BYTES));
    }

    #[test]
    fn saved_rows_apply_cpp_defaults_and_numeric_hotkey_rules() {
        let records = deserialize_saved_scripts(&json!([
            {"path": "a.lua"},
            {"path": "b.lua", "hotkey": -1, "enabled": "yes", "disable_outside_roblox": false},
            {"path": "c.lua", "hotkey": 4_294_967_296_u64, "enabled": true},
            {"path": "d.lua", "hotkey": 131_137, "ui_state": {"nested": {}, "ok": "value"}},
            {"path": 5},
            null
        ]));
        assert_eq!(records.len(), 4);
        assert_eq!(records[0].hotkey.raw(), SCRIPT_UNBOUND_HOTKEY);
        assert!(!records[0].enabled);
        assert!(records[0].disable_outside_roblox);
        assert_eq!(records[1].hotkey.raw(), SCRIPT_UNBOUND_HOTKEY);
        assert!(!records[1].enabled);
        assert!(!records[1].disable_outside_roblox);
        assert_eq!(records[2].hotkey.raw(), SCRIPT_UNBOUND_HOTKEY);
        assert!(records[2].enabled);
        assert_eq!(records[3].hotkey.raw(), 131_137);
        assert_eq!(records[3].ui_state, json!({"ok": "value"}));
    }

    #[test]
    fn serialize_preserves_current_ui_state_without_resanitizing() {
        let mut record = SavedScriptRecord::new("script.lua");
        record.hotkey = Hotkey::new(0x2_0041);
        record.enabled = true;
        record.disable_outside_roblox = false;
        record.ui_state = json!({"nested": {"kept": true}});
        assert_eq!(
            serialize_saved_scripts(&[record]),
            json!([{
                "path": "script.lua",
                "hotkey": 131_137,
                "enabled": true,
                "disable_outside_roblox": false,
                "ui_state": {"nested": {"kept": true}}
            }])
        );
    }
}

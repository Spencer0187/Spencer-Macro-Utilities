//! Preservation-safe live profile helpers shared by app UI state adapters.

use serde_json::{Map, Number, Value};
use smu_profile::payload::{PreservedProfilePayload, ProfilePayloadState};
use smu_profile::{SavedSettingSource, SavedSettingValue};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

#[derive(Debug, Default)]
pub struct LiveSavedSettings {
    state: RwLock<ProfilePayloadState>,
}

impl LiveSavedSettings {
    pub fn replace(&self, payload: &PreservedProfilePayload) {
        *write_lock(&self.state) = payload.normalized().clone();
    }
}

impl SavedSettingSource for LiveSavedSettings {
    fn get_saved_setting(&self, name: &str) -> Option<SavedSettingValue> {
        SavedSettingSource::get_saved_setting(&*read_lock(&self.state), name)
    }
}

fn read_lock<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    match lock.read() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn write_lock<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    match lock.write() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[must_use]
pub fn has(payload: &PreservedProfilePayload, key: &str) -> bool {
    payload.raw_value(key).is_some()
}

#[must_use]
pub fn bool_value(payload: &PreservedProfilePayload, key: &str) -> Option<bool> {
    has(payload, key).then(|| payload.normalized_value(key)?.as_bool()).flatten()
}

#[must_use]
pub fn i32_value(payload: &PreservedProfilePayload, key: &str) -> Option<i32> {
    if !has(payload, key) {
        return None;
    }
    i32::try_from(payload.normalized_value(key)?.as_i64()?).ok()
}

#[must_use]
pub fn u32_value(payload: &PreservedProfilePayload, key: &str) -> Option<u32> {
    if !has(payload, key) {
        return None;
    }
    u32::try_from(payload.normalized_value(key)?.as_u64()?).ok()
}

#[must_use]
pub fn f32_value(payload: &PreservedProfilePayload, key: &str) -> Option<f32> {
    if !has(payload, key) {
        return None;
    }
    let value = payload.normalized_value(key)?.as_f64()?;
    if !value.is_finite() || value < f64::from(f32::MIN) || value > f64::from(f32::MAX) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    Some(value as f32)
}

#[must_use]
pub fn string_value<'a>(payload: &'a PreservedProfilePayload, key: &str) -> Option<&'a str> {
    if !has(payload, key) {
        return None;
    }
    payload.normalized_value(key)?.as_str()
}

pub fn insert_bool(overlay: &mut Map<String, Value>, key: &str, value: bool) {
    overlay.insert(key.to_owned(), Value::Bool(value));
}

pub fn insert_i32(overlay: &mut Map<String, Value>, key: &str, value: i32) {
    overlay.insert(key.to_owned(), Value::Number(Number::from(value)));
}

pub fn insert_u32(overlay: &mut Map<String, Value>, key: &str, value: u32) {
    overlay.insert(key.to_owned(), Value::Number(Number::from(value)));
}

pub fn insert_f32(overlay: &mut Map<String, Value>, key: &str, value: f32) {
    if let Some(number) = Number::from_f64(f64::from(value)) {
        overlay.insert(key.to_owned(), Value::Number(number));
    }
}

pub fn insert_string(overlay: &mut Map<String, Value>, key: &str, value: impl Into<String>) {
    overlay.insert(key.to_owned(), Value::String(value.into()));
}

#[must_use]
pub fn section_flag(payload: &PreservedProfilePayload, key: &str, index: usize) -> Option<bool> {
    payload.raw_value(key)?.as_array()?.get(index)?.as_bool()
}

pub fn overlay_section_flag(
    baseline: &ProfilePayloadState,
    overlay: &mut Map<String, Value>,
    key: &str,
    index: usize,
    value: bool,
) {
    let mut values = overlay
        .get(key)
        .or_else(|| baseline.value(key))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if values.len() <= index {
        values.resize(index + 1, Value::Bool(false));
    }
    values[index] = Value::Bool(value);
    overlay.insert(key.to_owned(), Value::Array(values));
}

#[must_use]
pub fn object_bool(object: &Map<String, Value>, key: &str) -> Option<bool> {
    object.get(key)?.as_bool()
}

#[must_use]
pub fn object_i32(object: &Map<String, Value>, key: &str) -> Option<i32> {
    i32::try_from(object.get(key)?.as_i64()?).ok()
}

#[must_use]
pub fn object_u32(object: &Map<String, Value>, key: &str) -> Option<u32> {
    u32::try_from(object.get(key)?.as_u64()?).ok()
}

#[must_use]
pub fn object_f32(object: &Map<String, Value>, key: &str) -> Option<f32> {
    let value = object.get(key)?.as_f64()?;
    if !value.is_finite() || value < f64::from(f32::MIN) || value > f64::from(f32::MAX) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    Some(value as f32)
}

#[must_use]
pub fn object_string<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    object.get(key)?.as_str()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::LiveSavedSettings;
    use serde_json::json;
    use smu_profile::payload::PreservedProfilePayload;
    use smu_profile::{SavedSettingSource, SavedSettingValue};

    #[test]
    fn live_saved_settings_exposes_normalized_char_buffer_precedence() {
        let payload = PreservedProfilePayload::from_value(&json!({
            "ItemClipDelay": "81",
            "clip_delay": 3,
            "unknown": true
        }))
        .expect("payload should validate");
        let live = LiveSavedSettings::default();
        live.replace(&payload);
        assert_eq!(live.get_saved_setting("clip_delay"), Some(SavedSettingValue::Integer(81)));
        assert_eq!(live.get_saved_setting("missing_key"), None);
    }
}

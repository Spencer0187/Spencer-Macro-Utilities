//! Script-facing saved-setting values.

use crate::payload::ProfilePayloadState;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub enum SavedSettingValue {
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
}

/// Read-only script-facing saved-setting source.
pub trait SavedSettingSource: Send + Sync {
    fn get_saved_setting(&self, name: &str) -> Option<SavedSettingValue>;
}

fn from_json(value: &Value) -> Option<SavedSettingValue> {
    match value {
        Value::Bool(value) => Some(SavedSettingValue::Bool(*value)),
        Value::Number(number) => number
            .as_i64()
            .map(SavedSettingValue::Integer)
            .or_else(|| number.as_f64().map(SavedSettingValue::Float)),
        Value::String(value) => Some(SavedSettingValue::String(value.clone())),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

impl SavedSettingSource for ProfilePayloadState {
    fn get_saved_setting(&self, name: &str) -> Option<SavedSettingValue> {
        self.value(name).and_then(from_json)
    }
}

#[cfg(test)]
mod tests {
    use super::{SavedSettingSource, SavedSettingValue};
    use crate::payload::ProfilePayloadState;

    #[test]
    fn payload_state_exposes_scalar_saved_values() {
        let state = ProfilePayloadState::default();
        assert_eq!(state.get_saved_setting("camfixtoggle"), Some(SavedSettingValue::Bool(false)));
        assert_eq!(
            state.get_saved_setting("RobloxSensValue"),
            Some(SavedSettingValue::String("0.5".to_owned()))
        );
        assert_eq!(state.get_saved_setting("screen_width"), Some(SavedSettingValue::Integer(853)));
        assert_eq!(state.get_saved_setting("imported_scripts"), None);
        assert_eq!(state.get_saved_setting("not_a_setting"), None);
    }
}

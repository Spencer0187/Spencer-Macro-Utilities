//! Compatibility-first handling of the existing SMU profile container format.

use serde_json::{Map, Value};
use thiserror::Error;

pub const METADATA_KEY: &str = "_metadata";
pub const LAST_ACTIVE_PROFILE_KEY: &str = "last_active_profile";
pub const DEFAULT_PROFILE_NAME: &str = "(default)";

/// High-level container format detected before individual settings are decoded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContainerFormat {
    Profile,
    LegacyFlat,
    Unknown,
}

/// Errors that prevent the JSON container from being used at all.
#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("settings root must be a JSON object")]
    RootNotObject,
}

/// Returns true for the current profile-container format used by the C++ implementation.
#[must_use]
pub fn is_profile_format(root: &Value) -> bool {
    root.as_object()
        .is_some_and(|object| object.get(METADATA_KEY).is_some_and(serde_json::Value::is_object))
}

/// Mirrors the C++ legacy-flat detector using a caller-provided set of recognizable setting keys.
#[must_use]
pub fn is_legacy_flat_format<'a>(
    root: &Value,
    recognizable_keys: impl IntoIterator<Item = &'a str>,
) -> bool {
    let Some(object) = root.as_object() else {
        return false;
    };
    if object.contains_key(METADATA_KEY) {
        return false;
    }
    recognizable_keys.into_iter().any(|key| object.contains_key(key)) || object.contains_key("text")
}

#[must_use]
pub fn detect_format<'a>(
    root: &Value,
    recognizable_keys: impl IntoIterator<Item = &'a str>,
) -> ContainerFormat {
    if is_profile_format(root) {
        ContainerFormat::Profile
    } else if is_legacy_flat_format(root, recognizable_keys) {
        ContainerFormat::LegacyFlat
    } else {
        ContainerFormat::Unknown
    }
}

/// Mirrors `FindBestProfile` from the C++ oracle.
///
/// Priority is requested profile, metadata last-active profile, then the last non-default object in
/// JSON object iteration order. `(default)` is never selected as a fallback.
#[must_use]
pub fn find_best_profile(root: &Value, requested_name: &str) -> Option<String> {
    let object = root.as_object()?;

    if !requested_name.is_empty()
        && object.get(requested_name).is_some_and(serde_json::Value::is_object)
    {
        return Some(requested_name.to_owned());
    }

    if let Some(last_active) = object
        .get(METADATA_KEY)
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get(LAST_ACTIVE_PROFILE_KEY))
        .and_then(Value::as_str)
        && !last_active.is_empty()
        && last_active != DEFAULT_PROFILE_NAME
        && object.get(last_active).is_some_and(serde_json::Value::is_object)
    {
        return Some(last_active.to_owned());
    }

    object
        .iter()
        .filter(|(name, value)| {
            name.as_str() != METADATA_KEY
                && name.as_str() != DEFAULT_PROFILE_NAME
                && value.is_object()
        })
        .map(|(name, _)| name.clone())
        .next_back()
}

/// Ensures metadata exists without changing unrelated profile data.
///
/// # Errors
///
/// Returns [`ProfileError::RootNotObject`] when the settings root is not a JSON object.
pub fn metadata_mut(root: &mut Value) -> Result<&mut Map<String, Value>, ProfileError> {
    let object = root.as_object_mut().ok_or(ProfileError::RootNotObject)?;
    if !object.get(METADATA_KEY).is_some_and(Value::is_object) {
        object.insert(METADATA_KEY.to_owned(), Value::Object(Map::new()));
    }
    object.get_mut(METADATA_KEY).and_then(Value::as_object_mut).ok_or(ProfileError::RootNotObject)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use serde_json::json;

    use super::{
        ContainerFormat, DEFAULT_PROFILE_NAME, detect_format, find_best_profile, metadata_mut,
    };

    const LEGACY_KEYS: [&str; 3] = ["macrotoggled", "wallhopswitch", "screen_width"];

    #[test]
    fn current_profile_format_requires_object_metadata() {
        assert_eq!(
            detect_format(&json!({"_metadata": {}, "Profile 1": {}}), LEGACY_KEYS),
            ContainerFormat::Profile
        );
        assert_eq!(
            detect_format(&json!({"_metadata": false, "macrotoggled": true}), LEGACY_KEYS),
            ContainerFormat::Unknown
        );
    }

    #[test]
    fn legacy_flat_detection_matches_cpp_shape() {
        assert_eq!(
            detect_format(&json!({"wallhopswitch": true}), LEGACY_KEYS),
            ContainerFormat::LegacyFlat
        );
        assert_eq!(
            detect_format(&json!({"text": "hello"}), LEGACY_KEYS),
            ContainerFormat::LegacyFlat
        );
    }

    #[test]
    fn best_profile_priority_matches_cpp() {
        let root = json!({
            "_metadata": {"last_active_profile": "Profile 2"},
            "(default)": {},
            "Profile 1": {},
            "Profile 2": {}
        });
        assert_eq!(find_best_profile(&root, "Profile 1").as_deref(), Some("Profile 1"));
        assert_eq!(find_best_profile(&root, "missing").as_deref(), Some("Profile 2"));
    }

    #[test]
    fn default_profile_is_never_resume_fallback() {
        let root = json!({
            "_metadata": {"last_active_profile": DEFAULT_PROFILE_NAME},
            "(default)": {}
        });
        assert_eq!(find_best_profile(&root, ""), None);
    }

    #[test]
    fn metadata_creation_preserves_profiles() {
        let mut root = json!({"Profile 1": {"macrotoggled": true}});
        let metadata = metadata_mut(&mut root).expect("test fixture is an object");
        metadata.insert("shortdescriptions".to_owned(), json!(true));
        assert_eq!(root["Profile 1"]["macrotoggled"], true);
        assert_eq!(root["_metadata"]["shortdescriptions"], true);
    }
}

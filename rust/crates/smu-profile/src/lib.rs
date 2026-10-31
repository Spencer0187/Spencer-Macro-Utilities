//! Compatibility-first handling of the existing SMU profile container format.

pub mod payload;
pub mod saved;

pub use saved::{SavedSettingSource, SavedSettingValue};

use serde::Serialize;
use serde_json::{Map, Value};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

pub const METADATA_KEY: &str = "_metadata";
pub const LAST_ACTIVE_PROFILE_KEY: &str = "last_active_profile";
pub const DEFAULT_PROFILE_NAME: &str = "(default)";
pub const CURRENT_SETTINGS_FILENAME: &str = "SMCSettings.json";
pub const LEGACY_SETTINGS_FILENAME: &str = "RMCSettings.json";

include!("generated_legacy_keys.rs");

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// High-level container format detected before individual settings are decoded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContainerFormat {
    Profile,
    LegacyFlat,
    Unknown,
}

/// Errors that prevent a profile operation from completing.
#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("settings root must be a JSON object")]
    RootNotObject,
    #[error("settings I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid settings JSON at {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

/// Returns true for the current profile-container format used by the C++ implementation.
#[must_use]
pub fn is_profile_format(root: &Value) -> bool {
    root.as_object().is_some_and(|object| object.get(METADATA_KEY).is_some_and(Value::is_object))
}

/// Mirrors the C++ legacy-flat detector using the exact generated production key set.
#[must_use]
pub fn is_legacy_flat_format(root: &Value) -> bool {
    let Some(object) = root.as_object() else {
        return false;
    };
    if object.contains_key(METADATA_KEY) {
        return false;
    }
    LEGACY_FORMAT_KEYS.iter().any(|key| object.contains_key(*key))
}

#[must_use]
pub fn detect_format(root: &Value) -> ContainerFormat {
    if is_profile_format(root) {
        ContainerFormat::Profile
    } else if is_legacy_flat_format(root) {
        ContainerFormat::LegacyFlat
    } else {
        ContainerFormat::Unknown
    }
}

/// Mirrors `FindBestProfile` from the C++ implementation.
///
/// C++ `nlohmann::json` objects use ordered map semantics here, so the final fallback is the
/// lexicographically greatest non-default object key. `(default)` is never selected implicitly.
#[must_use]
pub fn find_best_profile(root: &Value, requested_name: &str) -> Option<String> {
    let object = root.as_object()?;

    if !requested_name.is_empty() && object.get(requested_name).is_some_and(Value::is_object) {
        return Some(requested_name.to_owned());
    }

    if let Some(last_active) = object
        .get(METADATA_KEY)
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get(LAST_ACTIVE_PROFILE_KEY))
        .and_then(Value::as_str)
        && !last_active.is_empty()
        && last_active != DEFAULT_PROFILE_NAME
        && object.get(last_active).is_some_and(Value::is_object)
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
        .max()
}

/// Returns profile-object names using the same sort/default-first policy as C++.
#[must_use]
pub fn profile_names(root: &Value) -> Vec<String> {
    let Some(object) = root.as_object() else {
        return Vec::new();
    };

    let mut names: Vec<String> = object
        .iter()
        .filter(|(name, value)| name.as_str() != METADATA_KEY && value.is_object())
        .map(|(name, _)| name.clone())
        .collect();
    names.sort();

    if let Some(index) = names.iter().position(|name| name == DEFAULT_PROFILE_NAME) {
        let default = names.remove(index);
        names.insert(0, default);
    }
    names
}

/// Read-only profile catalog loaded through the same primary/backup recovery order as mutations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileCatalog {
    pub names: Vec<String>,
    pub preferred_profile: Option<String>,
    pub loaded_from_backup: bool,
}

/// Startup classification of the resolved settings path before the application performs any write.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StartupProfileSource {
    /// Neither the primary path nor its `.bak` exists.
    Missing,
    /// A source exists but neither primary nor backup can be parsed as JSON.
    Unreadable,
    /// Legacy flat settings that must be applied, materialized, and migrated to `Profile 1`.
    LegacyFlat { payload: payload::PreservedProfilePayload, loaded_from_backup: bool },
    /// An object container. `preferred_name == None` means C++ would create `Profile 1` from defaults;
    /// `preferred_payload == None` with a name means the chosen profile failed payload validation.
    Structured {
        preferred_name: Option<String>,
        preferred_payload: Option<payload::PreservedProfilePayload>,
        loaded_from_backup: bool,
    },
    /// Parseable JSON that is not an object (for example a scalar or array). C++ selects this source
    /// during resolution but refuses to load or overwrite it during startup.
    Unsupported { loaded_from_backup: bool },
}

/// Returns whether the primary settings path or its `.bak` exists.
#[must_use]
pub fn settings_source_exists(path: &Path) -> bool {
    has_settings_source(path)
}

/// Classifies one already-resolved settings path without mutating it.
#[must_use]
pub fn startup_profile_source_from_file(path: &Path) -> StartupProfileSource {
    if !has_settings_source(path) {
        return StartupProfileSource::Missing;
    }
    let Some((root, loaded_from_backup)) = read_json_file_with_backup(path) else {
        return StartupProfileSource::Unreadable;
    };
    if is_legacy_flat_format(&root) {
        let Some(payload) = payload::PreservedProfilePayload::from_value(&root) else {
            return StartupProfileSource::Unreadable;
        };
        return StartupProfileSource::LegacyFlat { payload, loaded_from_backup };
    }
    let Some(object) = root.as_object() else {
        return StartupProfileSource::Unsupported { loaded_from_backup };
    };
    let preferred_name = find_best_profile(&root, "");
    let preferred_payload = preferred_name
        .as_deref()
        .and_then(|name| object.get(name).and_then(payload::PreservedProfilePayload::from_value));
    StartupProfileSource::Structured { preferred_name, preferred_payload, loaded_from_backup }
}

/// Reads a profile catalog without modifying either the primary file or its backup.
#[must_use]
pub fn profile_catalog_from_file(path: &Path) -> Option<ProfileCatalog> {
    let (root, loaded_from_backup) = read_json_file_with_backup(path)?;
    Some(ProfileCatalog {
        names: profile_names(&root),
        preferred_profile: find_best_profile(&root, ""),
        loaded_from_backup,
    })
}

/// Reads one named profile payload through primary/backup recovery without modifying the file.
#[must_use]
pub fn profile_payload_from_file(
    path: &Path,
    profile_name: &str,
) -> Option<payload::PreservedProfilePayload> {
    let (root, _) = read_json_file_with_backup(path)?;
    let profile = root.as_object()?.get(profile_name)?;
    payload::PreservedProfilePayload::from_value(profile)
}

/// Returns a cloned `_metadata` object from the primary/backup settings source without mutating it.
#[must_use]
pub fn metadata_snapshot_from_file(path: &Path) -> Option<Map<String, Value>> {
    let (root, _) = read_json_file_with_backup(path)?;
    root.as_object()?.get(METADATA_KEY)?.as_object().cloned()
}

/// Overlays selected `_metadata` fields while preserving every unrelated metadata/root/profile key.
/// The same primary/backup recovery and atomic write path as profile mutations is used.
#[must_use]
pub fn overlay_metadata_in_file(path: &Path, overlay: &Map<String, Value>) -> bool {
    let mut root = match read_json_file_with_backup(path) {
        Some((root, _)) => root,
        None if !has_settings_source(path) => Value::Object(Map::new()),
        None => return false,
    };
    let Ok(metadata) = metadata_mut(&mut root) else {
        return false;
    };
    for (key, value) in overlay {
        metadata.insert(key.clone(), value.clone());
    }
    write_json_file(path, &root, false)
}

/// Replaces or creates one named profile and overlays selected `_metadata` fields in one write.
/// Unknown sibling profiles, root fields, and unrelated metadata are preserved exactly.
#[must_use]
pub fn upsert_profile_payload_with_metadata_in_file(
    path: &Path,
    profile_name: &str,
    profile_payload: &Value,
    make_active: bool,
    metadata_overlay: &Map<String, Value>,
) -> bool {
    if profile_name.is_empty() || profile_name == METADATA_KEY {
        return false;
    }
    let Some(preserved) = payload::PreservedProfilePayload::from_value(profile_payload) else {
        return false;
    };
    let mut root = match read_json_file_with_backup(path) {
        Some((root, _)) => root,
        None if !has_settings_source(path) => Value::Object(Map::new()),
        None => return false,
    };
    let Some(root_object) = root.as_object_mut() else {
        return false;
    };
    root_object.insert(profile_name.to_owned(), preserved.raw_snapshot());
    let Ok(metadata) = metadata_mut(&mut root) else {
        return false;
    };
    for (key, value) in metadata_overlay {
        metadata.insert(key.clone(), value.clone());
    }
    if make_active {
        // Match C++ SaveMetadata: the profile being saved as active is authoritative even if a caller's
        // preservation overlay happens to contain a stale last_active_profile value.
        metadata.insert(LAST_ACTIVE_PROFILE_KEY.to_owned(), Value::String(profile_name.to_owned()));
    }
    write_json_file(path, &root, false)
}

/// Replaces or creates one named profile payload while preserving every sibling profile and unknown
/// root field. The supplied payload must pass the C++ compatibility validator.
///
/// When `make_active` is true, `_metadata.last_active_profile` is updated in the same write.
#[must_use]
pub fn upsert_profile_payload_in_file(
    path: &Path,
    profile_name: &str,
    profile_payload: &Value,
    make_active: bool,
) -> bool {
    upsert_profile_payload_with_metadata_in_file(
        path,
        profile_name,
        profile_payload,
        make_active,
        &Map::new(),
    )
}

/// Replaces a legacy-flat settings document with one profile container, matching the C++
/// `TryLoadLastActiveProfile` deserialize-then-`SaveSettings("Profile 1")` conversion shape.
///
/// The caller supplies the already materialized live serializer payload. Legacy root keys are not
/// retained as siblings; only the named profile and active-profile metadata are written.
#[must_use]
pub fn replace_legacy_flat_with_profile_in_file(
    path: &Path,
    profile_name: &str,
    profile_payload: &Value,
) -> bool {
    if profile_name.is_empty()
        || profile_name == DEFAULT_PROFILE_NAME
        || profile_name == METADATA_KEY
    {
        return false;
    }
    let Some((root, _)) = read_json_file_with_backup(path) else {
        return false;
    };
    if !is_legacy_flat_format(&root) {
        return false;
    }
    let Some(preserved) = payload::PreservedProfilePayload::from_value(profile_payload) else {
        return false;
    };
    let converted = json_container_with_active_profile(profile_name, preserved.raw_snapshot());
    write_json_file(path, &converted, false)
}

fn json_container_with_active_profile(profile_name: &str, profile_payload: Value) -> Value {
    let mut metadata = Map::new();
    metadata.insert(LAST_ACTIVE_PROFILE_KEY.to_owned(), Value::String(profile_name.to_owned()));
    let mut root = Map::new();
    root.insert(METADATA_KEY.to_owned(), Value::Object(metadata));
    root.insert(profile_name.to_owned(), profile_payload);
    Value::Object(root)
}

/// Produces the C++ profile UI's first available `Profile N` name.
#[must_use]
pub fn generate_new_profile_name(existing_names: &[String]) -> String {
    for index in 1..=1000 {
        let candidate = format!("Profile {index}");
        if !existing_names.contains(&candidate) {
            return candidate;
        }
    }
    "Profile Error".to_owned()
}

/// Produces the same duplicate-profile default name used by the C++ profile UI.
#[must_use]
pub fn generate_unique_profile_name(base_name: &str, existing_names: &[String]) -> String {
    let first = format!("{base_name} (Copy)");
    if !existing_names.contains(&first) {
        return first;
    }

    for index in 2..=1000 {
        let candidate = format!("{base_name} (Copy {index})");
        if !existing_names.contains(&candidate) {
            return candidate;
        }
    }
    format!("{base_name} (Copy Error)")
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

/// Applies the in-memory portion of C++ `DuplicateProfileInFile`.
#[must_use]
pub fn duplicate_profile(root: &mut Value, source_name: &str, new_name: &str) -> bool {
    let Some(object) = root.as_object_mut() else {
        return false;
    };
    if object.contains_key(new_name) {
        return false;
    }
    let Some(source) = object.get(source_name).cloned() else {
        return false;
    };
    object.insert(new_name.to_owned(), source);
    true
}

/// Applies the in-memory portion of C++ `RenameProfileInFile`.
///
/// Matching C++, equal old/new names short-circuit to success before any file or profile checks.
#[must_use]
pub fn rename_profile(root: &mut Value, old_name: &str, new_name: &str) -> bool {
    if old_name == new_name {
        return true;
    }
    if old_name == DEFAULT_PROFILE_NAME || new_name == DEFAULT_PROFILE_NAME {
        return false;
    }
    let Some(object) = root.as_object_mut() else {
        return false;
    };
    if object.contains_key(new_name) {
        return false;
    }
    let Some(profile) = object.remove(old_name) else {
        return false;
    };
    object.insert(new_name.to_owned(), profile);
    true
}

/// Applies the in-memory portion of C++ `DeleteProfileFromFile`.
#[must_use]
pub fn delete_profile(root: &mut Value, profile_name: &str) -> bool {
    if profile_name == DEFAULT_PROFILE_NAME {
        return false;
    }
    root.as_object_mut().is_some_and(|object| object.remove(profile_name).is_some())
}

/// Duplicates a profile and persists the resulting JSON using C++-compatible backup rules.
#[must_use]
pub fn duplicate_profile_in_file(path: &Path, source_name: &str, new_name: &str) -> bool {
    let Some((mut root, _)) = read_json_file_with_backup(path) else {
        return false;
    };
    if !duplicate_profile(&mut root, source_name, new_name) {
        return false;
    }
    write_json_file(path, &root, false)
}

/// Renames a profile and persists the resulting JSON using C++-compatible backup rules.
#[must_use]
pub fn rename_profile_in_file(path: &Path, old_name: &str, new_name: &str) -> bool {
    if old_name == new_name {
        return true;
    }
    let Some((mut root, _)) = read_json_file_with_backup(path) else {
        return false;
    };
    if !rename_profile(&mut root, old_name, new_name) {
        return false;
    }
    write_json_file(path, &root, false)
}

/// Deletes a profile and persists the resulting JSON using C++-compatible backup rules.
#[must_use]
pub fn delete_profile_from_file(path: &Path, profile_name: &str) -> bool {
    if profile_name == DEFAULT_PROFILE_NAME {
        return false;
    }
    let Some((mut root, _)) = read_json_file_with_backup(path) else {
        return false;
    };
    if !delete_profile(&mut root, profile_name) {
        return false;
    }
    write_json_file(path, &root, false)
}

/// Discovers the first readable settings source using C++ priority without migrating or repairing it.
///
/// This is intended for read-only UI/catalog composition while full live profile serialization is not
/// yet attached. `SMCSettings.json` globally outranks `RMCSettings.json`; within one filename the
/// canonical, executable, executable-parent, then current-directory order is preserved.
#[must_use]
pub fn discover_settings_file_path_read_only_from_directories(
    canonical_directory: &Path,
    executable_directory: &Path,
    current_directory: &Path,
) -> Option<PathBuf> {
    let mut directories = Vec::new();
    append_unique_directory(&mut directories, canonical_directory);
    append_unique_directory(&mut directories, executable_directory);
    if !path_is_empty(executable_directory)
        && let Some(parent) = executable_directory.parent()
    {
        append_unique_directory(&mut directories, parent);
    }
    append_unique_directory(&mut directories, current_directory);

    for filename in [CURRENT_SETTINGS_FILENAME, LEGACY_SETTINGS_FILENAME] {
        for directory in &directories {
            let candidate = directory.join(filename);
            if has_settings_source(&candidate) && profile_catalog_from_file(&candidate).is_some() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Resolves and, when necessary, migrates settings exactly in the C++ discovery order.
///
/// Valid `SMCSettings.json` sources globally outrank `RMCSettings.json` sources. Within one
/// filename, canonical, executable, executable-parent, then current-directory locations are tried.
#[must_use]
pub fn resolve_settings_file_path_from_directories(
    canonical_directory: &Path,
    executable_directory: &Path,
    current_directory: &Path,
) -> PathBuf {
    let canonical_settings = if path_is_empty(canonical_directory) {
        PathBuf::new()
    } else {
        canonical_directory.join(CURRENT_SETTINGS_FILENAME)
    };

    let mut directories = Vec::new();
    append_unique_directory(&mut directories, canonical_directory);
    append_unique_directory(&mut directories, executable_directory);
    if !path_is_empty(executable_directory)
        && let Some(parent) = executable_directory.parent()
    {
        append_unique_directory(&mut directories, parent);
    }
    append_unique_directory(&mut directories, current_directory);

    let mut selected: Option<(PathBuf, Value, bool)> = None;
    for filename in [CURRENT_SETTINGS_FILENAME, LEGACY_SETTINGS_FILENAME] {
        for directory in &directories {
            let candidate = directory.join(filename);
            if !has_settings_source(&candidate) {
                continue;
            }
            if let Some((data, loaded_from_backup)) = read_json_file_with_backup(&candidate) {
                selected = Some((candidate, data, loaded_from_backup));
                break;
            }
        }
        if selected.is_some() {
            break;
        }
    }

    if let Some((selected_path, data, loaded_from_backup)) = selected {
        let canonical_primary_is_already_valid = !canonical_settings.as_os_str().is_empty()
            && selected_path == canonical_settings
            && !loaded_from_backup;
        if canonical_primary_is_already_valid {
            return canonical_settings;
        }

        if !canonical_settings.as_os_str().is_empty()
            && ensure_parent_directory(&canonical_settings)
            && write_json_file(&canonical_settings, &data, true)
        {
            return canonical_settings;
        }
        return selected_path;
    }

    if !canonical_settings.as_os_str().is_empty() && ensure_parent_directory(&canonical_settings) {
        return canonical_settings;
    }
    if !path_is_empty(current_directory) {
        return current_directory.join(CURRENT_SETTINGS_FILENAME);
    }
    if !path_is_empty(executable_directory) {
        return executable_directory.join(CURRENT_SETTINGS_FILENAME);
    }
    PathBuf::from(CURRENT_SETTINGS_FILENAME)
}

fn path_is_empty(path: &Path) -> bool {
    path.as_os_str().is_empty()
}

fn append_unique_directory(directories: &mut Vec<PathBuf>, directory: &Path) {
    if path_is_empty(directory) {
        return;
    }
    let path = directory.to_path_buf();
    if !directories.contains(&path) {
        directories.push(path);
    }
}

fn path_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value: OsString = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn settings_backup_path(path: &Path) -> PathBuf {
    path_with_suffix(path, ".bak")
}

fn has_settings_source(path: &Path) -> bool {
    path.exists() || settings_backup_path(path).exists()
}

fn read_json_file_direct(path: &Path) -> Result<Value, ProfileError> {
    let file =
        File::open(path).map_err(|source| ProfileError::Io { path: path.to_path_buf(), source })?;
    serde_json::from_reader(file)
        .map_err(|source| ProfileError::Json { path: path.to_path_buf(), source })
}

fn read_json_file_with_backup(path: &Path) -> Option<(Value, bool)> {
    if let Ok(data) = read_json_file_direct(path) {
        return Some((data, false));
    }
    read_json_file_direct(&settings_backup_path(path)).ok().map(|data| (data, true))
}

fn ensure_parent_directory(path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return true;
    };
    if parent.as_os_str().is_empty() {
        return true;
    }
    fs::create_dir_all(parent).is_ok()
}

fn copy_settings_file(source: &Path, destination: &Path) -> bool {
    if !ensure_parent_directory(destination) || fs::copy(source, destination).is_err() {
        return false;
    }
    adjust_settings_permissions(destination)
}

#[cfg(unix)]
fn adjust_settings_permissions(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).is_ok()
}

#[cfg(not(unix))]
const fn adjust_settings_permissions(_path: &Path) -> bool {
    true
}

fn write_pretty_json(path: &Path, data: &Value) -> bool {
    let Ok(file) = OpenOptions::new().write(true).create_new(true).open(path) else {
        return false;
    };
    let mut writer = BufWriter::new(file);
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut writer, formatter);
    if data.serialize(&mut serializer).is_err() || writer.flush().is_err() {
        let _ = fs::remove_file(path);
        return false;
    }
    let Ok(file) = writer.into_inner() else {
        let _ = fs::remove_file(path);
        return false;
    };
    if !adjust_settings_permissions(path) || file.sync_all().is_err() {
        let _ = fs::remove_file(path);
        return false;
    }
    true
}

fn replace_settings_file(source: &Path, destination: &Path) -> bool {
    #[cfg(windows)]
    {
        if destination.exists() && fs::remove_file(destination).is_err() {
            return false;
        }
    }
    fs::rename(source, destination).is_ok()
}

#[cfg(unix)]
fn flush_directory(path: &Path) {
    if let Ok(directory) = File::open(path) {
        let _ = directory.sync_all();
    }
}

// Windows does not provide the directory fsync semantics used by Unix.
#[cfg(not(unix))]
const fn flush_directory(_path: &Path) {}

fn next_temp_path(path: &Path) -> PathBuf {
    let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    path_with_suffix(path, &format!(".tmp-{}-{counter}", std::process::id()))
}

fn next_corrupt_path(path: &Path) -> PathBuf {
    let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    path_with_suffix(path, &format!(".corrupt-{}-{counter}", std::process::id()))
}

fn write_json_file(path: &Path, data: &Value, allow_replace_unreadable_primary: bool) -> bool {
    if !ensure_parent_directory(path) {
        return false;
    }

    let temp_path = next_temp_path(path);
    if !write_pretty_json(&temp_path, data) {
        return false;
    }

    if path.exists() {
        if read_json_file_direct(path).is_ok() {
            if !copy_settings_file(path, &settings_backup_path(path)) {
                let _ = fs::remove_file(&temp_path);
                return false;
            }
        } else if read_json_file_direct(&settings_backup_path(path)).is_err() {
            if !allow_replace_unreadable_primary {
                let _ = fs::remove_file(&temp_path);
                return false;
            }
            if !copy_settings_file(path, &next_corrupt_path(path)) {
                let _ = fs::remove_file(&temp_path);
                return false;
            }
        }
    }

    if !replace_settings_file(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return false;
    }
    let _ = adjust_settings_permissions(path);
    if let Some(parent) = path.parent() {
        flush_directory(parent);
    }
    true
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic)]

    use super::{
        CURRENT_SETTINGS_FILENAME, ContainerFormat, DEFAULT_PROFILE_NAME, LAST_ACTIVE_PROFILE_KEY,
        LEGACY_FORMAT_KEYS, LEGACY_SETTINGS_FILENAME, METADATA_KEY, StartupProfileSource,
        delete_profile_from_file, detect_format,
        discover_settings_file_path_read_only_from_directories, duplicate_profile_in_file,
        find_best_profile, generate_new_profile_name, generate_unique_profile_name, metadata_mut,
        metadata_snapshot_from_file, overlay_metadata_in_file, profile_catalog_from_file,
        profile_names, profile_payload_from_file, read_json_file_with_backup,
        rename_profile_in_file, replace_legacy_flat_with_profile_in_file,
        resolve_settings_file_path_from_directories, settings_backup_path, settings_source_exists,
        startup_profile_source_from_file, upsert_profile_payload_in_file,
        upsert_profile_payload_with_metadata_in_file,
    };
    use serde::Deserialize;
    use serde_json::{Map, Value, json};
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_DIRECTORY_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[derive(Debug, Deserialize)]
    struct Fixture {
        legacy_keys: Vec<String>,
        format_cases: Vec<FormatCase>,
        best_profile_cases: Vec<BestProfileCase>,
        profile_name_cases: Vec<ProfileNameCase>,
        mutation_cases: Vec<MutationCase>,
        recovery_cases: Vec<RecoveryCase>,
        resolution_cases: Vec<ResolutionCase>,
    }

    #[derive(Debug, Deserialize)]
    struct FormatCase {
        name: String,
        serialized: String,
        format: String,
    }

    #[derive(Debug, Deserialize)]
    struct BestProfileCase {
        name: String,
        serialized: String,
        requested: String,
        best: String,
    }

    #[derive(Debug, Deserialize)]
    struct ProfileNameCase {
        name: String,
        document: Value,
        names: Vec<String>,
    }

    #[derive(Debug, Deserialize)]
    struct MutationCase {
        name: String,
        operation: String,
        arguments: Vec<String>,
        document: Value,
        success: bool,
        result: Value,
        backup_exists: bool,
    }

    #[derive(Debug, Deserialize)]
    struct RecoveryCase {
        name: String,
        initial_primary: Option<Value>,
        initial_backup: Option<Value>,
        operation: Vec<String>,
        success: Option<bool>,
        primary: Option<Value>,
        backup: Option<Value>,
        first_success: Option<bool>,
        first_primary: Option<Value>,
        first_backup: Option<Value>,
        corrupt_primary_before_second: Option<String>,
        second_operation: Option<Vec<String>>,
        second_success: Option<bool>,
        second_primary: Option<Value>,
        second_backup: Option<Value>,
    }

    #[derive(Debug, Deserialize)]
    struct ResolutionCase {
        name: String,
        sources: BTreeMap<String, Value>,
        resolved: String,
        canonical_primary: Value,
        source_state_after: BTreeMap<String, Value>,
        corrupt_primary_count: usize,
    }

    struct TestDirectory {
        path: PathBuf,
    }

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let counter = TEST_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("smu-profile-rust-{label}-{}-{counter}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("temporary test directory should be creatable");
            Self { path }
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn fixture() -> Fixture {
        serde_json::from_str(include_str!("../../../../compat/profiles/cases.json"))
            .expect("C++ generated profile compatibility fixture should parse")
    }

    fn format_name(format: ContainerFormat) -> &'static str {
        match format {
            ContainerFormat::Profile => "profile",
            ContainerFormat::LegacyFlat => "legacy-flat",
            ContainerFormat::Unknown => "unknown",
        }
    }

    fn write_source(path: &Path, value: &Value) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture parent should be creatable");
        }
        if let Some(raw) = value.as_str() {
            fs::write(path, raw).expect("raw fixture should be writable");
        } else {
            fs::write(path, serde_json::to_vec(value).expect("fixture JSON should serialize"))
                .expect("JSON fixture should be writable");
        }
    }

    fn inspect_source(path: &Path) -> Value {
        if !path.exists() {
            return json!({"exists": false});
        }
        let bytes = fs::read(path).expect("fixture source should be readable");
        serde_json::from_slice::<Value>(&bytes).map_or_else(
            |_| {
                json!({
                    "exists": true,
                    "raw": String::from_utf8(bytes).expect("fixture raw data should be UTF-8")
                })
            },
            |value| json!({"exists": true, "json": value}),
        )
    }

    fn source_path(layout: &BTreeMap<&str, PathBuf>, key: &str) -> PathBuf {
        let (location, filename) =
            key.split_once('/').expect("fixture source key should contain slash");
        layout.get(location).expect("fixture location should exist").join(filename)
    }

    fn slash_relative(path: &Path, root: &Path) -> String {
        path.strip_prefix(root)
            .expect("resolved path should remain inside fixture root")
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    }

    fn execute_mutation(path: &Path, operation: &[String]) -> bool {
        match operation {
            [kind, source, target] if kind == "duplicate" => {
                duplicate_profile_in_file(path, source, target)
            }
            [kind, source, target] if kind == "rename" => {
                rename_profile_in_file(path, source, target)
            }
            [kind, profile] if kind == "delete" => delete_profile_from_file(path, profile),
            _ => false,
        }
    }

    #[test]
    fn legacy_replacement_writes_only_profile_and_metadata_and_keeps_backup_rules() {
        let temporary = TestDirectory::new("legacy-replacement");
        let path = temporary.path.join("settings.json");
        write_source(
            &path,
            &json!({
                "vk_f5": 0x71,
                "future_legacy_junk": 123,
                "imported_scripts": [{"path":"legacy.lua"}]
            }),
        );
        let legacy = match startup_profile_source_from_file(&path) {
            StartupProfileSource::LegacyFlat { payload, .. } => payload,
            other => panic!("expected legacy payload, got {other:?}"),
        };
        let materialized = legacy.materialized_serializer_payload();
        assert!(replace_legacy_flat_with_profile_in_file(
            &path,
            "Profile 1",
            &materialized.raw_snapshot(),
        ));
        let written = read_json_file_with_backup(&path)
            .map(|(value, _)| value)
            .expect("converted profile should read");
        assert_eq!(written[METADATA_KEY][LAST_ACTIVE_PROFILE_KEY], json!("Profile 1"));
        assert!(written.get("vk_f5").is_none());
        assert_eq!(written["Profile 1"]["vk_f5"], json!(0x71));
        assert_eq!(written["Profile 1"]["imported_scripts"], json!([{"path":"legacy.lua"}]));
        assert!(written["Profile 1"].get("future_legacy_junk").is_none());
        assert!(settings_backup_path(&path).exists());
    }

    #[test]
    fn startup_source_distinguishes_missing_unreadable_legacy_structured_and_scalar() {
        let temporary = TestDirectory::new("startup-source");
        let path = temporary.path.join("settings.json");
        assert!(!settings_source_exists(&path));
        assert_eq!(startup_profile_source_from_file(&path), StartupProfileSource::Missing);

        fs::write(&path, b"{ broken").expect("corrupt fixture should write");
        assert!(settings_source_exists(&path));
        assert_eq!(startup_profile_source_from_file(&path), StartupProfileSource::Unreadable);

        write_source(&path, &json!({"vk_f5": 0x72, "future": 1}));
        match startup_profile_source_from_file(&path) {
            StartupProfileSource::LegacyFlat { payload, loaded_from_backup } => {
                assert!(!loaded_from_backup);
                assert_eq!(payload.raw_value("vk_f5"), Some(&json!(0x72)));
            }
            other => panic!("expected legacy flat, got {other:?}"),
        }

        write_source(
            &path,
            &json!({
                "_metadata": {"last_active_profile": "Profile 2"},
                "Profile 2": {"vk_f5": 0x73, "future": {"keep": true}}
            }),
        );
        match startup_profile_source_from_file(&path) {
            StartupProfileSource::Structured {
                preferred_name,
                preferred_payload,
                loaded_from_backup,
            } => {
                assert_eq!(preferred_name.as_deref(), Some("Profile 2"));
                assert!(!loaded_from_backup);
                assert_eq!(
                    preferred_payload.as_ref().and_then(|payload| payload.raw_value("future")),
                    Some(&json!({"keep": true}))
                );
            }
            other => panic!("expected structured source, got {other:?}"),
        }

        write_source(&path, &json!([1, 2, 3]));
        assert_eq!(
            startup_profile_source_from_file(&path),
            StartupProfileSource::Unsupported { loaded_from_backup: false }
        );
    }

    #[test]
    fn startup_source_uses_valid_backup_when_primary_is_corrupt() {
        let temporary = TestDirectory::new("startup-backup");
        let path = temporary.path.join("settings.json");
        let backup = settings_backup_path(&path);
        fs::write(&path, b"broken").expect("corrupt primary should write");
        write_source(
            &backup,
            &json!({
                "_metadata": {"last_active_profile": "Profile 1"},
                "Profile 1": {"vk_f5": 0x74}
            }),
        );
        match startup_profile_source_from_file(&path) {
            StartupProfileSource::Structured { loaded_from_backup, preferred_name, .. } => {
                assert!(loaded_from_backup);
                assert_eq!(preferred_name.as_deref(), Some("Profile 1"));
            }
            other => panic!("expected structured backup source, got {other:?}"),
        }
    }

    #[test]
    fn read_only_discovery_preserves_cpp_source_priority_without_migration() {
        let temporary = TestDirectory::new("readonly-discovery");
        let canonical = temporary.path.join("canonical");
        let executable = temporary.path.join("app").join("bin");
        let current = temporary.path.join("cwd");
        fs::create_dir_all(&canonical).expect("canonical dir");
        fs::create_dir_all(&executable).expect("executable dir");
        fs::create_dir_all(&current).expect("current dir");

        let legacy = canonical.join(LEGACY_SETTINGS_FILENAME);
        write_source(&legacy, &json!({"vk_f5": 1}));
        let current_primary = current.join(CURRENT_SETTINGS_FILENAME);
        write_source(&current_primary, &json!({"_metadata": {}, "Profile 9": {"vk_f5": 9}}));

        let resolved = discover_settings_file_path_read_only_from_directories(
            &canonical,
            &executable,
            &current,
        );
        assert_eq!(resolved.as_deref(), Some(current_primary.as_path()));
        assert!(legacy.exists());
        assert!(!canonical.join(CURRENT_SETTINGS_FILENAME).exists());
    }

    #[test]
    fn named_profile_payload_read_preserves_unknown_and_script_fields() {
        let temporary = TestDirectory::new("named-payload-read");
        let path = temporary.path.join("settings.json");
        let mut profile = crate::payload::ProfilePayloadState::default().snapshot();
        let object = profile.as_object_mut().expect("default profile object");
        object.insert("future_key".to_owned(), json!({"keep": true}));
        object.insert("imported_scripts".to_owned(), json!([{"path": "x.lua"}]));
        write_source(&path, &json!({"_metadata": {}, "Profile A": profile}));
        let preserved = profile_payload_from_file(&path, "Profile A")
            .expect("named preserved profile should load");
        assert_eq!(preserved.raw_value("future_key"), Some(&json!({"keep": true})));
        assert_eq!(preserved.raw_value("imported_scripts"), Some(&json!([{"path": "x.lua"}])));
    }

    #[test]
    fn profile_payload_upsert_refuses_unreadable_existing_source() {
        let temporary = TestDirectory::new("payload-upsert-unreadable");
        let path = temporary.path.join("settings.json");
        fs::write(&path, b"{ broken").expect("corrupt source should write");
        let before = fs::read(&path).expect("corrupt source should be readable as bytes");
        let payload = crate::payload::ProfilePayloadState::default().snapshot();
        assert!(!upsert_profile_payload_in_file(&path, "Profile 1", &payload, true));
        assert_eq!(
            fs::read(&path).expect("corrupt source should remain readable as bytes"),
            before
        );
    }

    #[test]
    fn profile_payload_upsert_creates_fresh_container_when_missing() {
        let temporary = TestDirectory::new("payload-upsert-create");
        let path = temporary.path.join("nested").join(CURRENT_SETTINGS_FILENAME);
        let payload = crate::payload::ProfilePayloadState::default().snapshot();
        assert!(upsert_profile_payload_in_file(&path, "Profile 1", &payload, true));
        let (written, _) = read_json_file_with_backup(&path).expect("fresh container should load");
        assert!(written["Profile 1"].is_object());
        assert_eq!(written[METADATA_KEY][LAST_ACTIVE_PROFILE_KEY], json!("Profile 1"));
    }

    #[test]
    fn profile_payload_upsert_preserves_siblings_unknown_root_fields_and_marks_active() {
        let temporary = TestDirectory::new("payload-upsert");
        let path = temporary.path.join("settings.json");
        let original = json!({
            "_metadata": {"last_active_profile": "Other", "future_meta": 7},
            "root_future": {"x": 1},
            "Other": {"sentinel": true}
        });
        write_source(&path, &original);
        let mut payload = crate::payload::ProfilePayloadState::default().snapshot();
        payload
            .as_object_mut()
            .expect("default payload object")
            .insert("future_profile_key".to_owned(), json!([1, 2, 3]));
        assert!(upsert_profile_payload_in_file(&path, "New Profile", &payload, true));
        let (written, _) = read_json_file_with_backup(&path).expect("written root should load");
        assert_eq!(written["root_future"], json!({"x": 1}));
        assert_eq!(written["Other"], json!({"sentinel": true}));
        assert_eq!(written["_metadata"]["future_meta"], json!(7));
        assert_eq!(written["_metadata"][LAST_ACTIVE_PROFILE_KEY], json!("New Profile"));
        assert_eq!(written["New Profile"]["future_profile_key"], json!([1, 2, 3]));
    }

    #[test]
    fn metadata_snapshot_and_overlay_preserve_unrelated_root_profile_and_metadata_fields() {
        let temporary = TestDirectory::new("metadata-overlay");
        let path = temporary.path.join("settings.json");
        write_source(
            &path,
            &json!({
                "_metadata": {"future_meta": {"keep": true}, "current_theme_index": 1},
                "root_future": [1,2,3],
                "Profile 1": {"vk_f5": 0x74}
            }),
        );
        let before = metadata_snapshot_from_file(&path).expect("metadata should load");
        assert_eq!(before.get("future_meta"), Some(&json!({"keep": true})));

        let overlay = Map::from_iter([
            ("current_theme_index".to_owned(), json!(2)),
            ("show_theme_menu".to_owned(), json!(true)),
        ]);
        assert!(overlay_metadata_in_file(&path, &overlay));
        let (written, _) = read_json_file_with_backup(&path).expect("metadata overlay should load");
        assert_eq!(written["root_future"], json!([1, 2, 3]));
        assert_eq!(written["Profile 1"]["vk_f5"], json!(0x74));
        assert_eq!(written["_metadata"]["future_meta"], json!({"keep": true}));
        assert_eq!(written["_metadata"]["current_theme_index"], json!(2));
        assert_eq!(written["_metadata"]["show_theme_menu"], json!(true));
    }

    #[test]
    fn profile_and_metadata_overlay_are_written_in_one_preservation_safe_mutation() {
        let temporary = TestDirectory::new("payload-metadata-overlay");
        let path = temporary.path.join("settings.json");
        write_source(
            &path,
            &json!({
                "_metadata": {"future_meta": 7},
                "Other": {"sentinel": true}
            }),
        );
        let payload = crate::payload::ProfilePayloadState::default().snapshot();
        let overlay = Map::from_iter([
            ("current_theme_index".to_owned(), json!(2)),
            (LAST_ACTIVE_PROFILE_KEY.to_owned(), json!("Stale Profile")),
        ]);
        assert!(upsert_profile_payload_with_metadata_in_file(
            &path,
            "Profile 1",
            &payload,
            true,
            &overlay,
        ));
        let (written, _) = read_json_file_with_backup(&path).expect("combined write should load");
        assert_eq!(written["Other"], json!({"sentinel": true}));
        assert_eq!(written["_metadata"]["future_meta"], json!(7));
        assert_eq!(written["_metadata"]["current_theme_index"], json!(2));
        assert_eq!(written["_metadata"][LAST_ACTIVE_PROFILE_KEY], json!("Profile 1"));
    }

    #[test]
    fn profile_catalog_reads_primary_without_mutation() {
        let temporary = TestDirectory::new("catalog-primary");
        let path = temporary.path.join("settings.json");
        let document = json!({
            "_metadata": {"last_active_profile": "Profile 2"},
            "(default)": {"x": 0},
            "Profile 1": {"x": 1},
            "Profile 2": {"x": 2}
        });
        write_source(&path, &document);
        let before = fs::read(&path).expect("catalog primary should be readable");
        let catalog = profile_catalog_from_file(&path).expect("catalog should load");
        assert_eq!(catalog.names, ["(default)", "Profile 1", "Profile 2"]);
        assert_eq!(catalog.preferred_profile.as_deref(), Some("Profile 2"));
        assert!(!catalog.loaded_from_backup);
        assert_eq!(fs::read(&path).expect("catalog primary should remain readable"), before);
    }

    #[test]
    fn profile_catalog_falls_back_to_backup_without_repairing_primary() {
        let temporary = TestDirectory::new("catalog-backup");
        let path = temporary.path.join("settings.json");
        let backup = PathBuf::from(format!("{}.bak", path.display()));
        fs::write(&path, b"{ broken").expect("corrupt primary should be writable");
        write_source(
            &backup,
            &json!({
                "_metadata": {"last_active_profile": "Profile 3"},
                "Profile 3": {"x": 3}
            }),
        );
        let primary_before = fs::read(&path).expect("primary should be readable as bytes");
        let catalog = profile_catalog_from_file(&path).expect("backup catalog should load");
        assert_eq!(catalog.names, ["Profile 3"]);
        assert_eq!(catalog.preferred_profile.as_deref(), Some("Profile 3"));
        assert!(catalog.loaded_from_backup);
        assert_eq!(fs::read(&path).expect("primary should not be repaired"), primary_before);
    }

    #[test]
    fn new_profile_name_matches_cpp_first_gap_policy() {
        let names = vec!["Profile 1".to_owned(), "Profile 3".to_owned(), "Other".to_owned()];
        assert_eq!(generate_new_profile_name(&names), "Profile 2");
    }

    #[test]
    fn cpp_generated_format_detection_cases_match() {
        let fixture = fixture();
        let generated_keys: Vec<&str> = fixture.legacy_keys.iter().map(String::as_str).collect();
        assert_eq!(LEGACY_FORMAT_KEYS, generated_keys, "generated legacy key set must match C++");
        for case in fixture.format_cases {
            let observed = serde_json::from_str::<Value>(&case.serialized)
                .map_or("invalid-json", |root| format_name(detect_format(&root)));
            assert_eq!(observed, case.format, "case {}", case.name);
        }
    }

    #[test]
    fn cpp_generated_best_profile_cases_match() {
        let fixture = fixture();
        for case in fixture.best_profile_cases {
            let root: Value = serde_json::from_str(&case.serialized)
                .expect("best-profile fixture JSON should parse");
            let observed = find_best_profile(&root, &case.requested).unwrap_or_default();
            assert_eq!(observed, case.best, "case {}", case.name);
        }
    }

    #[test]
    fn cpp_generated_profile_name_cases_match() {
        let fixture = fixture();
        for case in fixture.profile_name_cases {
            assert_eq!(profile_names(&case.document), case.names, "case {}", case.name);
        }
    }

    #[test]
    fn cpp_generated_profile_mutation_cases_match() {
        let fixture = fixture();
        for case in fixture.mutation_cases {
            let temporary = TestDirectory::new(&case.name);
            let path = temporary.path.join("settings.json");
            fs::write(
                &path,
                serde_json::to_vec(&case.document).expect("fixture JSON should serialize"),
            )
            .expect("mutation fixture should be writable");

            let mut operation = Vec::with_capacity(case.arguments.len() + 1);
            operation.push(case.operation.clone());
            operation.extend(case.arguments.clone());
            let success = execute_mutation(&path, &operation);
            assert_eq!(success, case.success, "case {}", case.name);
            let observed: Value = serde_json::from_slice(
                &fs::read(&path).expect("mutation result should remain readable"),
            )
            .expect("mutation result should be JSON");
            assert_eq!(observed, case.result, "case {}", case.name);
            assert_eq!(
                PathBuf::from(format!("{}.bak", path.display())).exists(),
                case.backup_exists,
                "case {}",
                case.name
            );
        }
    }

    #[test]
    fn cpp_generated_recovery_cases_match() {
        let fixture = fixture();
        for case in fixture.recovery_cases {
            let temporary = TestDirectory::new(&case.name);
            let path = temporary.path.join("settings.json");
            let backup = PathBuf::from(format!("{}.bak", path.display()));

            if let Some(primary) = &case.initial_primary {
                write_source(&path, primary);
            }
            if let Some(initial_backup) = &case.initial_backup {
                write_source(&backup, initial_backup);
            }

            if let Some(expected_first_success) = case.first_success {
                let first_success = execute_mutation(&path, &case.operation);
                assert_eq!(
                    first_success, expected_first_success,
                    "case {} first operation",
                    case.name
                );
                assert_eq!(
                    inspect_source(&path),
                    case.first_primary.expect("sequence case should provide first primary"),
                    "case {} first primary",
                    case.name
                );
                assert_eq!(
                    inspect_source(&backup),
                    case.first_backup.expect("sequence case should provide first backup"),
                    "case {} first backup",
                    case.name
                );
                fs::write(
                    &path,
                    case.corrupt_primary_before_second
                        .as_deref()
                        .expect("sequence case should provide corrupt primary"),
                )
                .expect("sequence primary should be corruptible");
                let second_success = execute_mutation(
                    &path,
                    case.second_operation
                        .as_deref()
                        .expect("sequence case should provide second operation"),
                );
                assert_eq!(
                    second_success,
                    case.second_success.expect("sequence case should provide second success"),
                    "case {} second operation",
                    case.name
                );
                assert_eq!(
                    inspect_source(&path),
                    case.second_primary.expect("sequence case should provide second primary"),
                    "case {} second primary",
                    case.name
                );
                assert_eq!(
                    inspect_source(&backup),
                    case.second_backup.expect("sequence case should provide second backup"),
                    "case {} second backup",
                    case.name
                );
            } else {
                let success = execute_mutation(&path, &case.operation);
                assert_eq!(
                    success,
                    case.success.expect("single recovery case should provide success"),
                    "case {}",
                    case.name
                );
                assert_eq!(
                    inspect_source(&path),
                    case.primary.expect("single recovery case should provide primary"),
                    "case {} primary",
                    case.name
                );
                assert_eq!(
                    inspect_source(&backup),
                    case.backup.expect("single recovery case should provide backup"),
                    "case {} backup",
                    case.name
                );
            }
        }
    }

    #[test]
    fn cpp_generated_resolution_cases_match() {
        let fixture = fixture();
        for case in fixture.resolution_cases {
            let temporary = TestDirectory::new(&case.name);
            let root = &temporary.path;
            let layout = BTreeMap::from([
                ("canonical", root.join("config")),
                ("executable", root.join("app/bin")),
                ("parent", root.join("app")),
                ("current", root.join("cwd")),
            ]);
            for directory in layout.values() {
                fs::create_dir_all(directory).expect("resolution directory should be creatable");
            }
            for (key, value) in &case.sources {
                write_source(&source_path(&layout, key), value);
            }

            let resolved = resolve_settings_file_path_from_directories(
                layout.get("canonical").expect("canonical path should exist"),
                layout.get("executable").expect("executable path should exist"),
                layout.get("current").expect("current path should exist"),
            );
            assert_eq!(slash_relative(&resolved, root), case.resolved, "case {}", case.name);

            let canonical = layout
                .get("canonical")
                .expect("canonical path should exist")
                .join(CURRENT_SETTINGS_FILENAME);
            assert_eq!(inspect_source(&canonical), case.canonical_primary, "case {}", case.name);
            for (key, expected) in &case.source_state_after {
                assert_eq!(
                    inspect_source(&source_path(&layout, key)),
                    *expected,
                    "case {} source {}",
                    case.name,
                    key
                );
            }
            let corrupt_count =
                fs::read_dir(layout.get("canonical").expect("canonical path should exist"))
                    .expect("canonical directory should be readable")
                    .filter_map(Result::ok)
                    .filter(|entry| {
                        entry.file_name().to_string_lossy().starts_with("SMCSettings.json.corrupt-")
                    })
                    .count();
            assert_eq!(corrupt_count, case.corrupt_primary_count, "case {}", case.name);
        }
    }

    #[test]
    fn metadata_creation_preserves_profiles() {
        let mut root = json!({"Profile 1": {"macrotoggled": true}});
        let metadata = metadata_mut(&mut root).expect("test fixture is an object");
        metadata.insert("shortdescriptions".to_owned(), json!(true));
        assert_eq!(root["Profile 1"]["macrotoggled"], true);
        assert_eq!(root["_metadata"]["shortdescriptions"], true);
    }

    #[test]
    fn unique_copy_name_matches_cpp_sequence() {
        let existing = vec!["Profile 1 (Copy)".to_owned(), "Profile 1 (Copy 2)".to_owned()];
        assert_eq!(generate_unique_profile_name("Profile 1", &existing), "Profile 1 (Copy 3)");
        assert_eq!(DEFAULT_PROFILE_NAME, "(default)");
    }
}

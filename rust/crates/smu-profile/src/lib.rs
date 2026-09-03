//! Compatibility-first handling of the existing SMU profile container format.

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
fn adjust_settings_permissions(_path: &Path) -> bool {
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

fn flush_directory(path: &Path) {
    #[cfg(unix)]
    if let Ok(directory) = File::open(path) {
        let _ = directory.sync_all();
    }
}

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
    #![allow(clippy::expect_used)]

    use super::{
        CURRENT_SETTINGS_FILENAME, ContainerFormat, DEFAULT_PROFILE_NAME, LEGACY_FORMAT_KEYS,
        delete_profile_from_file, detect_format, duplicate_profile_in_file, find_best_profile,
        generate_unique_profile_name, metadata_mut, profile_names, rename_profile_in_file,
        resolve_settings_file_path_from_directories,
    };
    use serde::Deserialize;
    use serde_json::{Value, json};
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

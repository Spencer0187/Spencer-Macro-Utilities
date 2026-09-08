use crate::profile_browser::{ProfileAction, ProfileBrowserState};
use smu_profile::payload::PreservedProfilePayload;
use smu_profile::{
    StartupProfileSource, delete_profile_from_file, duplicate_profile_in_file,
    generate_unique_profile_name, metadata_snapshot_from_file, profile_catalog_from_file,
    profile_payload_from_file, rename_profile_in_file, replace_legacy_flat_with_profile_in_file,
    resolve_settings_file_path_from_directories, startup_profile_source_from_file,
    upsert_profile_payload_in_file, upsert_profile_payload_with_metadata_in_file,
};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileLoadFailure {
    Unreadable,
    Missing,
    Invalid,
}

#[derive(Debug)]
pub struct ProfileBrowserService {
    path: Option<PathBuf>,
    current_name: Option<String>,
    current_payload: Option<PreservedProfilePayload>,
}

impl ProfileBrowserService {
    #[must_use]
    pub fn new(parity_isolated: bool) -> Self {
        let path = (!parity_isolated).then(resolve_settings_path);
        Self { path, current_name: None, current_payload: None }
    }

    #[cfg(test)]
    pub(crate) const fn from_path_for_testing(path: PathBuf) -> Self {
        Self { path: Some(path), current_name: None, current_payload: None }
    }

    #[must_use]
    pub fn startup_source(&self) -> StartupProfileSource {
        self.path.as_deref().map_or(StartupProfileSource::Missing, startup_profile_source_from_file)
    }

    #[must_use]
    pub fn metadata_snapshot(&self) -> Option<serde_json::Map<String, serde_json::Value>> {
        self.path.as_deref().and_then(metadata_snapshot_from_file)
    }

    #[must_use]
    pub fn save_default_payload(&self, payload: &PreservedProfilePayload) -> bool {
        let Some(path) = self.path.as_deref() else {
            return false;
        };
        upsert_profile_payload_in_file(
            path,
            smu_profile::DEFAULT_PROFILE_NAME,
            &payload.raw_snapshot(),
            false,
        )
    }

    #[must_use]
    pub fn replace_legacy_with_profile(
        &self,
        name: &str,
        payload: &PreservedProfilePayload,
    ) -> bool {
        let Some(path) = self.path.as_deref() else {
            return false;
        };
        replace_legacy_flat_with_profile_in_file(path, name, &payload.raw_snapshot())
    }

    #[must_use]
    pub fn load_payload(&self, name: &str) -> Option<PreservedProfilePayload> {
        self.path.as_deref().and_then(|path| profile_payload_from_file(path, name))
    }

    pub fn load_payload_result(
        &self,
        name: &str,
    ) -> Result<PreservedProfilePayload, ProfileLoadFailure> {
        let Some(path) = self.path.as_deref() else {
            return Err(ProfileLoadFailure::Unreadable);
        };
        let Some(catalog) = profile_catalog_from_file(path) else {
            return Err(ProfileLoadFailure::Unreadable);
        };
        if !catalog.names.iter().any(|candidate| candidate == name) {
            return Err(ProfileLoadFailure::Missing);
        }
        profile_payload_from_file(path, name).ok_or(ProfileLoadFailure::Invalid)
    }

    #[must_use]
    pub fn path_display(&self) -> String {
        self.path
            .as_deref()
            .map_or_else(|| "settings file".to_owned(), |path| path.display().to_string())
    }

    #[must_use]
    pub fn save_payload(&self, name: &str, payload: &PreservedProfilePayload) -> bool {
        let Some(path) = self.path.as_deref() else {
            return false;
        };
        upsert_profile_payload_in_file(path, name, &payload.raw_snapshot(), true)
    }

    #[must_use]
    pub fn save_payload_with_metadata(
        &self,
        name: &str,
        payload: &PreservedProfilePayload,
        metadata: &serde_json::Map<String, serde_json::Value>,
    ) -> bool {
        let Some(path) = self.path.as_deref() else {
            return false;
        };
        upsert_profile_payload_with_metadata_in_file(
            path,
            name,
            &payload.raw_snapshot(),
            true,
            metadata,
        )
    }

    pub fn mark_loaded(&mut self, name: String, payload: PreservedProfilePayload) {
        self.current_name = Some(name);
        self.current_payload = Some(payload);
    }

    pub fn mark_current_name_only(&mut self, name: String) {
        self.current_name = Some(name);
        self.current_payload = None;
    }

    pub fn clear_loaded(&mut self) {
        self.current_name = None;
        self.current_payload = None;
    }

    #[must_use]
    pub fn current_name(&self) -> Option<&str> {
        self.current_name.as_deref()
    }

    #[must_use]
    pub const fn current_payload(&self) -> Option<&PreservedProfilePayload> {
        self.current_payload.as_ref()
    }

    pub fn refresh_selected(&self, state: &mut ProfileBrowserState, name: Option<&str>) {
        self.refresh(state, name);
    }

    #[must_use]
    pub fn next_profile_name(&self) -> String {
        let names = self
            .path
            .as_deref()
            .and_then(profile_catalog_from_file)
            .map_or_else(Vec::new, |catalog| catalog.names);
        smu_profile::generate_new_profile_name(&names)
    }

    pub fn prepare_shell(&self, state: &mut ProfileBrowserState) {
        if !state.refresh_requested() {
            return;
        }
        state.clear_refresh_request();
        self.refresh(state, None);
    }

    #[must_use]
    pub fn duplicate_profile(&self, source: &str) -> Option<String> {
        let path = self.path.as_deref()?;
        let catalog = profile_catalog_from_file(path)?;
        let new_name = generate_unique_profile_name(source, &catalog.names);
        duplicate_profile_in_file(path, source, &new_name).then_some(new_name)
    }

    #[must_use]
    pub fn handle_action(
        &mut self,
        action: &ProfileAction,
        state: &mut ProfileBrowserState,
    ) -> bool {
        let Some(path) = self.path.as_deref() else {
            self.refresh(state, None);
            return false;
        };
        let (selected_after, succeeded) = match action {
            ProfileAction::SaveTo { .. } | ProfileAction::Load { .. } => return false,
            ProfileAction::Duplicate { source } if source != smu_profile::DEFAULT_PROFILE_NAME => {
                let Some(catalog) = profile_catalog_from_file(path) else {
                    self.refresh(state, None);
                    return false;
                };
                let new_name = generate_unique_profile_name(source, &catalog.names);
                let duplicated = duplicate_profile_in_file(path, source, &new_name);
                (duplicated.then_some(new_name), duplicated)
            }
            ProfileAction::Rename { old_name, new_name } => {
                let renamed = rename_profile_in_file(path, old_name, new_name);
                if renamed && self.current_name.as_deref() == Some(old_name.as_str()) {
                    self.current_name = Some(new_name.clone());
                }
                (renamed.then(|| new_name.clone()), renamed)
            }
            ProfileAction::Delete { name } => {
                let deleted = delete_profile_from_file(path, name);
                if deleted && self.current_name.as_deref() == Some(name.as_str()) {
                    self.clear_loaded();
                }
                (None, deleted)
            }
            ProfileAction::Duplicate { .. } => (None, false),
        };
        self.refresh(state, selected_after.as_deref());
        succeeded
    }

    fn refresh(&self, state: &mut ProfileBrowserState, select_name: Option<&str>) {
        let catalog = self.path.as_deref().and_then(profile_catalog_from_file);
        state.set_current_loaded_profile(self.current_name());
        state.set_catalog(catalog, select_name);
    }
}

fn resolve_settings_path() -> PathBuf {
    let canonical = canonical_settings_directory();
    let executable = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let current = std::env::current_dir().unwrap_or_default();
    resolve_settings_file_path_from_directories(&canonical, &executable, &current)
}

#[cfg(target_os = "linux")]
fn canonical_settings_directory() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(xdg).join("SpencerMacroUtilities");
    }
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|home| home.join(".config").join("SpencerMacroUtilities"))
        .unwrap_or_default()
}

#[cfg(target_os = "windows")]
fn canonical_settings_directory() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|root| root.join("Spencer Macro Utilities"))
        .unwrap_or_default()
}

#[cfg(target_os = "macos")]
fn canonical_settings_directory() -> PathBuf {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|home| {
            home.join("Library").join("Application Support").join("Spencer Macro Utilities")
        })
        .unwrap_or_default()
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn canonical_settings_directory() -> PathBuf {
    PathBuf::new()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic)]

    use super::*;
    use serde_json::{Value, json};
    use smu_profile::{
        CURRENT_SETTINGS_FILENAME, LAST_ACTIVE_PROFILE_KEY, LEGACY_SETTINGS_FILENAME, METADATA_KEY,
        profile_catalog_from_file,
    };
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory {
        path: PathBuf,
    }

    impl TestDirectory {
        fn new() -> Self {
            let counter = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("smu-app-profile-browser-{}-{counter}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("profile browser test directory");
            Self { path }
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn parity_service_is_storage_isolated() {
        let service = ProfileBrowserService::new(true);
        assert!(service.path.is_none());
    }

    #[test]
    fn whole_object_crud_uses_only_supplied_test_file() {
        let temporary = TestDirectory::new();
        let path = temporary.path.join("settings.json");
        fs::write(
            &path,
            br#"{"_metadata":{"last_active_profile":"Profile 1"},"Profile 1":{"x":1},"Profile 2":{"x":2}}"#,
        )
        .expect("profile fixture should be writable");
        let mut service = ProfileBrowserService::from_path_for_testing(path.clone());
        let mut state = ProfileBrowserState::default();
        service.refresh(&mut state, None);

        assert!(service.handle_action(
            &ProfileAction::Duplicate { source: "Profile 1".to_owned() },
            &mut state,
        ));
        let catalog = profile_catalog_from_file(&path).expect("catalog after duplicate");
        assert!(catalog.names.contains(&"Profile 1 (Copy)".to_owned()));

        assert!(service.handle_action(
            &ProfileAction::Rename {
                old_name: "Profile 1 (Copy)".to_owned(),
                new_name: "Renamed".to_owned(),
            },
            &mut state,
        ));
        let catalog = profile_catalog_from_file(&path).expect("catalog after rename");
        assert!(catalog.names.contains(&"Renamed".to_owned()));

        assert!(
            service
                .handle_action(&ProfileAction::Delete { name: "Renamed".to_owned() }, &mut state,)
        );
        let catalog = profile_catalog_from_file(&path).expect("catalog after delete");
        assert!(!catalog.names.contains(&"Renamed".to_owned()));
    }

    #[test]
    fn preferred_payload_follows_metadata_and_preserves_unknown_fields() {
        let temporary = TestDirectory::new();
        let path = temporary.path.join("settings.json");
        fs::write(
            &path,
            serde_json::to_vec(&json!({
                "_metadata": {"last_active_profile": "Profile 2"},
                "Profile 1": {"vk_f5": 0x71},
                "Profile 2": {"vk_f5": 0x72, "future": {"keep": true}}
            }))
            .expect("profile fixture should serialize"),
        )
        .expect("profile fixture should be writable");
        let service = ProfileBrowserService::from_path_for_testing(path);

        match service.startup_source() {
            StartupProfileSource::Structured {
                preferred_name: Some(name),
                preferred_payload: Some(payload),
                ..
            } => {
                assert_eq!(name, "Profile 2");
                assert_eq!(payload.raw_value("vk_f5"), Some(&json!(0x72)));
                assert_eq!(payload.raw_value("future"), Some(&json!({"keep": true})));
            }
            other => panic!("expected preferred structured profile, got {other:?}"),
        }
    }

    #[test]
    fn load_result_distinguishes_unreadable_missing_and_invalid_profiles() {
        let temporary = TestDirectory::new();
        let path = temporary.path.join("settings.json");
        fs::write(&path, b"{ broken").expect("unreadable fixture should write");
        let service = ProfileBrowserService::from_path_for_testing(path.clone());
        assert_eq!(service.load_payload_result("Profile 1"), Err(ProfileLoadFailure::Unreadable));

        fs::write(
            &path,
            serde_json::to_vec(&json!({
                "_metadata": {},
                "Invalid": {"section_toggles": 5}
            }))
            .expect("load-result fixture should serialize"),
        )
        .expect("load-result fixture should write");
        assert_eq!(service.load_payload_result("Missing"), Err(ProfileLoadFailure::Missing));
        assert_eq!(service.load_payload_result("Invalid"), Err(ProfileLoadFailure::Invalid));
    }

    #[test]
    fn save_payload_creates_container_preserves_payload_and_marks_active() {
        let temporary = TestDirectory::new();
        let path = temporary.path.join("nested").join("settings.json");
        let service = ProfileBrowserService::from_path_for_testing(path.clone());
        let payload = PreservedProfilePayload::from_value(&json!({
            "vk_f5": 0x73,
            "future": [1, 2, 3],
            "imported_scripts": [{"path": "saved.lua"}]
        }))
        .expect("payload should validate");

        assert!(service.save_payload("Profile 1", &payload));
        let written: Value =
            serde_json::from_slice(&fs::read(&path).expect("saved profile should be readable"))
                .expect("saved profile container should be JSON");
        assert_eq!(written[METADATA_KEY][LAST_ACTIVE_PROFILE_KEY], json!("Profile 1"));
        assert_eq!(written["Profile 1"]["future"], json!([1, 2, 3]));
        assert_eq!(written["Profile 1"]["imported_scripts"], json!([{"path": "saved.lua"}]));
    }

    #[test]
    fn rename_and_delete_track_current_loaded_profile() {
        let temporary = TestDirectory::new();
        let path = temporary.path.join("settings.json");
        fs::write(
            &path,
            serde_json::to_vec(&json!({
                "_metadata": {"last_active_profile": "Profile 1"},
                "Profile 1": {"vk_f5": 0x71}
            }))
            .expect("profile fixture should serialize"),
        )
        .expect("profile fixture should be writable");
        let payload = profile_payload_from_file(&path, "Profile 1").expect("profile should load");
        let mut service = ProfileBrowserService::from_path_for_testing(path);
        service.mark_loaded("Profile 1".to_owned(), payload);
        let mut state = ProfileBrowserState::default();

        assert!(service.handle_action(
            &ProfileAction::Rename {
                old_name: "Profile 1".to_owned(),
                new_name: "Renamed".to_owned(),
            },
            &mut state,
        ));
        assert_eq!(service.current_name(), Some("Renamed"));
        assert!(service.current_payload().is_some());

        assert!(
            service
                .handle_action(&ProfileAction::Delete { name: "Renamed".to_owned() }, &mut state,)
        );
        assert_eq!(service.current_name(), None);
        assert!(service.current_payload().is_none());
    }

    #[test]
    fn mutation_result_reports_persistence_failure() {
        let temporary = TestDirectory::new();
        let path = temporary.path.join("settings.json");
        fs::create_dir_all(&path).expect("directory-shaped settings target should be created");
        let mut service = ProfileBrowserService::from_path_for_testing(path);
        let mut state = ProfileBrowserState::default();

        assert!(!service.handle_action(
            &ProfileAction::Rename {
                old_name: "Profile 1".to_owned(),
                new_name: "Renamed".to_owned(),
            },
            &mut state,
        ));
        assert!(!service.handle_action(
            &ProfileAction::Delete { name: "Profile 1".to_owned() },
            &mut state,
        ));
    }

    #[test]
    fn filenames_remain_profile_contract_constants() {
        assert_eq!(CURRENT_SETTINGS_FILENAME, "SMCSettings.json");
        assert_eq!(LEGACY_SETTINGS_FILENAME, "RMCSettings.json");
    }
}

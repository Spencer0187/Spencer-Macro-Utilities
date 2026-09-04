use crate::profile_browser::{ProfileAction, ProfileBrowserState};
use smu_profile::{
    delete_profile_from_file, discover_settings_file_path_read_only_from_directories,
    duplicate_profile_in_file, generate_unique_profile_name, profile_catalog_from_file,
    rename_profile_in_file,
};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct ProfileBrowserService {
    path: Option<PathBuf>,
}

impl ProfileBrowserService {
    #[must_use]
    pub fn new(parity_isolated: bool) -> Self {
        let path = (!parity_isolated).then(discover_existing_settings).flatten();
        Self { path }
    }

    #[cfg(test)]
    const fn from_path_for_testing(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    pub fn prepare_shell(&self, state: &mut ProfileBrowserState) {
        if !state.refresh_requested() {
            return;
        }
        state.clear_refresh_request();
        self.refresh(state, None);
    }

    pub fn handle_action(&self, action: &ProfileAction, state: &mut ProfileBrowserState) {
        let Some(path) = self.path.as_deref() else {
            self.refresh(state, None);
            return;
        };
        let selected_after = match action {
            ProfileAction::Duplicate { source } if source != smu_profile::DEFAULT_PROFILE_NAME => {
                let Some(catalog) = profile_catalog_from_file(path) else {
                    self.refresh(state, None);
                    return;
                };
                let new_name = generate_unique_profile_name(source, &catalog.names);
                duplicate_profile_in_file(path, source, &new_name).then_some(new_name)
            }
            ProfileAction::Rename { old_name, new_name } => {
                rename_profile_in_file(path, old_name, new_name).then(|| new_name.clone())
            }
            ProfileAction::Delete { name } => {
                let _ = delete_profile_from_file(path, name);
                None
            }
            ProfileAction::Duplicate { .. } => None,
        };
        self.refresh(state, selected_after.as_deref());
    }

    fn refresh(&self, state: &mut ProfileBrowserState, select_name: Option<&str>) {
        let catalog = self.path.as_deref().and_then(profile_catalog_from_file);
        state.set_catalog(catalog, select_name);
    }
}

fn discover_existing_settings() -> Option<PathBuf> {
    let canonical = canonical_settings_directory();
    let executable = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let current = std::env::current_dir().unwrap_or_default();
    discover_settings_file_path_read_only_from_directories(&canonical, &executable, &current)
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
    #![allow(clippy::expect_used)]

    use super::*;
    use smu_profile::{
        CURRENT_SETTINGS_FILENAME, LEGACY_SETTINGS_FILENAME, profile_catalog_from_file,
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
        let service = ProfileBrowserService::from_path_for_testing(path.clone());
        let mut state = ProfileBrowserState::default();
        service.refresh(&mut state, None);

        service.handle_action(
            &ProfileAction::Duplicate { source: "Profile 1".to_owned() },
            &mut state,
        );
        let catalog = profile_catalog_from_file(&path).expect("catalog after duplicate");
        assert!(catalog.names.contains(&"Profile 1 (Copy)".to_owned()));

        service.handle_action(
            &ProfileAction::Rename {
                old_name: "Profile 1 (Copy)".to_owned(),
                new_name: "Renamed".to_owned(),
            },
            &mut state,
        );
        let catalog = profile_catalog_from_file(&path).expect("catalog after rename");
        assert!(catalog.names.contains(&"Renamed".to_owned()));

        service.handle_action(&ProfileAction::Delete { name: "Renamed".to_owned() }, &mut state);
        let catalog = profile_catalog_from_file(&path).expect("catalog after delete");
        assert!(!catalog.names.contains(&"Renamed".to_owned()));
    }

    #[test]
    fn filenames_remain_profile_contract_constants() {
        assert_eq!(CURRENT_SETTINGS_FILENAME, "SMCSettings.json");
        assert_eq!(LEGACY_SETTINGS_FILENAME, "RMCSettings.json");
    }
}

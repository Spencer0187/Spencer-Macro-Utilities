//! Linux update-check composition for the Settings window.
//!
//! Release-contract validation and Linux `AppImage` staging live in `smu-updater`. This layer owns
//! background check/apply workers plus the C++-compatible confirmation and restart handoff state.

use crate::settings_menu::UpdaterPanelStatus;
use smu_updater::{
    CurlUpdateTransport, LATEST_RELEASE_PAGE_URL, UpdateChecker, UpdaterStatus, apply_linux_update,
    current_platform, linux_auto_apply_supported,
};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Default)]
struct UpdateUiState {
    status: UpdaterStatus,
    checked_once: bool,
    checking: bool,
    applying: bool,
    apply_failed: bool,
    update_confirm_open: bool,
    update_prompt_dismissed: bool,
    restart_requested: bool,
    action_message: String,
}

#[derive(Debug)]
pub struct LinuxUpdateService {
    enabled: bool,
    local_version: String,
    state: Arc<Mutex<UpdateUiState>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl LinuxUpdateService {
    #[must_use]
    pub fn new(local_version: String, enabled: bool) -> Self {
        Self {
            enabled,
            local_version,
            state: Arc::new(Mutex::new(UpdateUiState::default())),
            worker: Mutex::new(None),
        }
    }

    pub fn prepare(&self) -> UpdaterPanelStatus {
        self.reap_finished_worker();
        if self.automatic_check_needed() {
            self.start_check(false);
        }
        self.snapshot()
    }

    fn automatic_check_needed(&self) -> bool {
        let state = lock(&self.state);
        self.enabled && !state.checked_once && !state.checking
    }

    pub fn start_check(&self, force: bool) {
        if !self.enabled {
            return;
        }
        self.reap_finished_worker();
        {
            let mut state = lock(&self.state);
            if state.checking || state.applying || (state.checked_once && !force) {
                return;
            }
            if force {
                state.update_prompt_dismissed = false;
                state.apply_failed = false;
            }
            state.checking = true;
            "Checking for updates...".clone_into(&mut state.action_message);
        }

        let local_version = self.local_version.clone();
        let state = Arc::clone(&self.state);
        let worker = std::thread::spawn(move || {
            let auto_apply_supported = linux_auto_apply_supported();
            let mut checker = UpdateChecker::new(
                CurlUpdateTransport::default(),
                current_platform(),
                auto_apply_supported,
            );
            let status = checker.check_for_update(&local_version);
            let should_open = status.check_succeeded
                && status.update_available
                && status.latest_release.is_some();
            let mut ui = lock(&state);
            ui.checked_once = true;
            ui.checking = false;
            ui.action_message.clone_from(&status.message);
            ui.status = status;
            if should_open && !ui.update_prompt_dismissed {
                ui.update_confirm_open = true;
            }
        });
        *lock(&self.worker) = Some(worker);
    }

    pub fn request_apply(&self) {
        let mut state = lock(&self.state);
        if state.checking || state.applying {
            return;
        }
        if state.status.latest_release.is_none() || !state.status.update_available {
            state.action_message = if state.status.message.is_empty() {
                "No update is available.".to_owned()
            } else {
                state.status.message.clone()
            };
            return;
        }
        if !state.status.auto_apply_supported {
            "An update is available, but automatic installation is not supported in this launch mode."
                .clone_into(&mut state.action_message);
            return;
        }
        if state.status.selected_asset.is_none() {
            "An update is available, but no matching package asset was found for this platform."
                .clone_into(&mut state.action_message);
            return;
        }
        state.update_confirm_open = true;
    }

    pub fn confirm_apply(&self) {
        self.reap_finished_worker();
        let release = {
            let mut state = lock(&self.state);
            if state.applying {
                return;
            }
            let Some(release) = state.status.latest_release.clone() else {
                return;
            };
            state.update_confirm_open = false;
            state.update_prompt_dismissed = true;
            state.applying = true;
            state.apply_failed = false;
            "Downloading update...".clone_into(&mut state.action_message);
            release
        };
        let local_version = self.local_version.clone();
        let state = Arc::clone(&self.state);
        let worker = std::thread::spawn(move || {
            let result = apply_linux_update(&release, &local_version);
            let mut ui = lock(&state);
            ui.applying = false;
            match result {
                Ok(_) => {
                    ui.restart_requested = true;
                    "Update installer launched. The app will restart shortly."
                        .clone_into(&mut ui.action_message);
                }
                Err(error) => {
                    ui.apply_failed = true;
                    ui.action_message = error.to_string();
                }
            }
        });
        *lock(&self.worker) = Some(worker);
    }

    pub fn dismiss_update(&self, message: &str) {
        let mut state = lock(&self.state);
        state.update_confirm_open = false;
        state.update_prompt_dismissed = true;
        message.clone_into(&mut state.action_message);
    }

    #[must_use]
    pub fn restart_requested(&self) -> bool {
        lock(&self.state).restart_requested
    }

    pub fn open_latest_release(&self) {
        let message = match sdl3::url::open_url(LATEST_RELEASE_PAGE_URL) {
            Ok(()) => "Opened the official release page.".to_owned(),
            Err(error) => format!("Could not open the official release page: {error}"),
        };
        lock(&self.state).action_message = message;
    }

    pub fn open_latest_release_and_dismiss(&self) {
        self.open_latest_release();
        let mut state = lock(&self.state);
        state.update_confirm_open = false;
        state.update_prompt_dismissed = true;
    }

    pub fn shutdown(&self) {
        self.reap_finished_worker();
        // Checks and downloads have bounded external work. Successful apply returns only after the
        // helper has launched and `restart_requested` is set, so the normal restart path reaps that
        // worker here. A user-requested close during an unfinished check/apply intentionally detaches
        // the worker, matching the legacy detached-thread shutdown behavior.
        let _ = lock(&self.worker).take();
    }

    fn reap_finished_worker(&self) {
        let mut worker = lock(&self.worker);
        if worker.as_ref().is_some_and(JoinHandle::is_finished)
            && let Some(worker) = worker.take()
        {
            let _ = worker.join();
        }
    }

    fn snapshot(&self) -> UpdaterPanelStatus {
        let state = lock(&self.state);
        UpdaterPanelStatus {
            enabled: self.enabled,
            checked_once: state.checked_once,
            checking: state.checking,
            applying: state.applying,
            apply_failed: state.apply_failed,
            update_confirm_open: state.update_confirm_open,
            check_succeeded: state.status.check_succeeded,
            update_available: state.status.update_available,
            auto_apply_supported: state.status.auto_apply_supported,
            local_version: if state.status.local_version.is_empty() {
                self.local_version.clone()
            } else {
                state.status.local_version.clone()
            },
            latest_version: state.status.latest_version.clone(),
            selected_package: state
                .status
                .selected_asset
                .as_ref()
                .map(|asset| asset.name.clone())
                .unwrap_or_default(),
            status_message: state.status.message.clone(),
            action_message: state.action_message.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smu_updater::{ReleaseAsset, ReleaseInfo};

    fn eligible_status() -> UpdaterStatus {
        let asset = ReleaseAsset {
            name: "Spencer-Macro-Utilities-x86_64.AppImage".to_owned(),
            download_url: "https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/download/V9.9.9/Spencer-Macro-Utilities-x86_64.AppImage".to_owned(),
            size_bytes: 64,
            sha256: "00".repeat(32),
        };
        UpdaterStatus {
            check_succeeded: true,
            update_available: true,
            auto_apply_supported: true,
            local_version: "3.4.0".to_owned(),
            latest_version: "9.9.9".to_owned(),
            message: "An update is available.".to_owned(),
            latest_release: Some(ReleaseInfo {
                tag_name: "V9.9.9".to_owned(),
                version: "9.9.9".to_owned(),
                manifest_schema_version: 1,
                minimum_updater_version: "3.4.0".to_owned(),
                manifest_driven: true,
                assets: vec![asset.clone()],
                ..ReleaseInfo::default()
            }),
            selected_asset: Some(asset),
        }
    }

    #[test]
    fn disabled_service_never_starts_network_work() {
        let service = LinuxUpdateService::new("3.4.0".to_owned(), false);
        let status = service.prepare();
        assert!(!status.enabled);
        assert!(!status.checking);
        assert!(!status.checked_once);
        assert_eq!(status.local_version, "3.4.0");
        assert!(lock(&service.worker).is_none());
    }

    #[test]
    fn apply_request_without_available_release_keeps_prompt_closed() {
        let service = LinuxUpdateService::new("3.4.0".to_owned(), false);
        service.request_apply();
        let state = lock(&service.state);
        assert!(!state.update_confirm_open);
        assert_eq!(state.action_message, "No update is available.");
    }

    #[test]
    fn eligible_apply_request_opens_confirmation_without_starting_worker() {
        let service = LinuxUpdateService::new("3.4.0".to_owned(), false);
        lock(&service.state).status = eligible_status();
        service.request_apply();
        let state = lock(&service.state);
        assert!(state.update_confirm_open);
        assert!(!state.applying);
        drop(state);
        assert!(lock(&service.worker).is_none());
    }

    #[test]
    fn dismiss_update_closes_prompt_and_records_cpp_message() {
        let service = LinuxUpdateService::new("3.4.0".to_owned(), false);
        {
            let mut state = lock(&service.state);
            state.status = eligible_status();
            state.update_confirm_open = true;
        }
        service.dismiss_update("Update skipped.");
        let state = lock(&service.state);
        assert!(!state.update_confirm_open);
        assert!(state.update_prompt_dismissed);
        assert_eq!(state.action_message, "Update skipped.");
        assert!(!state.restart_requested);
        drop(state);
    }

    #[test]
    fn automatic_check_is_needed_at_startup_without_settings_gate() {
        let service = LinuxUpdateService::new("3.4.0".to_owned(), true);
        assert!(service.automatic_check_needed());
        let mut state = lock(&service.state);
        state.checking = true;
        drop(state);
        assert!(!service.automatic_check_needed());
    }
}

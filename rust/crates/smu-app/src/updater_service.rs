//! Linux update-check composition for the Settings window.
//!
//! Release-contract validation lives in `smu-updater`. This layer owns the background check worker
//! and user-facing status only. Automatic replacement is intentionally disabled until the Rust
//! `AppImage` verification/replacement path reaches parity with C++.

use crate::settings_menu::UpdaterPanelStatus;
use smu_updater::{
    CurlUpdateTransport, LATEST_RELEASE_PAGE_URL, UpdateChecker, UpdaterStatus, current_platform,
};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

#[derive(Debug, Default)]
struct UpdateUiState {
    status: UpdaterStatus,
    checked_once: bool,
    checking: bool,
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

    pub fn prepare(&self, settings_open: bool) -> UpdaterPanelStatus {
        self.reap_finished_worker();
        let should_start = {
            let state = lock(&self.state);
            self.enabled && settings_open && !state.checked_once && !state.checking
        };
        if should_start {
            self.start_check(false);
        }
        self.snapshot()
    }

    pub fn start_check(&self, force: bool) {
        if !self.enabled {
            return;
        }
        self.reap_finished_worker();
        {
            let mut state = lock(&self.state);
            if state.checking || (state.checked_once && !force) {
                return;
            }
            state.checking = true;
            "Checking for updates...".clone_into(&mut state.action_message);
        }

        let local_version = self.local_version.clone();
        let state = Arc::clone(&self.state);
        let worker = std::thread::spawn(move || {
            // Rust does not yet own the C++ AppImage replacement/rollback implementation, so this
            // check intentionally advertises auto-apply as unavailable even when running from an
            // otherwise replaceable AppImage.
            let mut checker =
                UpdateChecker::new(CurlUpdateTransport::default(), current_platform(), false);
            let status = checker.check_for_update(&local_version);
            let mut ui = lock(&state);
            ui.checked_once = true;
            ui.checking = false;
            ui.action_message.clone_from(&status.message);
            ui.status = status;
        });
        *lock(&self.worker) = Some(worker);
    }

    pub fn open_latest_release(&self) {
        let message = match sdl3::url::open_url(LATEST_RELEASE_PAGE_URL) {
            Ok(()) => "Opened the official release page.".to_owned(),
            Err(error) => format!("Could not open the official release page: {error}"),
        };
        lock(&self.state).action_message = message;
    }

    pub fn shutdown(&self) {
        self.reap_finished_worker();
        // A curl check has its own 60-second bound. Do not make application shutdown wait for that
        // external process: dropping an unfinished JoinHandle detaches only this non-mutating check.
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

    #[test]
    fn disabled_service_never_starts_network_work() {
        let service = LinuxUpdateService::new("3.4.0".to_owned(), false);
        let status = service.prepare(true);
        assert!(!status.enabled);
        assert!(!status.checking);
        assert!(!status.checked_once);
        assert_eq!(status.local_version, "3.4.0");
        assert!(lock(&service.worker).is_none());
    }

    #[test]
    fn unopened_settings_do_not_start_automatic_check() {
        let service = LinuxUpdateService::new("3.4.0".to_owned(), true);
        let status = service.prepare(false);
        assert!(status.enabled);
        assert!(!status.checking);
        assert!(!status.checked_once);
        assert!(lock(&service.worker).is_none());
    }
}

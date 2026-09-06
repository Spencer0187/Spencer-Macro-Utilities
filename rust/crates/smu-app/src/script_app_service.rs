use crate::{
    profile_adapter::LiveSavedSettings,
    script_profile_bridge::ScriptProfileBridge,
    script_ui::{ScriptAction, ScriptUiState},
};
use sdl3::video::Window;
use smu_core::Hotkey;
use smu_profile::payload::PreservedProfilePayload;
use smu_scripting::{ScriptImportResult, ScriptManager};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Debug)]
enum ScriptDialogResult {
    Selected(PathBuf),
    Canceled,
    Unavailable,
    Error(String),
}

fn script_dialog_error_result(error: sdl3::dialog::DialogError) -> ScriptDialogResult {
    match error {
        sdl3::dialog::DialogError::Canceled => ScriptDialogResult::Canceled,
        sdl3::dialog::DialogError::SdlError(_) => ScriptDialogResult::Unavailable,
        error => ScriptDialogResult::Error(error.to_string()),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[derive(Debug)]
pub struct ScriptAppServices {
    profile: ScriptProfileBridge,
    dialog_busy: Arc<AtomicBool>,
    dialog_result: Arc<Mutex<Option<ScriptDialogResult>>>,
    parity_imported_script: bool,
}

impl ScriptAppServices {
    #[must_use]
    pub fn new(
        manager: Arc<ScriptManager>,
        saved_settings: Arc<LiveSavedSettings>,
        parity_imported_script: bool,
    ) -> Self {
        Self {
            profile: ScriptProfileBridge::new(manager, saved_settings),
            dialog_busy: Arc::new(AtomicBool::new(false)),
            dialog_result: Arc::new(Mutex::new(None)),
            parity_imported_script,
        }
    }

    #[cfg(target_os = "windows")]
    #[must_use]
    pub fn manager(&self) -> Arc<ScriptManager> {
        self.profile.manager_arc()
    }

    #[must_use]
    pub fn serialize_scripts(&self) -> serde_json::Value {
        self.profile.serialize_scripts()
    }

    pub fn apply_profile(&self, payload: &PreservedProfilePayload) {
        self.profile.apply_profile(payload);
    }

    pub fn update_saved_settings(&self, payload: &PreservedProfilePayload) {
        self.profile.update_saved_settings(payload);
    }

    pub fn prepare_ui(&self, state: &mut ScriptUiState) {
        if self.parity_imported_script {
            state.apply_parity_reference();
            return;
        }
        let manager = self.profile.manager();
        state.set_snapshots(manager.snapshot());
        if let Some(index) = state.selected() {
            let running = manager.get(index).is_some_and(|record| record.is_running());
            if !running {
                let _ = manager.call_settings(index, true);
            }
            state.set_controls(manager.settings_controls(index));
        } else {
            state.set_controls(Vec::new());
        }

        let dialog_result = lock(&self.dialog_result).take();
        match dialog_result {
            Some(ScriptDialogResult::Selected(path)) => state.queue_import_candidate(path),
            Some(ScriptDialogResult::Canceled) | None => {}
            Some(ScriptDialogResult::Unavailable) => state.open_file_dialog_fallback(),
            Some(ScriptDialogResult::Error(error)) => state.set_import_error(error),
        }
    }

    #[must_use]
    pub fn input_capture_active(&self, state: &ScriptUiState) -> bool {
        self.dialog_busy.load(Ordering::Acquire) || state.file_dialog_fallback_open()
    }

    pub fn handle_action(
        &self,
        action: &ScriptAction,
        state: &mut ScriptUiState,
        window: &Window,
    ) -> bool {
        let manager = self.profile.manager();
        let mut imported = false;
        match action {
            ScriptAction::OpenImportDialog => self.open_dialog(window),
            ScriptAction::ConfirmImport { path } => {
                let result = manager.import_script_with_result(path);
                state.cancel_import();
                state.set_import_notice(String::new());
                match result {
                    ScriptImportResult::Success => {
                        imported = true;
                        state.set_snapshots(manager.snapshot());
                        state.select_last_script();
                        state.set_import_error(String::new());
                    }
                    ScriptImportResult::UnsupportedExtension => {
                        state.set_import_error("Unsupported script extension.");
                    }
                    ScriptImportResult::AlreadyImported => {
                        state.set_import_error("Script is already imported.");
                    }
                    ScriptImportResult::LoadFailed => state.set_import_error(
                        "Script could not be imported because it is malformed or invalid.",
                    ),
                }
            }
            ScriptAction::SetHotkey { index, hotkey } => {
                let _ = manager.set_hotkey(*index, Hotkey::new(*hotkey));
            }
            ScriptAction::SetEnabled { index, enabled } => {
                let _ = manager.set_enabled(*index, *enabled);
            }
            ScriptAction::SetDisableOutside { index, disabled } => {
                let _ = manager.set_disable_outside_roblox(*index, *disabled);
            }
            ScriptAction::Execute { index } => {
                let _ = manager.spawn_execute_script(*index);
            }
            ScriptAction::Reload { index } => {
                let _ = manager.reload_script(*index);
            }
            ScriptAction::Remove { index } => {
                if manager.remove_script(*index) {
                    state.remove_completed(*index);
                }
            }
            ScriptAction::ForceStop { index } => {
                let _ = manager.force_stop_script(*index);
            }
            ScriptAction::Reset { index } => {
                let _ = manager.reset_script(*index);
            }
            ScriptAction::SettingValue { index, id, value } => {
                let _ = manager.queue_settings_value(*index, id.clone(), value.clone());
            }
            ScriptAction::SettingButton { index, id } => {
                let _ = manager.queue_settings_button(*index, id.clone());
            }
        }
        imported
    }

    pub fn set_process_target(&self, selector: String, take_all: bool) {
        self.profile.manager().set_process_target(selector, take_all);
    }

    pub fn shutdown(&self) {
        self.profile.shutdown();
    }

    fn open_dialog(&self, window: &Window) {
        if self
            .dialog_busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let result_slot = Arc::clone(&self.dialog_result);
        let busy = Arc::clone(&self.dialog_busy);
        let filters =
            [sdl3::dialog::DialogFileFilter { name: "SMU Scripts", pattern: "smus;hss;lua;txt" }];
        let callback: sdl3::dialog::DialogCallback = Box::new(move |result, _filter| {
            let dialog_result = match result {
                Ok(paths) => paths
                    .into_iter()
                    .next()
                    .map_or(ScriptDialogResult::Canceled, ScriptDialogResult::Selected),
                Err(error) => script_dialog_error_result(error),
            };
            *lock(&result_slot) = Some(dialog_result);
            busy.store(false, Ordering::Release);
        });
        if let Err(error) = sdl3::dialog::show_open_file_dialog(
            &filters,
            Option::<&std::path::Path>::None,
            false,
            Some(window),
            callback,
        ) {
            *lock(&self.dialog_result) = Some(script_dialog_error_result(error));
            self.dialog_busy.store(false, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{ScriptAppServices, ScriptDialogResult, script_dialog_error_result};
    use crate::{profile_adapter::LiveSavedSettings, script_ui::ScriptUiState};
    use smu_scripting::{ScriptManager, ScriptManagerConfig, ScriptServices};
    use std::sync::Arc;

    fn service() -> ScriptAppServices {
        ScriptAppServices::new(
            Arc::new(ScriptManager::new(
                ScriptManagerConfig::new("test"),
                ScriptServices::default(),
            )),
            Arc::new(LiveSavedSettings::default()),
            false,
        )
    }

    #[test]
    fn native_dialog_sdl_failure_uses_in_app_fallback() {
        sdl3::set_error("unavailable").expect("set SDL error for dialog fallback test");
        assert!(matches!(
            script_dialog_error_result(sdl3::dialog::DialogError::SdlError(sdl3::get_error())),
            ScriptDialogResult::Unavailable
        ));
        assert!(matches!(
            script_dialog_error_result(sdl3::dialog::DialogError::Canceled),
            ScriptDialogResult::Canceled
        ));
    }

    #[test]
    fn no_backend_service_prepares_empty_script_ui() {
        let service = service();
        let mut state = ScriptUiState::default();
        service.prepare_ui(&mut state);
        assert!(state.selected().is_none());
        assert!(!service.input_capture_active(&state));
        service.shutdown();
    }
}

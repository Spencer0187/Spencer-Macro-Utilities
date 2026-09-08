//! Composed imported-script instance lifecycle.
//!
//! This is the Rust counterpart of C++ `ScriptInstance`: one Lua VM owns one script's API state,
//! service adapters, execution cleanup, settings capture, and cancellation handle. The caller still
//! chooses the worker thread; no script execution is implicitly moved onto the controller thread.

use crate::core_api::{ScriptEnvironment, ScriptLogSink, install_core_api};
use crate::display_api::{install_display_api, install_move_degrees_api};
use crate::input_api::{ScriptExecutionMode, ScriptInputContext, install_input_api};
use crate::input_callbacks::{ScriptInputCallbackContext, install_input_callback_api};
use crate::lua_runtime::{
    LuaExecutionError, LuaExecutionErrorKind, LuaScriptVm, LuaStopHandle, MAX_SCRIPT_RUNTIME,
    ScriptStopReason,
};
use crate::network_api::install_network_api;
use crate::process_api::{ScriptProcessContext, install_process_api, set_freeze};
use crate::roblox_log_api::{ScriptRobloxLogContext, install_roblox_log_api};
use crate::settings_ui::{ScriptSettingsUiContext, SettingsUiControl};
use crate::settings_ui_api::{call_on_settings, install_settings_ui_api};
use serde_json::Value as JsonValue;
use smu_core::Hotkey;
use smu_display::DisplayCapture;
use smu_input::InputDevice;
use smu_network::LagSwitchControl;
use smu_process::{ProcessControl, ProcessDiscovery};
use smu_profile::SavedSettingSource;
use smu_roblox::RobloxLogSource;
use std::fmt;
use std::fs;
use std::path::Path;
use std::sync::Arc;

const BINARY_CHUNK_ERROR: &str =
    "Precompiled Lua chunks are not supported. Please provide a plain-text Lua script.";
const MISSING_EXECUTE_ERROR: &str = "Script does not define onExecute().";

pub type RobloxLogSourceFactory = Arc<dyn Fn() -> Arc<dyn RobloxLogSource> + Send + Sync + 'static>;

#[derive(Clone, Default)]
pub struct ScriptServices {
    pub input: Option<Arc<dyn InputDevice>>,
    pub display: Option<Arc<dyn DisplayCapture>>,
    pub process_discovery: Option<Arc<dyn ProcessDiscovery>>,
    pub process_control: Option<Arc<dyn ProcessControl>>,
    pub network: Option<Arc<dyn LagSwitchControl>>,
    /// Creates one stateful Roblox log source per script instance, matching C++ `ScriptInstance` ownership.
    pub roblox_log_factory: Option<RobloxLogSourceFactory>,
    pub saved_settings: Option<Arc<dyn SavedSettingSource>>,
    pub logger: Option<Arc<dyn ScriptLogSink>>,
}

impl fmt::Debug for ScriptServices {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScriptServices")
            .field("input", &self.input.is_some())
            .field("display", &self.display.is_some())
            .field("process_discovery", &self.process_discovery.is_some())
            .field("process_control", &self.process_control.is_some())
            .field("network", &self.network.is_some())
            .field("roblox_log_factory", &self.roblox_log_factory.is_some())
            .field("saved_settings", &self.saved_settings.is_some())
            .field("logger", &self.logger.is_some())
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct ScriptInstanceConfig {
    pub local_version: String,
    pub memory_limit_mb: Option<usize>,
    pub initial_ui_state: JsonValue,
    pub process_selector: String,
    pub take_all_process_ids: bool,
    pub script_hotkey: Hotkey,
    pub strict_hotkey_matching: bool,
}

impl ScriptInstanceConfig {
    #[must_use]
    pub fn new(local_version: impl Into<String>) -> Self {
        Self {
            local_version: local_version.into(),
            memory_limit_mb: None,
            initial_ui_state: JsonValue::Object(serde_json::Map::new()),
            process_selector: String::new(),
            take_all_process_ids: false,
            script_hotkey: Hotkey::new(0),
            strict_hotkey_matching: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScriptCleanupReason {
    Completed,
    Cancelled,
    Timeout,
    Error,
}

impl ScriptCleanupReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Timeout => "timeout",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScriptInstanceDiagnostics {
    pub loaded: bool,
    pub running: bool,
    pub last_error: String,
    pub last_warning: String,
    pub last_cleanup_warning: String,
}

#[derive(Debug)]
pub enum ScriptLoadError {
    Io(std::io::Error),
    Lua(LuaExecutionError),
}

impl fmt::Display for ScriptLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Lua(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ScriptLoadError {}

pub struct LuaScriptInstance {
    vm: LuaScriptVm,
    services: ScriptServices,
    execution_context: ScriptInputContext,
    input_callbacks: ScriptInputCallbackContext,
    process_context: ScriptProcessContext,
    roblox_log_context: ScriptRobloxLogContext,
    settings_ui_context: ScriptSettingsUiContext,
    diagnostics: ScriptInstanceDiagnostics,
    has_settings_callback: bool,
}

impl fmt::Debug for LuaScriptInstance {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LuaScriptInstance")
            .field("vm", &self.vm)
            .field("services", &self.services)
            .field("execution_context", &self.execution_context)
            .field("input_callbacks", &self.input_callbacks)
            .field("process_context", &self.process_context)
            .field("roblox_log_context", &self.roblox_log_context)
            .field("settings_ui_context", &self.settings_ui_context)
            .field("diagnostics", &self.diagnostics)
            .field("has_settings_callback", &self.has_settings_callback)
            .finish()
    }
}

fn initialization_error(error: impl fmt::Display) -> LuaExecutionError {
    LuaExecutionError {
        kind: LuaExecutionErrorKind::Initialization,
        message: error.to_string(),
        during_coroutine_resume: false,
    }
}

fn cleanup_reason(error: Option<&LuaExecutionError>) -> ScriptCleanupReason {
    match error.map(|error| &error.kind) {
        None => ScriptCleanupReason::Completed,
        Some(LuaExecutionErrorKind::Cancelled) => ScriptCleanupReason::Cancelled,
        Some(LuaExecutionErrorKind::Timeout) => ScriptCleanupReason::Timeout,
        Some(
            LuaExecutionErrorKind::Memory
            | LuaExecutionErrorKind::Syntax
            | LuaExecutionErrorKind::Runtime
            | LuaExecutionErrorKind::Initialization,
        ) => ScriptCleanupReason::Error,
    }
}

fn normalize_load_error(mut error: LuaExecutionError) -> LuaExecutionError {
    if error.message.to_ascii_lowercase().contains("binary") {
        BINARY_CHUNK_ERROR.clone_into(&mut error.message);
    }
    error
}

impl LuaScriptInstance {
    /// Create one composed VM and register every currently ported service-backed Lua API.
    ///
    /// # Errors
    ///
    /// Returns an initialization failure when the VM or any API adapter cannot be installed.
    pub fn new(
        config: ScriptInstanceConfig,
        services: ScriptServices,
    ) -> Result<Self, LuaExecutionError> {
        let vm = LuaScriptVm::new(config.memory_limit_mb)?;
        let execution_context = ScriptInputContext::default();
        execution_context.set_script_hotkey(config.script_hotkey);
        execution_context.set_strict_hotkey_matching(config.strict_hotkey_matching);
        let input_callbacks = ScriptInputCallbackContext::default();
        let process_context = ScriptProcessContext::default();
        process_context.set_target_selector(config.process_selector, config.take_all_process_ids);
        let roblox_log_context = ScriptRobloxLogContext::default();
        let roblox_log = services.roblox_log_factory.as_ref().map(|factory| factory());
        let settings_ui_context = ScriptSettingsUiContext::from_ui_state(&config.initial_ui_state);

        install_core_api(
            &vm,
            ScriptEnvironment::new(config.local_version),
            services.saved_settings.clone(),
            services.display.clone(),
            services.logger.clone(),
        )
        .map_err(initialization_error)?;
        install_input_api(&vm, execution_context.clone(), services.input.clone())
            .map_err(initialization_error)?;
        install_input_callback_api(
            &vm,
            &execution_context,
            &input_callbacks,
            services.input.as_ref(),
        )
        .map_err(initialization_error)?;
        install_display_api(
            &vm,
            execution_context.clone(),
            services.input.clone(),
            services.display.clone(),
        )
        .map_err(initialization_error)?;
        install_move_degrees_api(
            &vm,
            execution_context.clone(),
            services.input.clone(),
            services.display.clone(),
            services.saved_settings.as_deref(),
        )
        .map_err(initialization_error)?;
        install_process_api(
            &vm,
            execution_context.clone(),
            process_context.clone(),
            services.process_discovery.clone(),
            services.process_control.clone(),
        )
        .map_err(initialization_error)?;
        install_network_api(
            &vm,
            execution_context.clone(),
            services.network.clone(),
            services.logger.clone(),
        )
        .map_err(initialization_error)?;
        install_roblox_log_api(
            &vm,
            execution_context.clone(),
            roblox_log_context.clone(),
            roblox_log,
        )
        .map_err(initialization_error)?;
        install_settings_ui_api(&vm, &execution_context, &settings_ui_context)
            .map_err(initialization_error)?;

        Ok(Self {
            vm,
            services,
            execution_context,
            input_callbacks,
            process_context,
            roblox_log_context,
            settings_ui_context,
            diagnostics: ScriptInstanceDiagnostics::default(),
            has_settings_callback: false,
        })
    }

    #[must_use]
    pub fn stop_handle(&self) -> LuaStopHandle {
        self.vm.stop_handle()
    }

    #[must_use]
    pub const fn vm(&self) -> &LuaScriptVm {
        &self.vm
    }

    #[must_use]
    pub const fn diagnostics(&self) -> &ScriptInstanceDiagnostics {
        &self.diagnostics
    }

    #[must_use]
    pub const fn has_settings_callback(&self) -> bool {
        self.has_settings_callback
    }

    #[must_use]
    pub const fn execution_context(&self) -> &ScriptInputContext {
        &self.execution_context
    }

    #[must_use]
    pub fn execution_context_handle(&self) -> ScriptInputContext {
        self.execution_context.clone()
    }

    #[must_use]
    pub const fn process_context(&self) -> &ScriptProcessContext {
        &self.process_context
    }

    #[must_use]
    pub fn process_context_handle(&self) -> ScriptProcessContext {
        self.process_context.clone()
    }

    #[must_use]
    pub const fn settings_ui_context(&self) -> &ScriptSettingsUiContext {
        &self.settings_ui_context
    }

    #[must_use]
    pub fn settings_ui_context_handle(&self) -> ScriptSettingsUiContext {
        self.settings_ui_context.clone()
    }

    /// Load and execute one plain-text script chunk, then run the non-render settings callback and
    /// enforce the manager's historical `onExecute` requirement.
    ///
    /// A failing `onSettings` call leaves the chunk load successful, matching C++ `loadRecord`.
    ///
    /// # Errors
    ///
    /// Returns top-level Lua failures or the missing-`onExecute` compatibility error.
    pub fn load_source(
        &mut self,
        source: &[u8],
        chunk_name: &str,
    ) -> Result<(), LuaExecutionError> {
        self.diagnostics.loaded = false;
        self.diagnostics.running = false;
        self.diagnostics.last_error.clear();
        self.diagnostics.last_cleanup_warning.clear();

        if let Err(error) =
            self.vm.execute_text_bytes_with_limit(source, chunk_name, MAX_SCRIPT_RUNTIME)
        {
            let error = normalize_load_error(error);
            self.diagnostics.last_error.clone_from(&error.message);
            if !matches!(
                error.kind,
                LuaExecutionErrorKind::Syntax
                    | LuaExecutionErrorKind::Memory
                    | LuaExecutionErrorKind::Initialization
            ) {
                self.log_warning(&format!(
                    "Imported script failed during load script: {}",
                    error.message
                ));
            }
            return Err(error);
        }

        self.has_settings_callback = self.vm.has_function("onSettings");
        if self.has_settings_callback
            && let Err(error) = call_on_settings(
                &self.vm,
                &self.execution_context,
                &self.settings_ui_context,
                false,
            )
        {
            self.diagnostics.last_error.clone_from(&error.message);
            self.log_warning(&format!(
                "Imported script failed during onSettings: {}",
                error.message
            ));
        }

        if !self.vm.has_function("onExecute") {
            let error = LuaExecutionError {
                kind: LuaExecutionErrorKind::Runtime,
                message: MISSING_EXECUTE_ERROR.to_owned(),
                during_coroutine_resume: false,
            };
            self.diagnostics.last_error.clone_from(&error.message);
            return Err(error);
        }

        self.diagnostics.loaded = true;
        Ok(())
    }

    /// Read and load a script file without allowing precompiled Lua chunks.
    ///
    /// # Errors
    ///
    /// Returns filesystem or Lua load/execution failures.
    pub fn load_file(&mut self, path: &Path) -> Result<(), ScriptLoadError> {
        let bytes = fs::read(path).map_err(|error| {
            self.diagnostics.last_error = error.to_string();
            ScriptLoadError::Io(error)
        })?;
        self.load_source(&bytes, &path.to_string_lossy()).map_err(ScriptLoadError::Lua)
    }

    /// Execute `onExecute` on the caller's current thread and always perform the C++ end-of-run
    /// cleanup sequence.
    ///
    /// # Errors
    ///
    /// Returns runtime, timeout, cancellation, or missing-function failures from `onExecute`.
    pub fn execute(&mut self) -> Result<(), LuaExecutionError> {
        self.execute_with_started(|| {})
    }

    /// Execute `onExecute` and notify the owner only after the VM execution budget is armed.
    /// This lets `ScriptManager` publish `running=true` without a pre-start cancellation race.
    ///
    /// # Errors
    ///
    /// Returns runtime, timeout, cancellation, or missing-function failures from `onExecute`.
    pub fn execute_with_started(
        &mut self,
        started: impl FnOnce(),
    ) -> Result<(), LuaExecutionError> {
        self.diagnostics.running = true;
        self.diagnostics.last_warning.clear();
        self.execution_context.clear_lag_switch_diagnostics();
        let has_execute_callback = self.vm.has_function("onExecute");
        let result = if has_execute_callback {
            self.vm.call_no_args_with_started("onExecute", MAX_SCRIPT_RUNTIME, started)
        } else {
            started();
            Err(LuaExecutionError {
                kind: LuaExecutionErrorKind::Runtime,
                message: MISSING_EXECUTE_ERROR.to_owned(),
                during_coroutine_resume: false,
            })
        };

        match &result {
            Ok(()) => {
                self.diagnostics.last_error = self.execution_context.lag_switch_error();
            }
            Err(error)
                if error.kind == LuaExecutionErrorKind::Cancelled
                    && !error.during_coroutine_resume =>
            {
                self.diagnostics.last_error.clear();
            }
            Err(error) => {
                self.diagnostics.last_error.clone_from(&error.message);
                if error.during_coroutine_resume {
                    self.log_warning(&format!(
                        "Imported script failed during coroutine sleep resume: {}",
                        error.message
                    ));
                } else if has_execute_callback {
                    self.log_warning(&format!(
                        "Imported script failed during run onExecute: {}",
                        error.message
                    ));
                }
            }
        }
        self.diagnostics.last_warning = self.execution_context.lag_switch_warning();

        let reason = cleanup_reason(result.as_ref().err());
        self.finish_execution(reason);
        result
    }

    /// Invoke `onSettings`; render mode consumes app-queued control edits/button presses.
    ///
    /// # Errors
    ///
    /// Returns the settings callback's Lua failure.
    pub fn call_settings(&mut self, render_mode: bool) -> Result<bool, LuaExecutionError> {
        let result = call_on_settings(
            &self.vm,
            &self.execution_context,
            &self.settings_ui_context,
            render_mode,
        );
        if let Err(error) = &result {
            self.diagnostics.last_error.clone_from(&error.message);
            self.log_warning(&format!(
                "Imported script failed during onSettings: {}",
                error.message
            ));
        }
        result
    }

    #[must_use]
    pub fn settings_controls(&self) -> Vec<SettingsUiControl> {
        self.settings_ui_context.controls()
    }

    #[must_use]
    pub fn ui_state(&self) -> JsonValue {
        self.settings_ui_context.ui_state()
    }

    /// Replace persisted script UI state and rebuild the Lua `settings` table/cache.
    ///
    /// # Errors
    ///
    /// Returns an initialization-style error if rebuilding the settings API fails.
    pub fn replace_ui_state(&mut self, value: &JsonValue) -> Result<(), LuaExecutionError> {
        self.settings_ui_context.replace_ui_state(value);
        install_settings_ui_api(&self.vm, &self.execution_context, &self.settings_ui_context)
            .map_err(initialization_error)
    }

    fn finish_execution(&mut self, reason: ScriptCleanupReason) {
        self.execution_context.release_all_managed_hotkeys(self.services.input.as_ref());
        self.input_callbacks.clear_callbacks(&self.vm);
        self.roblox_log_context.clear_callbacks(&self.vm);
        self.call_cleanup(reason);
        self.release_lag_switch_controls();
        self.diagnostics.running = false;
    }

    fn call_cleanup(&mut self, reason: ScriptCleanupReason) {
        if !self.vm.has_function("onCleanup") {
            return;
        }
        let previous_mode = self.execution_context.mode();
        self.execution_context.set_mode(ScriptExecutionMode::Cleanup);
        let result = self.vm.call_cleanup(reason.as_str());
        self.execution_context.set_mode(previous_mode);
        match result {
            Ok(()) => self.diagnostics.last_cleanup_warning.clear(),
            Err(error) => {
                self.diagnostics.last_cleanup_warning =
                    if error.kind == LuaExecutionErrorKind::Timeout {
                        "Imported script onCleanup timed out.".to_owned()
                    } else {
                        format!("Imported script failed during onCleanup: {}", error.message)
                    };
                self.log_warning(&self.diagnostics.last_cleanup_warning.clone());
            }
        }
    }

    fn log_warning(&self, message: &str) {
        if let Some(logger) = &self.services.logger {
            logger.log_warning(message.as_bytes());
        }
    }

    fn release_lag_switch_controls(&self) {
        if !self.execution_context.take_lag_switch_touched() {
            return;
        }
        if let Some(network) = &self.services.network {
            network.clear_script_state(self.execution_context.owner_token());
        }
    }

    /// Release persistent instance-owned state. This mirrors C++ `cleanup()` rather than normal
    /// end-of-`onExecute` cleanup: process freeze is released here, not automatically after a run.
    pub fn shutdown(&mut self) {
        self.vm.stop_handle().request_cancel();
        self.execution_context.release_all_managed_hotkeys(self.services.input.as_ref());
        self.input_callbacks.clear_callbacks(&self.vm);
        self.roblox_log_context.clear_callbacks(&self.vm);
        set_freeze(
            false,
            &self.process_context,
            self.services.process_discovery.as_ref(),
            self.services.process_control.as_ref(),
        );
        self.release_lag_switch_controls();
        self.execution_context.set_mode(ScriptExecutionMode::Normal);
        self.diagnostics.running = false;
        self.diagnostics.loaded = false;
        self.has_settings_callback = false;
    }

    #[must_use]
    pub fn last_stop_reason(&self) -> ScriptStopReason {
        self.vm.stop_handle().reason()
    }
}

impl Drop for LuaScriptInstance {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use smu_input::{InputInjector, KeyState};
    use smu_network::{LagSwitchConfig, LagSwitchInitializationFailure, LagSwitchState};
    use smu_process::ProcessRecord;
    use std::io;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum InputEvent {
        Hold(smu_core::KeyCode),
        Release(smu_core::KeyCode),
    }

    #[derive(Debug, Default)]
    struct FakeInput {
        events: Mutex<Vec<InputEvent>>,
    }

    impl FakeInput {
        fn events(&self) -> Vec<InputEvent> {
            self.events.lock().expect("input events lock").clone()
        }
    }

    impl KeyState for FakeInput {
        fn is_pressed(&self, _key: smu_core::KeyCode) -> bool {
            false
        }
    }

    impl InputInjector for FakeInput {
        fn hold_key(&self, key: smu_core::KeyCode, _extended: bool) {
            self.events.lock().expect("input events lock").push(InputEvent::Hold(key));
        }

        fn release_key(&self, key: smu_core::KeyCode, _extended: bool) {
            self.events.lock().expect("input events lock").push(InputEvent::Release(key));
        }

        fn move_mouse(&self, _dx: i32, _dy: i32) {}
        fn mouse_wheel(&self, _delta: i32) {}
    }

    #[derive(Debug)]
    struct FakeNetwork {
        state: Mutex<LagSwitchState>,
        clears: Mutex<Vec<usize>>,
        initialized: AtomicBool,
        initialize_success: bool,
        initialization_failure: LagSwitchInitializationFailure,
    }

    impl Default for FakeNetwork {
        fn default() -> Self {
            Self {
                state: Mutex::new(LagSwitchState::default()),
                clears: Mutex::new(Vec::new()),
                initialized: AtomicBool::new(true),
                initialize_success: true,
                initialization_failure: LagSwitchInitializationFailure::Error(String::new()),
            }
        }
    }

    impl FakeNetwork {
        fn cold_failure(initialization_failure: LagSwitchInitializationFailure) -> Self {
            Self {
                initialized: AtomicBool::new(false),
                initialize_success: false,
                initialization_failure,
                ..Self::default()
            }
        }

        fn state(&self) -> std::sync::MutexGuard<'_, LagSwitchState> {
            self.state.lock().expect("network state lock")
        }
    }

    impl LagSwitchControl for FakeNetwork {
        fn initialize(&self) -> bool {
            if self.initialize_success {
                self.initialized.store(true, Ordering::Release);
                true
            } else {
                false
            }
        }

        fn is_initialized(&self) -> bool {
            self.initialized.load(Ordering::Acquire)
        }

        fn initialization_failure(&self) -> LagSwitchInitializationFailure {
            self.initialization_failure.clone()
        }

        fn is_available(&self) -> bool {
            true
        }

        fn effective_config(&self) -> LagSwitchConfig {
            self.state().effective_config()
        }

        fn is_blocking_active(&self) -> bool {
            self.state().effective_config().currently_blocking
        }

        fn is_base_blocking_active(&self) -> bool {
            self.state().base_blocking()
        }

        fn set_base_blocking_active(&self, active: bool) {
            self.state().set_base_blocking(active);
        }

        fn set_base_config(&self, config: LagSwitchConfig) {
            self.state().set_base_config(config);
        }

        fn unsupported_reason(&self) -> String {
            String::new()
        }

        fn set_script_blocking_active(&self, owner_token: usize, active: bool) {
            self.state().set_script_blocking(owner_token, active);
        }

        fn set_script_config_override(&self, owner_token: usize, config: LagSwitchConfig) {
            self.state().set_script_config_override(owner_token, config);
        }

        fn clear_script_config_override(&self, owner_token: usize) {
            self.state().clear_script_config_override(owner_token);
        }

        fn clear_script_state(&self, owner_token: usize) {
            self.clears.lock().expect("network clear lock").push(owner_token);
            self.state().clear_script_state(owner_token);
        }
    }

    #[derive(Debug)]
    struct FakeDiscovery;

    impl ProcessDiscovery for FakeDiscovery {
        fn snapshot(&self) -> io::Result<Vec<ProcessRecord>> {
            Ok(vec![ProcessRecord {
                pid: 42,
                parent_pid: Some(1),
                command_name: Some("game".to_owned()),
                executable_name: None,
            }])
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum ProcessEvent {
        Suspend(u32),
        Resume(u32),
    }

    #[derive(Debug, Default)]
    struct FakeProcessControl(Mutex<Vec<ProcessEvent>>);

    impl ProcessControl for FakeProcessControl {
        fn suspend(&self, pid: u32) -> bool {
            self.0.lock().expect("process events lock").push(ProcessEvent::Suspend(pid));
            true
        }

        fn resume(&self, pid: u32) -> bool {
            self.0.lock().expect("process events lock").push(ProcessEvent::Resume(pid));
            true
        }
    }

    fn instance_with_network(
        network: Arc<FakeNetwork>,
    ) -> (LuaScriptInstance, Arc<FakeInput>, Arc<FakeNetwork>, Arc<FakeProcessControl>) {
        instance_with_network_and_logger(network, None)
    }

    fn instance_with_network_and_logger(
        network: Arc<FakeNetwork>,
        logger: Option<Arc<dyn ScriptLogSink>>,
    ) -> (LuaScriptInstance, Arc<FakeInput>, Arc<FakeNetwork>, Arc<FakeProcessControl>) {
        let input = Arc::new(FakeInput::default());
        let process = Arc::new(FakeProcessControl::default());
        let services = ScriptServices {
            input: Some(input.clone()),
            process_discovery: Some(Arc::new(FakeDiscovery)),
            process_control: Some(process.clone()),
            network: Some(network.clone()),
            logger,
            ..ScriptServices::default()
        };
        let mut config = ScriptInstanceConfig::new("3.4.0");
        config.process_selector = "game".to_owned();
        let instance =
            LuaScriptInstance::new(config, services).expect("instance should initialize");
        (instance, input, network, process)
    }

    fn instance_with_services()
    -> (LuaScriptInstance, Arc<FakeInput>, Arc<FakeNetwork>, Arc<FakeProcessControl>) {
        instance_with_network(Arc::new(FakeNetwork::default()))
    }

    #[derive(Debug, Default)]
    struct RecordingLogger {
        warnings: Mutex<Vec<String>>,
    }

    impl RecordingLogger {
        fn warnings(&self) -> Vec<String> {
            self.warnings.lock().expect("warning log lock").clone()
        }
    }

    impl ScriptLogSink for RecordingLogger {
        fn log_info(&self, _message: &[u8]) {}

        fn log_warning(&self, message: &[u8]) {
            self.warnings
                .lock()
                .expect("warning log lock")
                .push(String::from_utf8_lossy(message).into_owned());
        }
    }

    #[derive(Debug)]
    struct IdentifiedLogSource(usize);

    impl RobloxLogSource for IdentifiedLogSource {
        fn read_log(&self, _include_existing: bool) -> smu_roblox::RobloxLogSnapshot {
            smu_roblox::RobloxLogSnapshot {
                available: true,
                path: format!("source-{}", self.0).into(),
                ..smu_roblox::RobloxLogSnapshot::default()
            }
        }
    }

    #[test]
    fn roblox_log_factory_creates_independent_source_per_script_instance() {
        let next_id = Arc::new(AtomicUsize::new(0));
        let factory_counter = Arc::clone(&next_id);
        let factory: RobloxLogSourceFactory = Arc::new(move || {
            let id = factory_counter.fetch_add(1, Ordering::Relaxed);
            Arc::new(IdentifiedLogSource(id))
        });
        let services =
            ScriptServices { roblox_log_factory: Some(factory), ..ScriptServices::default() };
        let first = LuaScriptInstance::new(ScriptInstanceConfig::new("3.4.0"), services.clone())
            .expect("first instance should initialize");
        let second = LuaScriptInstance::new(ScriptInstanceConfig::new("3.4.0"), services)
            .expect("second instance should initialize");

        assert_eq!(next_id.load(Ordering::Relaxed), 2);
        assert_eq!(
            first
                .vm()
                .eval::<String>("return readRobloxLog().path", Duration::from_secs(1))
                .expect("first log source path should be readable"),
            "source-0"
        );
        assert_eq!(
            second
                .vm()
                .eval::<String>("return readRobloxLog().path", Duration::from_secs(1))
                .expect("second log source path should be readable"),
            "source-1"
        );
    }

    #[test]
    fn execution_settings_and_cleanup_failures_publish_cpp_warning_text() {
        let logger = Arc::new(RecordingLogger::default());
        let services = ScriptServices { logger: Some(logger.clone()), ..ScriptServices::default() };
        let mut instance = LuaScriptInstance::new(ScriptInstanceConfig::new("3.4.0"), services)
            .expect("instance should initialize");
        instance
            .load_source(
                b"function onSettings() error('settings boom') end; function onExecute() error('execute boom') end; function onCleanup() error('cleanup boom') end",
                "warnings.lua",
            )
            .expect("settings failure should not make load fail");
        instance.call_settings(true).expect_err("settings callback should fail");
        instance.execute().expect_err("execute callback should fail");

        let warnings = logger.warnings();
        assert!(warnings.iter().any(|warning| {
            warning.contains("Imported script failed during onSettings:")
                && warning.contains("settings boom")
        }));
        assert!(warnings.iter().any(|warning| {
            warning.contains("Imported script failed during run onExecute:")
                && warning.contains("execute boom")
        }));
        assert!(warnings.iter().any(|warning| {
            warning.contains("Imported script failed during onCleanup:")
                && warning.contains("cleanup boom")
        }));
    }

    #[test]
    fn coroutine_sleep_resume_failure_publishes_cpp_warning_context() {
        let logger = Arc::new(RecordingLogger::default());
        let services = ScriptServices { logger: Some(logger.clone()), ..ScriptServices::default() };
        let mut instance = LuaScriptInstance::new(ScriptInstanceConfig::new("3.4.0"), services)
            .expect("instance should initialize");
        instance
            .load_source(
                b"function onExecute() local co=coroutine.create(function() sleep(1); error('resume boom') end); assert(coroutine.resume(co)) end",
                "coroutine-warning.lua",
            )
            .expect("script should load");

        let error = instance.execute().expect_err("resume failure should fail execution");
        assert!(error.during_coroutine_resume);
        let warnings = logger.warnings();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].starts_with("Imported script failed during coroutine sleep resume: "));
        assert!(warnings[0].contains("resume boom"));
    }

    #[test]
    fn load_runs_settings_then_requires_on_execute() {
        let (mut instance, _, _, _) = instance_with_services();
        instance
            .load_source(
                b"settings_seen=0; function onSettings() settings_seen=settings_seen+1 end; function onExecute() end",
                "ok.lua",
            )
            .expect("script should load");
        assert!(instance.diagnostics().loaded);
        assert!(instance.has_settings_callback());
        assert_eq!(
            instance
                .vm()
                .eval::<i64>("return settings_seen", Duration::from_secs(1))
                .expect("settings counter should return"),
            1
        );

        let logger = Arc::new(RecordingLogger::default());
        let services = ScriptServices { logger: Some(logger.clone()), ..ScriptServices::default() };
        let mut missing = LuaScriptInstance::new(ScriptInstanceConfig::new("3.4.0"), services)
            .expect("missing callback instance should initialize");
        let error = missing
            .load_source(b"function onSettings() settings.x=true end", "missing.lua")
            .expect_err("missing onExecute should fail manager compatibility gate");
        assert_eq!(error.message, MISSING_EXECUTE_ERROR);
        assert!(!missing.diagnostics().loaded);
        assert!(logger.warnings().is_empty());
    }

    #[test]
    fn failing_on_settings_does_not_fail_load_like_cpp_manager() {
        let (mut instance, _, _, _) = instance_with_services();
        instance
            .load_source(
                b"function onSettings() error('settings boom') end; function onExecute() end",
                "settings-error.lua",
            )
            .expect("settings failure should not make load fail");
        assert!(instance.diagnostics().loaded);
        assert!(instance.diagnostics().last_error.contains("settings boom"));
    }

    #[test]
    fn execute_releases_managed_input_before_cleanup_and_network_after_cleanup() {
        let (mut instance, input, network, _) = instance_with_services();
        instance
            .load_source(
                concat!(
                    "cleanup_reason=''; cleanup_active=false; ",
                    "function onExecute() input.hold('Ctrl+A'); lagSwitch(true) end; ",
                    "function onCleanup(reason) cleanup_reason=reason; cleanup_active=getLagSwitchStatus().active end"
                )
                .as_bytes(),
                "run.lua",
            )
            .expect("script should load");
        instance.execute().expect("script should complete");
        assert_eq!(
            input.events(),
            [
                InputEvent::Hold(smu_core::KeyCode::CONTROL),
                InputEvent::Hold(smu_core::KeyCode::A),
                InputEvent::Release(smu_core::KeyCode::CONTROL),
                InputEvent::Release(smu_core::KeyCode::A),
            ]
        );
        assert_eq!(
            instance
                .vm()
                .eval::<(String, bool)>(
                    "return cleanup_reason,cleanup_active",
                    Duration::from_secs(1),
                )
                .expect("cleanup state should return"),
            ("completed".to_owned(), true)
        );
        assert!(!network.is_blocking_active());
        assert_eq!(network.clears.lock().expect("clear lock").len(), 1);
        assert!(!instance.diagnostics().running);
    }

    #[test]
    fn runtime_error_uses_error_cleanup_reason_and_preserves_last_error() {
        let (mut instance, _, _, _) = instance_with_services();
        instance
            .load_source(
                b"reason=''; function onExecute() error('boom') end; function onCleanup(r) reason=r end",
                "error.lua",
            )
            .expect("script should load");
        let error = instance.execute().expect_err("onExecute should fail");
        assert!(error.message.contains("boom"));
        assert!(instance.diagnostics().last_error.contains("boom"));
        assert_eq!(
            instance
                .vm()
                .eval::<String>("return reason", Duration::from_secs(1))
                .expect("cleanup reason should return"),
            "error"
        );
    }

    #[test]
    fn lag_switch_admin_failure_surfaces_warning_without_failing_script() {
        let network = Arc::new(FakeNetwork::cold_failure(
            LagSwitchInitializationFailure::RequiresAdministrator,
        ));
        let logger = Arc::new(RecordingLogger::default());
        let (mut instance, _, _, _) =
            instance_with_network_and_logger(network, Some(logger.clone()));
        instance
            .load_source(b"function onExecute() lagSwitch(true) end", "lag-admin.lua")
            .expect("script should load");

        instance.execute().expect("lag-switch init warning must not fail script");

        assert_eq!(
            instance.diagnostics().last_warning,
            crate::network_api::LAG_SWITCH_REQUIRES_ADMIN_WARNING
        );
        assert_eq!(
            logger.warnings(),
            vec![crate::network_api::LAG_SWITCH_REQUIRES_ADMIN_WARNING.to_owned()]
        );
        assert!(instance.diagnostics().last_error.is_empty());
    }

    #[test]
    fn lag_switch_backend_failure_surfaces_last_error_without_failing_script() {
        let network = Arc::new(FakeNetwork::cold_failure(LagSwitchInitializationFailure::Error(
            "driver probe failed".to_owned(),
        )));
        let logger = Arc::new(RecordingLogger::default());
        let (mut instance, _, _, _) =
            instance_with_network_and_logger(network, Some(logger.clone()));
        instance
            .load_source(b"function onExecute() lagSwitch(true) end", "lag-error.lua")
            .expect("script should load");

        instance.execute().expect("lag-switch init errors remain non-throwing like C++");

        assert_eq!(instance.diagnostics().last_error, "driver probe failed");
        assert_eq!(logger.warnings(), vec!["driver probe failed".to_owned()]);
        assert!(instance.diagnostics().last_warning.is_empty());
    }

    #[test]
    fn cancellation_clears_last_error_and_reports_cancelled_cleanup_reason() {
        let (mut instance, _, _, _) = instance_with_services();
        instance
            .load_source(
                b"reason=''; function onExecute() sleep(5000) end; function onCleanup(r) reason=r end",
                "cancel.lua",
            )
            .expect("script should load");
        let stop = instance.stop_handle();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(5));
            stop.request_cancel();
        });
        let error = instance.execute().expect_err("run should cancel");
        thread.join().expect("cancel thread should finish");
        assert_eq!(error.kind, LuaExecutionErrorKind::Cancelled);
        assert!(instance.diagnostics().last_error.is_empty());
        assert_eq!(
            instance
                .vm()
                .eval::<String>("return reason", Duration::from_secs(1))
                .expect("cleanup reason should return"),
            "cancelled"
        );
    }

    #[test]
    fn process_freeze_persists_after_execute_but_shutdown_releases_it() {
        let (mut instance, _, _, process) = instance_with_services();
        instance
            .load_source(b"function onExecute() freeze(true) end", "freeze.lua")
            .expect("script should load");
        instance.execute().expect("freeze run should complete");
        assert_eq!(*process.0.lock().expect("process events lock"), [ProcessEvent::Suspend(42)]);
        instance.shutdown();
        assert_eq!(
            *process.0.lock().expect("process events lock"),
            [ProcessEvent::Suspend(42), ProcessEvent::Resume(42)]
        );
    }

    #[test]
    fn cleanup_failures_do_not_replace_primary_execute_error() {
        let (mut instance, _, _, _) = instance_with_services();
        instance
            .load_source(
                b"function onExecute() error('primary') end; function onCleanup() error('cleanup') end",
                "double-error.lua",
            )
            .expect("script should load");
        let error = instance.execute().expect_err("run should fail");
        assert!(error.message.contains("primary"));
        assert!(instance.diagnostics().last_error.contains("primary"));
        assert!(instance.diagnostics().last_cleanup_warning.contains("cleanup"));
    }

    #[test]
    fn binary_chunks_are_rejected_with_cpp_friendly_message() {
        let (mut instance, _, _, _) = instance_with_services();
        let error = instance
            .load_source(b"\x1bLua\x54\0\0\0", "binary.lua")
            .expect_err("binary chunk should fail text-only loader");
        assert_eq!(error.message, BINARY_CHUNK_ERROR);
    }

    #[test]
    fn top_level_settings_state_is_composed_before_chunk_execution() {
        let mut config = ScriptInstanceConfig::new("3.4.0");
        config.initial_ui_state = serde_json::json!({"answer":42});
        let mut instance = LuaScriptInstance::new(config, ScriptServices::default())
            .expect("instance should initialize");
        instance
            .load_source(b"seen=settings.answer; function onExecute() end", "settings.lua")
            .expect("script should load");
        assert_eq!(
            instance
                .vm()
                .eval::<i64>("return seen", Duration::from_secs(1))
                .expect("top-level settings value should return"),
            42
        );
    }
}

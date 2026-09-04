//! Imported-script manager with per-script worker ownership.
//!
//! This preserves C++ `ScriptManager` import/reload/reset/persistence behavior while keeping each
//! Lua VM behind its own mutex. Script execution can run concurrently across records; a record has
//! at most one active execution worker and exposes a cancellation handle that never waits for the
//! Lua mutex.

use crate::instance::{
    LuaScriptInstance, ScriptInstanceConfig, ScriptInstanceDiagnostics, ScriptServices,
};
use crate::metadata::{
    ImportedScriptMetadata, is_supported_script_extension, parse_script_hotkey_metadata,
    parse_script_metadata,
};
use crate::persistence::{
    SCRIPT_UNBOUND_HOTKEY, SavedScriptRecord, deserialize_saved_scripts, normalize_script_hotkey,
    serialize_saved_scripts,
};
use crate::{LuaStopHandle, ScriptInputContext, ScriptProcessContext, ScriptSettingsUiContext};
use serde_json::Value as JsonValue;
use smu_core::Hotkey;
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const RELOAD_STOP_TIMEOUT: Duration = Duration::from_secs(5);
const MANAGER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(35);
const STOP_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScriptImportResult {
    Success,
    UnsupportedExtension,
    AlreadyImported,
    LoadFailed,
}

#[derive(Clone, Debug)]
pub struct ScriptManagerConfig {
    pub local_version: String,
    pub process_selector: String,
    pub take_all_process_ids: bool,
}

impl ScriptManagerConfig {
    #[must_use]
    pub fn new(local_version: impl Into<String>) -> Self {
        Self {
            local_version: local_version.into(),
            process_selector: String::new(),
            take_all_process_ids: false,
        }
    }
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScriptRecordSnapshot {
    pub path: PathBuf,
    pub metadata: ImportedScriptMetadata,
    pub hotkey: Hotkey,
    pub enabled: bool,
    pub disable_outside_roblox: bool,
    pub loaded: bool,
    pub missing: bool,
    pub running: bool,
    pub last_error: String,
    pub last_warning: String,
    pub ui_state: JsonValue,
}

pub struct ScriptRecord {
    path: PathBuf,
    metadata: Mutex<ImportedScriptMetadata>,
    hotkey: AtomicU32,
    enabled: AtomicBool,
    disable_outside_roblox: AtomicBool,
    loaded: AtomicBool,
    missing: AtomicBool,
    active: AtomicBool,
    running: AtomicBool,
    last_error: Mutex<String>,
    last_warning: Mutex<String>,
    ui_state: Mutex<JsonValue>,
    instance: Mutex<Option<LuaScriptInstance>>,
    stop_handle: Mutex<Option<LuaStopHandle>>,
    execution_context: Mutex<Option<ScriptInputContext>>,
    process_context: Mutex<Option<ScriptProcessContext>>,
    settings_context: Mutex<Option<ScriptSettingsUiContext>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for ScriptRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScriptRecord")
            .field("path", &self.path)
            .field("hotkey", &self.hotkey.load(Ordering::Acquire))
            .field("enabled", &self.enabled.load(Ordering::Acquire))
            .field("disable_outside_roblox", &self.disable_outside_roblox.load(Ordering::Acquire))
            .field("loaded", &self.loaded.load(Ordering::Acquire))
            .field("missing", &self.missing.load(Ordering::Acquire))
            .field("active", &self.active.load(Ordering::Acquire))
            .field("running", &self.running.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl ScriptRecord {
    fn new(path: PathBuf, hotkey: Hotkey, enabled: bool, disable_outside_roblox: bool) -> Self {
        Self {
            path,
            metadata: Mutex::new(ImportedScriptMetadata::default()),
            hotkey: AtomicU32::new(hotkey.raw()),
            enabled: AtomicBool::new(enabled),
            disable_outside_roblox: AtomicBool::new(disable_outside_roblox),
            loaded: AtomicBool::new(false),
            missing: AtomicBool::new(false),
            active: AtomicBool::new(false),
            running: AtomicBool::new(false),
            last_error: Mutex::new(String::new()),
            last_warning: Mutex::new(String::new()),
            ui_state: Mutex::new(JsonValue::Object(serde_json::Map::new())),
            instance: Mutex::new(None),
            stop_handle: Mutex::new(None),
            execution_context: Mutex::new(None),
            process_context: Mutex::new(None),
            settings_context: Mutex::new(None),
            worker: Mutex::new(None),
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn is_loaded(&self) -> bool {
        self.loaded.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn is_missing(&self) -> bool {
        self.missing.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn hotkey(&self) -> Hotkey {
        Hotkey::new(self.hotkey.load(Ordering::Acquire))
    }

    #[must_use]
    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn disable_outside_roblox(&self) -> bool {
        self.disable_outside_roblox.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn metadata(&self) -> ImportedScriptMetadata {
        lock(&self.metadata).clone()
    }

    #[must_use]
    pub fn ui_state(&self) -> JsonValue {
        let settings_context = lock(&self.settings_context).clone();
        if let Some(context) = settings_context {
            return context.ui_state();
        }
        lock(&self.ui_state).clone()
    }

    #[must_use]
    pub fn last_error(&self) -> String {
        lock(&self.last_error).clone()
    }

    #[must_use]
    pub fn last_warning(&self) -> String {
        lock(&self.last_warning).clone()
    }

    fn set_last_error(&self, value: impl Into<String>) {
        *lock(&self.last_error) = value.into();
    }

    fn clear_last_error(&self) {
        lock(&self.last_error).clear();
    }

    fn clear_last_warning(&self) {
        lock(&self.last_warning).clear();
    }

    #[must_use]
    pub fn snapshot(&self) -> ScriptRecordSnapshot {
        ScriptRecordSnapshot {
            path: self.path.clone(),
            metadata: self.metadata(),
            hotkey: self.hotkey(),
            enabled: self.enabled(),
            disable_outside_roblox: self.disable_outside_roblox(),
            loaded: self.is_loaded(),
            missing: self.is_missing(),
            running: self.is_running(),
            last_error: self.last_error(),
            last_warning: self.last_warning(),
            ui_state: self.ui_state(),
        }
    }

    #[must_use]
    pub fn settings_context(&self) -> Option<ScriptSettingsUiContext> {
        lock(&self.settings_context).clone()
    }
}

pub struct ScriptManager {
    config: ScriptManagerConfig,
    services: ScriptServices,
    process_target: Mutex<(String, bool)>,
    scripts: Mutex<Vec<Arc<ScriptRecord>>>,
}

impl fmt::Debug for ScriptManager {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScriptManager")
            .field("config", &self.config)
            .field("services", &self.services)
            .field("count", &self.count())
            .finish_non_exhaustive()
    }
}

struct ActiveGuard<'a> {
    active: &'a AtomicBool,
    running: &'a AtomicBool,
}

impl Drop for ActiveGuard<'_> {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        self.active.store(false, Ordering::Release);
    }
}

impl ScriptManager {
    #[must_use]
    pub fn new(config: ScriptManagerConfig, services: ScriptServices) -> Self {
        let process_target =
            Mutex::new((config.process_selector.clone(), config.take_all_process_ids));
        Self { config, services, process_target, scripts: Mutex::new(Vec::new()) }
    }

    #[must_use]
    pub fn count(&self) -> usize {
        lock(&self.scripts).len()
    }

    #[must_use]
    pub fn get(&self, index: usize) -> Option<Arc<ScriptRecord>> {
        lock(&self.scripts).get(index).cloned()
    }

    #[must_use]
    pub fn snapshot(&self) -> Vec<ScriptRecordSnapshot> {
        lock(&self.scripts).iter().map(|record| record.snapshot()).collect()
    }

    #[must_use]
    pub fn normalize_path(path: &Path) -> PathBuf {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
        };

        if let Ok(canonical) = std::fs::canonicalize(&absolute) {
            return canonical;
        }
        let lexical = lexical_normalize(&absolute);
        if let Ok(canonical) = std::fs::canonicalize(&lexical) {
            return canonical;
        }

        let mut probe = lexical.clone();
        let mut suffix = Vec::new();
        let canonical_prefix = loop {
            if let Ok(canonical) = std::fs::canonicalize(&probe) {
                break Some(canonical);
            }
            let Some(name) = probe.file_name().map(std::ffi::OsStr::to_os_string) else {
                break None;
            };
            suffix.push(name);
            if !probe.pop() {
                break None;
            }
        };
        let mut combined = canonical_prefix.unwrap_or(lexical);
        for component in suffix.into_iter().rev() {
            combined.push(component);
        }
        lexical_normalize(&combined)
    }

    fn is_duplicate_path_locked(records: &[Arc<ScriptRecord>], path: &Path) -> bool {
        let normalized = Self::normalize_path(path);
        records.iter().any(|record| Self::normalize_path(record.path()) == normalized)
    }

    pub fn import_script(&self, path: &Path) -> bool {
        self.import_script_with_result(path) == ScriptImportResult::Success
    }

    pub fn import_script_with_result(&self, path: &Path) -> ScriptImportResult {
        let normalized = Self::normalize_path(path);
        if !is_supported_script_extension(&normalized) {
            return ScriptImportResult::UnsupportedExtension;
        }

        let record = Arc::new(ScriptRecord::new(
            normalized,
            Hotkey::new(SCRIPT_UNBOUND_HOTKEY),
            false,
            true,
        ));
        {
            let mut scripts = lock(&self.scripts);
            if Self::is_duplicate_path_locked(&scripts, record.path()) {
                return ScriptImportResult::AlreadyImported;
            }
            scripts.push(Arc::clone(&record));
        }

        if !self.load_record(&record) {
            Self::shutdown_record(&record);
            lock(&self.scripts).retain(|candidate| !Arc::ptr_eq(candidate, &record));
            return ScriptImportResult::LoadFailed;
        }

        let hotkey = parse_script_hotkey_metadata(record.path())
            .unwrap_or_else(|| Hotkey::new(SCRIPT_UNBOUND_HOTKEY));
        Self::set_record_hotkey(&record, hotkey);
        ScriptImportResult::Success
    }

    pub fn import_script_from_save(
        &self,
        path: &Path,
        hotkey: Hotkey,
        enabled: bool,
        disable_outside_roblox: bool,
    ) -> bool {
        self.import_saved_record(path, hotkey, enabled, disable_outside_roblox).is_some()
    }

    fn import_saved_record(
        &self,
        path: &Path,
        hotkey: Hotkey,
        enabled: bool,
        disable_outside_roblox: bool,
    ) -> Option<Arc<ScriptRecord>> {
        let normalized = Self::normalize_path(path);
        let record = Arc::new(ScriptRecord::new(
            normalized,
            normalize_script_hotkey(hotkey.raw()),
            enabled,
            disable_outside_roblox,
        ));
        {
            let mut scripts = lock(&self.scripts);
            if Self::is_duplicate_path_locked(&scripts, record.path()) {
                return None;
            }
            scripts.push(Arc::clone(&record));
        }
        let _ = self.load_record(&record);
        Some(record)
    }

    fn load_record(&self, record: &Arc<ScriptRecord>) -> bool {
        if std::fs::metadata(record.path()).is_err() {
            *lock(&record.metadata) = ImportedScriptMetadata {
                name: record
                    .path()
                    .file_stem()
                    .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned()),
                ..ImportedScriptMetadata::default()
            };
            record.missing.store(true, Ordering::Release);
            record.loaded.store(false, Ordering::Release);
            record.set_last_error("Script file is missing.");
            return false;
        }
        if !is_supported_script_extension(record.path()) {
            *lock(&record.metadata) = ImportedScriptMetadata {
                name: record
                    .path()
                    .file_stem()
                    .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned()),
                ..ImportedScriptMetadata::default()
            };
            record.loaded.store(false, Ordering::Release);
            record.set_last_error("Unsupported script extension.");
            return false;
        }

        let metadata = parse_script_metadata(record.path());
        *lock(&record.metadata) = metadata.clone();
        let (process_selector, take_all_process_ids) = lock(&self.process_target).clone();
        let mut config = ScriptInstanceConfig::new(self.config.local_version.clone());
        config.memory_limit_mb = metadata.memory_limit_mb;
        config.initial_ui_state = record.ui_state();
        config.process_selector = process_selector;
        config.take_all_process_ids = take_all_process_ids;
        config.script_hotkey = record.hotkey();

        let mut instance = match LuaScriptInstance::new(config, self.services.clone()) {
            Ok(instance) => instance,
            Err(error) => {
                record.loaded.store(false, Ordering::Release);
                record.set_last_error(error.message);
                return false;
            }
        };
        let stop_handle = instance.stop_handle();
        let execution_context = instance.execution_context_handle();
        let process_context = instance.process_context_handle();
        let settings_context = instance.settings_ui_context_handle();
        let loaded = instance.load_file(record.path()).is_ok();
        Self::sync_record_diagnostics(record, instance.diagnostics());
        *lock(&record.ui_state) = instance.ui_state();
        *lock(&record.stop_handle) = Some(stop_handle);
        *lock(&record.execution_context) = Some(execution_context);
        *lock(&record.process_context) = Some(process_context);
        *lock(&record.settings_context) = Some(settings_context);
        *lock(&record.instance) = Some(instance);
        record.missing.store(false, Ordering::Release);
        record.loaded.store(loaded, Ordering::Release);
        loaded
    }

    fn sync_record_diagnostics(
        record: &Arc<ScriptRecord>,
        diagnostics: &ScriptInstanceDiagnostics,
    ) {
        lock(&record.last_error).clone_from(&diagnostics.last_error);
        lock(&record.last_warning).clone_from(&diagnostics.last_warning);
    }

    fn set_record_hotkey(record: &Arc<ScriptRecord>, hotkey: Hotkey) {
        let hotkey = normalize_script_hotkey(hotkey.raw());
        record.hotkey.store(hotkey.raw(), Ordering::Release);
        if let Some(context) = lock(&record.execution_context).as_ref() {
            context.set_script_hotkey(hotkey);
        }
    }

    pub fn set_hotkey(&self, index: usize, hotkey: Hotkey) -> bool {
        let Some(record) = self.get(index) else {
            return false;
        };
        Self::set_record_hotkey(&record, hotkey);
        true
    }

    pub fn set_enabled(&self, index: usize, enabled: bool) -> bool {
        let Some(record) = self.get(index) else {
            return false;
        };
        record.enabled.store(enabled, Ordering::Release);
        true
    }

    pub fn set_disable_outside_roblox(&self, index: usize, disabled: bool) -> bool {
        let Some(record) = self.get(index) else {
            return false;
        };
        record.disable_outside_roblox.store(disabled, Ordering::Release);
        true
    }

    pub fn set_process_target(&self, selector: impl Into<String>, take_all_process_ids: bool) {
        let selector = selector.into();
        *lock(&self.process_target) = (selector.clone(), take_all_process_ids);
        let records = lock(&self.scripts).clone();
        for record in records {
            if let Some(context) = lock(&record.process_context).as_ref() {
                context.set_target_selector(selector.clone(), take_all_process_ids);
            }
        }
    }

    pub fn reload_script(&self, index: usize) -> bool {
        let Some(record) = self.get(index) else {
            return false;
        };
        if !Self::stop_and_wait(&record, RELOAD_STOP_TIMEOUT) {
            return false;
        }
        if record.active.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err()
        {
            return false;
        }
        record.running.store(true, Ordering::Release);
        let _active = ActiveGuard { active: &record.active, running: &record.running };
        Self::join_finished_worker(&record);

        let preserved_hotkey = record.hotkey();
        let preserved_enabled = record.enabled();
        let preserved_disable_outside = record.disable_outside_roblox();
        Self::shutdown_instance(&record);
        record.loaded.store(false, Ordering::Release);
        record.missing.store(false, Ordering::Release);
        record.clear_last_error();

        let loaded = self.load_record(&record);
        if let Some(hotkey) = parse_script_hotkey_metadata(record.path()) {
            Self::set_record_hotkey(&record, hotkey);
        } else {
            Self::set_record_hotkey(&record, preserved_hotkey);
        }
        record.enabled.store(preserved_enabled, Ordering::Release);
        record.disable_outside_roblox.store(preserved_disable_outside, Ordering::Release);
        loaded
    }

    pub fn reset_script(&self, index: usize) -> bool {
        let Some(record) = self.get(index) else {
            return false;
        };
        if !Self::stop_and_wait(&record, RELOAD_STOP_TIMEOUT) {
            return false;
        }
        if record.active.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err()
        {
            return false;
        }
        record.running.store(true, Ordering::Release);
        let _active = ActiveGuard { active: &record.active, running: &record.running };
        Self::join_finished_worker(&record);

        *lock(&record.ui_state) = JsonValue::Object(serde_json::Map::new());
        Self::shutdown_instance(&record);
        record.loaded.store(false, Ordering::Release);
        record.missing.store(false, Ordering::Release);
        record.clear_last_error();
        record.clear_last_warning();
        record.enabled.store(false, Ordering::Release);
        record.disable_outside_roblox.store(true, Ordering::Release);

        let loaded = self.load_record(&record);
        let hotkey = parse_script_hotkey_metadata(record.path())
            .unwrap_or_else(|| Hotkey::new(SCRIPT_UNBOUND_HOTKEY));
        Self::set_record_hotkey(&record, hotkey);
        loaded
    }

    fn begin_execute(record: &Arc<ScriptRecord>) -> bool {
        if record.is_missing() || !record.is_loaded() {
            return false;
        }
        if record.active.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err()
        {
            return false;
        }
        if lock(&record.instance).is_none() {
            record.active.store(false, Ordering::Release);
            return false;
        }
        true
    }

    fn execute_record(record: &Arc<ScriptRecord>) -> bool {
        let _active = ActiveGuard { active: &record.active, running: &record.running };
        let (result, diagnostics) = {
            let mut instance_guard = lock(&record.instance);
            let Some(instance) = instance_guard.as_mut() else {
                return false;
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                instance.execute_with_started(|| record.running.store(true, Ordering::Release))
            }));
            let diagnostics = instance.diagnostics().clone();
            drop(instance_guard);
            (result, diagnostics)
        };
        Self::sync_record_diagnostics(record, &diagnostics);
        result.map_or_else(
            |_| {
                record.set_last_error("Script execution failed with an unknown Rust panic.");
                false
            },
            |result| result.is_ok(),
        )
    }

    pub fn execute_script(&self, index: usize) -> bool {
        let Some(record) = self.get(index) else {
            return false;
        };
        if !Self::begin_execute(&record) {
            return false;
        }
        Self::execute_record(&record)
    }

    /// Launch one named script worker. Different records can execute concurrently, while a second
    /// launch for the same record is rejected until its existing worker finishes.
    pub fn spawn_execute_script(&self, index: usize) -> bool {
        let Some(record) = self.get(index) else {
            return false;
        };
        Self::join_finished_worker(&record);
        if !Self::begin_execute(&record) {
            return false;
        }
        let worker_record = Arc::clone(&record);
        let worker = std::thread::spawn(move || {
            let _ = Self::execute_record(&worker_record);
        });
        *lock(&record.worker) = Some(worker);
        true
    }

    pub fn force_stop_script(&self, index: usize) -> bool {
        let Some(record) = self.get(index) else {
            return false;
        };
        if !record.is_running() {
            return false;
        }
        let handle = lock(&record.stop_handle).clone();
        let Some(handle) = handle else {
            return false;
        };
        handle.request_cancel();
        true
    }

    pub fn call_settings(&self, index: usize, render_mode: bool) -> bool {
        let Some(record) = self.get(index) else {
            return false;
        };
        let (result, diagnostics, ui_state) = {
            let mut instance_guard = if render_mode {
                match record.instance.try_lock() {
                    Ok(guard) => guard,
                    Err(TryLockError::WouldBlock) => return true,
                    Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
                }
            } else {
                lock(&record.instance)
            };
            let Some(instance) = instance_guard.as_mut() else {
                record.set_last_error("Script is not loaded.");
                return false;
            };
            let result = instance.call_settings(render_mode);
            let diagnostics = instance.diagnostics().clone();
            let ui_state = instance.ui_state();
            drop(instance_guard);
            (result, diagnostics, ui_state)
        };
        Self::sync_record_diagnostics(&record, &diagnostics);
        *lock(&record.ui_state) = ui_state;
        result.is_ok()
    }

    #[must_use]
    pub fn settings_controls(&self, index: usize) -> Vec<crate::SettingsUiControl> {
        self.get(index)
            .and_then(|record| record.settings_context())
            .map_or_else(Vec::new, |context| context.controls())
    }

    pub fn queue_settings_value(
        &self,
        index: usize,
        id: impl Into<String>,
        value: JsonValue,
    ) -> bool {
        let Some(context) = self.get(index).and_then(|record| record.settings_context()) else {
            return false;
        };
        context.queue_value(id, value);
        true
    }

    pub fn queue_settings_button(&self, index: usize, id: impl Into<String>) -> bool {
        let Some(context) = self.get(index).and_then(|record| record.settings_context()) else {
            return false;
        };
        context.queue_button_press(id);
        true
    }

    pub fn remove_script(&self, index: usize) -> bool {
        let Some(record) = self.get(index) else {
            return false;
        };
        if !Self::stop_and_wait(&record, RELOAD_STOP_TIMEOUT) {
            return false;
        }
        {
            let mut scripts = lock(&self.scripts);
            let Some(position) =
                scripts.iter().position(|candidate| Arc::ptr_eq(candidate, &record))
            else {
                return false;
            };
            scripts.remove(position);
        }
        Self::join_worker(&record);
        Self::shutdown_record(&record);
        true
    }

    fn request_cancel(record: &Arc<ScriptRecord>) -> bool {
        let handle = lock(&record.stop_handle).clone();
        let Some(handle) = handle else {
            return false;
        };
        handle.request_cancel();
        true
    }

    fn stop_and_wait(record: &Arc<ScriptRecord>, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while record.is_active() {
            if record.is_running() {
                let _ = Self::request_cancel(record);
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(STOP_POLL_INTERVAL);
        }
        true
    }

    fn join_finished_worker(record: &Arc<ScriptRecord>) {
        let worker = {
            let mut slot = lock(&record.worker);
            if slot.as_ref().is_some_and(JoinHandle::is_finished) { slot.take() } else { None }
        };
        if let Some(worker) = worker {
            let _ = worker.join();
        }
    }

    fn join_worker(record: &Arc<ScriptRecord>) {
        let worker = lock(&record.worker).take();
        if let Some(worker) = worker {
            let _ = worker.join();
        }
    }

    fn shutdown_instance(record: &Arc<ScriptRecord>) {
        let mut instance = lock(&record.instance).take();
        if let Some(instance) = instance.as_mut() {
            instance.shutdown();
        }
        *lock(&record.stop_handle) = None;
        *lock(&record.execution_context) = None;
        *lock(&record.process_context) = None;
        *lock(&record.settings_context) = None;
    }

    fn shutdown_record(record: &Arc<ScriptRecord>) {
        Self::shutdown_instance(record);
        record.loaded.store(false, Ordering::Release);
    }

    #[must_use]
    pub fn serialize(&self) -> JsonValue {
        let records = lock(&self.scripts)
            .iter()
            .map(|record| SavedScriptRecord {
                path: record.path.clone(),
                hotkey: record.hotkey(),
                enabled: record.enabled(),
                disable_outside_roblox: record.disable_outside_roblox(),
                ui_state: record.ui_state(),
            })
            .collect::<Vec<_>>();
        serialize_saved_scripts(&records)
    }

    pub fn deserialize(&self, value: &JsonValue) {
        self.clear();
        for saved in deserialize_saved_scripts(value) {
            let Some(record) = self.import_saved_record(
                &saved.path,
                saved.hotkey,
                saved.enabled,
                saved.disable_outside_roblox,
            ) else {
                continue;
            };
            *lock(&record.ui_state) = saved.ui_state.clone();
            let mut instance_guard = lock(&record.instance);
            if let Some(instance) = instance_guard.as_mut()
                && let Err(error) = instance.replace_ui_state(&saved.ui_state)
            {
                record.set_last_error(error.message);
            }
            drop(instance_guard);
        }
    }

    /// Cancel all records and wait up to the C++ manager's 35-second shutdown window. Records that
    /// remain running are intentionally kept in the manager until their workers finish.
    pub fn clear(&self) {
        let records = lock(&self.scripts).clone();
        let deadline = Instant::now() + MANAGER_SHUTDOWN_TIMEOUT;
        while Instant::now() < deadline && records.iter().any(|record| record.is_active()) {
            for record in &records {
                if record.is_running() {
                    let _ = Self::request_cancel(record);
                }
            }
            std::thread::sleep(STOP_POLL_INTERVAL);
        }

        let mut to_cleanup = Vec::new();
        {
            let mut scripts = lock(&self.scripts);
            if scripts.iter().any(|record| record.is_active()) {
                let mut index = 0;
                while index < scripts.len() {
                    if scripts[index].is_active() {
                        index += 1;
                    } else {
                        to_cleanup.push(scripts.remove(index));
                    }
                }
            } else {
                to_cleanup.append(&mut *scripts);
            }
        }
        for record in to_cleanup {
            Self::join_worker(&record);
            Self::shutdown_record(&record);
        }
    }

    pub fn shutdown(&self) {
        self.clear();
    }
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = normalized.pop();
            }
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(Path::new(std::path::MAIN_SEPARATOR_STR)),
            Component::Normal(value) => normalized.push(value),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("smu-manager-{name}-{nonce}"));
        fs::create_dir_all(&path).expect("temp directory should be created");
        path
    }

    fn write_script(directory: &Path, name: &str, source: &str) -> PathBuf {
        let path = directory.join(name);
        fs::write(&path, source).expect("script should be written");
        path
    }

    fn manager() -> ScriptManager {
        ScriptManager::new(ScriptManagerConfig::new("3.4.0"), ScriptServices::default())
    }

    fn wait_until(predicate: impl Fn() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if predicate() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        false
    }

    #[test]
    fn interactive_import_loads_metadata_and_applies_metadata_hotkey_after_load() {
        let directory = temp_dir("import");
        let path = write_script(
            &directory,
            "test.lua",
            concat!(
                "-- @name: Example\n-- @keybind: Ctrl+A\n",
                "seen=getScriptHotkey(); function onExecute() end"
            ),
        );
        let manager = manager();
        assert_eq!(manager.import_script_with_result(&path), ScriptImportResult::Success);
        let record = manager.get(0).expect("record should exist");
        assert_eq!(record.metadata().name, "Example");
        assert_eq!(record.hotkey().raw(), 0x2_0041);
        assert!(record.is_loaded());
        let (seen_unbound, current_hotkey) = {
            let instance = lock(&record.instance);
            let vm = instance.as_ref().expect("instance should exist").vm();
            let seen_unbound = vm
                .eval::<bool>("return seen == nil", Duration::from_secs(1))
                .expect("top-level hotkey state should return");
            let current_hotkey = vm
                .eval::<u32>("return getScriptHotkey()", Duration::from_secs(1))
                .expect("post-import hotkey should return");
            drop(instance);
            (seen_unbound, current_hotkey)
        };
        assert!(seen_unbound);
        assert_eq!(current_hotkey, 0x2_0041);
        manager.shutdown();
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn duplicate_and_unsupported_import_results_match_cpp() {
        let directory = temp_dir("duplicate");
        let path = write_script(&directory, "a.lua", "function onExecute() end");
        let unsupported = write_script(&directory, "a.json", "{}");
        let manager = manager();
        assert_eq!(
            manager.import_script_with_result(&unsupported),
            ScriptImportResult::UnsupportedExtension
        );
        assert_eq!(manager.import_script_with_result(&path), ScriptImportResult::Success);
        let alias = directory.join("sub").join("..").join("a.lua");
        assert_eq!(manager.import_script_with_result(&alias), ScriptImportResult::AlreadyImported);
        manager.shutdown();
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn failed_interactive_import_is_removed_but_saved_import_is_retained() {
        let directory = temp_dir("failed");
        let bad = write_script(&directory, "bad.lua", "this is not lua !!!");
        let missing = directory.join("missing.lua");
        let manager = manager();
        assert_eq!(manager.import_script_with_result(&bad), ScriptImportResult::LoadFailed);
        assert_eq!(manager.count(), 0);
        assert!(manager.import_script_from_save(
            &missing,
            Hotkey::new(SCRIPT_UNBOUND_HOTKEY),
            true,
            false,
        ));
        let record = manager.get(0).expect("saved missing record should remain");
        assert!(record.is_missing());
        assert!(!record.is_loaded());
        assert_eq!(record.last_error(), "Script file is missing.");
        manager.shutdown();
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn record_snapshot_prefers_live_settings_context_state() {
        let directory = temp_dir("live-ui-state");
        let path = write_script(
            &directory,
            "dynamic.lua",
            concat!(
                "function onSettings() ui.dynamicTextbox('status','Status','initial') end
",
                "function onExecute() end"
            ),
        );
        let manager = manager();
        assert!(manager.import_script(&path));
        assert!(manager.call_settings(0, false));
        let record = manager.get(0).expect("record should exist");
        let context = record.settings_context().expect("settings context should exist");
        context.set_stored("status", serde_json::json!("live"));
        assert_eq!(record.snapshot().ui_state["status"], serde_json::json!("live"));
        assert_eq!(manager.serialize()[0]["ui_state"]["status"], serde_json::json!("live"));
        manager.shutdown();
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn deserialize_restores_saved_ui_state_after_initial_default_settings_call() {
        let directory = temp_dir("deserialize");
        let path = write_script(
            &directory,
            "settings.lua",
            concat!(
                "function onSettings() ui.checkbox('flag','Flag',false) end\n",
                "function onExecute() end"
            ),
        );
        let manager = manager();
        manager.deserialize(&serde_json::json!([{
            "path": path.to_string_lossy(),
            "hotkey": 131_137,
            "enabled": true,
            "disable_outside_roblox": false,
            "ui_state": {"flag": true, "text": "saved"}
        }]));
        let record = manager.get(0).expect("record should deserialize");
        assert_eq!(record.ui_state(), serde_json::json!({"flag":true,"text":"saved"}));
        assert_eq!(record.hotkey().raw(), 131_137);
        assert!(record.enabled());
        assert!(!record.disable_outside_roblox());
        manager.shutdown();
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn reload_preserves_flags_and_uses_new_metadata_hotkey_when_present() {
        let directory = temp_dir("reload");
        let path = write_script(&directory, "reload.lua", "function onExecute() end");
        let manager = manager();
        assert!(manager.import_script(&path));
        assert!(manager.set_hotkey(0, Hotkey::new(0x2_0041)));
        assert!(manager.set_enabled(0, true));
        assert!(manager.set_disable_outside_roblox(0, false));
        fs::write(&path, "-- @keybind: Shift+B\nfunction onExecute() end")
            .expect("script should be replaced");
        assert!(manager.reload_script(0));
        let record = manager.get(0).expect("record should remain");
        assert_eq!(record.hotkey().raw(), 0x1_0042);
        assert!(record.enabled());
        assert!(!record.disable_outside_roblox());
        manager.shutdown();
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn reset_clears_ui_and_flags_then_reapplies_metadata_hotkey() {
        let directory = temp_dir("reset");
        let path = write_script(
            &directory,
            "reset.lua",
            concat!(
                "-- @keybind: Ctrl+C\n",
                "function onSettings() ui.checkbox('flag','Flag',true) end\n",
                "function onExecute() end"
            ),
        );
        let manager = manager();
        assert!(manager.import_script(&path));
        assert!(manager.set_enabled(0, true));
        assert!(manager.set_disable_outside_roblox(0, false));
        let context = manager
            .get(0)
            .and_then(|record| record.settings_context())
            .expect("settings context should exist");
        context.queue_value("flag", serde_json::json!(false));
        assert!(manager.call_settings(0, true));
        assert_eq!(manager.get(0).expect("record").ui_state(), serde_json::json!({"flag":false}));

        assert!(manager.reset_script(0));
        let record = manager.get(0).expect("record should remain");
        assert!(!record.enabled());
        assert!(record.disable_outside_roblox());
        assert_eq!(record.hotkey().raw(), 0x2_0043);
        assert_eq!(record.ui_state(), serde_json::json!({"flag":true}));
        manager.shutdown();
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn separate_script_workers_can_run_simultaneously_and_same_record_cannot_overlap() {
        let directory = temp_dir("concurrency");
        let first = write_script(&directory, "first.lua", "function onExecute() sleep(1000) end");
        let second = write_script(&directory, "second.lua", "function onExecute() sleep(1000) end");
        let manager = manager();
        assert!(manager.import_script(&first));
        assert!(manager.import_script(&second));
        assert!(manager.spawn_execute_script(0));
        assert!(manager.spawn_execute_script(1));
        assert!(wait_until(|| {
            manager.get(0).is_some_and(|record| record.is_running())
                && manager.get(1).is_some_and(|record| record.is_running())
        }));
        assert!(!manager.spawn_execute_script(0));
        assert!(wait_until(|| {
            manager.get(0).is_some_and(|record| !record.is_running())
                && manager.get(1).is_some_and(|record| !record.is_running())
        }));
        manager.shutdown();
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn force_stop_cancels_worker_without_waiting_for_instance_mutex() {
        let directory = temp_dir("stop");
        let path = write_script(&directory, "stop.lua", "function onExecute() sleep(5000) end");
        let manager = manager();
        assert!(manager.import_script(&path));
        assert!(manager.spawn_execute_script(0));
        assert!(wait_until(|| manager.get(0).is_some_and(|record| record.is_running())));
        assert!(manager.force_stop_script(0));
        assert!(wait_until(|| manager.get(0).is_some_and(|record| !record.is_running())));
        assert!(manager.get(0).expect("record should remain").last_error().is_empty());
        manager.shutdown();
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn remove_running_script_requests_cancel_and_joins_worker() {
        let directory = temp_dir("remove");
        let path = write_script(&directory, "remove.lua", "function onExecute() sleep(5000) end");
        let manager = manager();
        assert!(manager.import_script(&path));
        assert!(manager.spawn_execute_script(0));
        assert!(wait_until(|| manager.get(0).is_some_and(|record| record.is_running())));
        assert!(manager.remove_script(0));
        assert_eq!(manager.count(), 0);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn serialize_matches_saved_record_shape() {
        let directory = temp_dir("serialize");
        let path = write_script(&directory, "save.lua", "function onExecute() end");
        let manager = manager();
        assert!(manager.import_script(&path));
        assert!(manager.set_hotkey(0, Hotkey::new(0x2_0041)));
        assert!(manager.set_enabled(0, true));
        assert_eq!(
            manager.serialize(),
            serde_json::json!([{
                "path": ScriptManager::normalize_path(&path).to_string_lossy(),
                "hotkey": 131_137,
                "enabled": true,
                "disable_outside_roblox": true,
                "ui_state": {}
            }])
        );
        manager.shutdown();
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn live_hotkey_and_process_target_handles_update_without_lua_mutex() {
        let directory = temp_dir("live-context");
        let path = write_script(&directory, "live.lua", "function onExecute() sleep(100) end");
        let manager = manager();
        assert!(manager.import_script(&path));
        let record = manager.get(0).expect("record should exist");
        assert!(manager.set_hotkey(0, Hotkey::new(0x2_0041)));
        assert_eq!(
            lock(&record.execution_context)
                .as_ref()
                .expect("execution context")
                .script_hotkey()
                .raw(),
            0x2_0041
        );
        manager.set_process_target("new-target", true);
        let process = lock(&record.process_context).as_ref().expect("process context").clone();
        assert_eq!(process.target_selector(), "new-target");
        assert!(process.take_all_process_ids());
        manager.shutdown();
        let _ = fs::remove_dir_all(directory);
    }
}

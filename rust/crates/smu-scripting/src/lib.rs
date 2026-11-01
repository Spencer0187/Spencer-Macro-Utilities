//! Compatibility-facing pieces of SMU's imported-script system.
//!
//! Lua execution is still being ported. Deterministic metadata/hotkey parsing is isolated here
//! because it can be differential-tested against the C++ compatibility oracle.

pub mod core_api;
pub mod display_api;
pub mod hotkey;
pub mod input_api;
pub mod input_callbacks;
pub mod instance;
pub mod lua_runtime;
pub mod managed_hotkeys;
pub mod manager;
pub mod memory;
pub mod metadata;
pub mod network_api;
pub mod persistence;
pub mod process_api;
pub mod roblox_log_api;
pub mod settings_ui;
pub mod settings_ui_api;

pub use core_api::{
    MAX_SCRIPT_LOG_MESSAGE_BYTES, ScriptCoreApiError, ScriptEnvironment, ScriptLogSink,
    install_core_api, script_platform_name,
};
pub use display_api::{
    MoveDegreesCalibration, ScriptDisplayApiError, cache_move_degrees_calibration,
    install_display_api, install_move_degrees_api,
};
pub use hotkey::{parse_script_hotkey, parse_script_key_name};
pub use input_api::{
    ScriptExecutionMode, ScriptInputApiError, ScriptInputContext, ScriptMouseMotionMode,
    install_input_api,
};
pub use input_callbacks::{
    DEFAULT_INPUT_CALLBACK_SCAN_MS, HotkeyCallbackKind, MAX_INPUT_CALLBACK_SCAN_MS,
    MAX_INPUT_CALLBACKS, MIN_INPUT_CALLBACK_SCAN_MS, ScriptInputCallbackApiError,
    ScriptInputCallbackContext, install_input_callback_api,
};
pub use instance::{
    LuaScriptInstance, RobloxLogSourceFactory, ScriptCleanupReason, ScriptInstanceConfig,
    ScriptInstanceDiagnostics, ScriptLoadError, ScriptServices,
};
pub use lua_runtime::{
    DEFAULT_SHOULD_YIELD_MICROS, LUA_HOOK_INSTRUCTION_COUNT, LuaExecutionError,
    LuaExecutionErrorKind, LuaScriptVm, LuaStopHandle, MAX_CLEANUP_RUNTIME, MAX_SCRIPT_RUNTIME,
    MAX_SETTINGS_RUNTIME, MAX_SINGLE_SLEEP_MICROS, ScriptStopReason,
};
pub use managed_hotkeys::{ManagedHotkeyResult, ManagedHotkeys, ManagedKeyAction};
pub use manager::{
    ScriptHotkeySnapshot, ScriptImportResult, ScriptManager, ScriptManagerConfig, ScriptRecord,
    ScriptRecordSnapshot,
};
pub use memory::{
    DEFAULT_SCRIPT_MEMORY_LIMIT_MB, LuaMemoryBudget, MAX_SCRIPT_MEMORY_LIMIT_MB,
    MIN_SCRIPT_MEMORY_LIMIT_MB, memory_limit_bytes,
};
pub use metadata::{
    ImportedScriptHeader, ImportedScriptMetadata, is_supported_script_extension,
    parse_script_header, parse_script_hotkey_metadata, parse_script_metadata,
};
pub use network_api::{ScriptNetworkApiError, install_network_api};
pub use persistence::{
    MAX_UI_ID_BYTES, MAX_UI_STATE_ENTRIES, MAX_UI_STATE_STRING_BYTES, SCRIPT_UNBOUND_HOTKEY,
    SavedScriptRecord, deserialize_saved_scripts, is_safe_ui_state_key, normalize_script_hotkey,
    sanitize_ui_state, serialize_saved_scripts,
};
pub use process_api::{ScriptProcessApiError, ScriptProcessContext, install_process_api};
pub use roblox_log_api::{ScriptRobloxLogApiError, install_roblox_log_api};

pub use settings_ui::{
    MAX_UI_CONTROLS_PER_SETTINGS_CALL, MAX_UI_DIMENSION, MAX_UI_DROPDOWN_ITEMS,
    ScriptSettingsUiContext, ScriptUiCache, SettingsUiCapture, SettingsUiControl,
    SettingsUiControlKind,
};

pub use settings_ui_api::{
    ScriptSettingsUiApiError, call_on_settings, install_settings_ui_api,
    sync_ui_state_from_lua_settings,
};

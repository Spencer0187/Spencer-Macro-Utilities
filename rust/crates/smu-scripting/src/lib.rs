//! Compatibility-facing pieces of SMU's imported-script system.
//!
//! Lua execution is still being ported. Deterministic metadata/hotkey parsing is isolated here
//! because it can be differential-tested against the C++ compatibility oracle.

pub mod hotkey;
pub mod managed_hotkeys;
pub mod memory;
pub mod metadata;
pub mod persistence;
pub mod settings_ui;

pub use hotkey::{parse_script_hotkey, parse_script_key_name};
pub use managed_hotkeys::{ManagedHotkeyResult, ManagedHotkeys, ManagedKeyAction};
pub use memory::{
    DEFAULT_SCRIPT_MEMORY_LIMIT_MB, LuaMemoryBudget, MAX_SCRIPT_MEMORY_LIMIT_MB,
    MIN_SCRIPT_MEMORY_LIMIT_MB, memory_limit_bytes,
};
pub use metadata::{
    ImportedScriptHeader, ImportedScriptMetadata, is_supported_script_extension,
    parse_script_header, parse_script_hotkey_metadata, parse_script_metadata,
};
pub use persistence::{
    MAX_UI_ID_BYTES, MAX_UI_STATE_ENTRIES, MAX_UI_STATE_STRING_BYTES, SCRIPT_UNBOUND_HOTKEY,
    SavedScriptRecord, deserialize_saved_scripts, is_safe_ui_state_key, normalize_script_hotkey,
    sanitize_ui_state, serialize_saved_scripts,
};

pub use settings_ui::{
    MAX_UI_CONTROLS_PER_SETTINGS_CALL, MAX_UI_DROPDOWN_ITEMS, ScriptUiCache, SettingsUiCapture,
    SettingsUiControl, SettingsUiControlKind,
};

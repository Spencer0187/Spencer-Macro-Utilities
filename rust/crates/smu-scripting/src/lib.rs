//! Compatibility-facing pieces of SMU's imported-script system.
//!
//! Lua execution is still being ported. Deterministic metadata/hotkey parsing is isolated here
//! because it can be differential-tested against the C++ compatibility oracle.

pub mod hotkey;
pub mod metadata;

pub use hotkey::{parse_script_hotkey, parse_script_key_name};
pub use metadata::{
    ImportedScriptHeader, ImportedScriptMetadata, is_supported_script_extension,
    parse_script_header, parse_script_hotkey_metadata, parse_script_metadata,
};

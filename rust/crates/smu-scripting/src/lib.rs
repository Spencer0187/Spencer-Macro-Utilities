//! Compatibility-facing pieces of SMU's imported-script system.
//!
//! Lua execution is intentionally not implemented yet. This crate starts with deterministic
//! metadata/hotkey parsing because it can be differential-tested against the C++ oracle.

pub mod hotkey;

pub use hotkey::{parse_script_hotkey, parse_script_key_name};

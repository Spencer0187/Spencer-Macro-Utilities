//! Platform/version/logging/saved-setting Lua globals.

use crate::lua_runtime::LuaScriptVm;
use mlua::{Error as LuaError, Lua, Value};
use smu_display::DisplayCapture;
use smu_profile::{SavedSettingSource, SavedSettingValue};
use std::fmt;
use std::sync::Arc;

pub const MAX_SCRIPT_LOG_MESSAGE_BYTES: usize = 4096;
const LOG_TRUNCATE_SUFFIX: &[u8] = b"...(truncated)";
const SCRIPT_LOG_PREFIX: &[u8] = b"[script] ";

/// Log sink kept outside the Lua VM so application logging remains a service concern.
pub trait ScriptLogSink: Send + Sync {
    fn log_info(&self, message: &[u8]);
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScriptEnvironment {
    pub local_version: String,
}

impl ScriptEnvironment {
    #[must_use]
    pub fn new(local_version: impl Into<String>) -> Self {
        Self { local_version: local_version.into() }
    }
}

#[derive(Debug)]
pub struct ScriptCoreApiError(String);

impl ScriptCoreApiError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ScriptCoreApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ScriptCoreApiError {}

impl From<LuaError> for ScriptCoreApiError {
    fn from(error: LuaError) -> Self {
        Self::new(error.to_string())
    }
}

#[must_use]
pub const fn script_platform_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "unknown"
    }
}

fn push_saved_value(lua: &Lua, value: SavedSettingValue) -> mlua::Result<Value> {
    match value {
        SavedSettingValue::Bool(value) => Ok(Value::Boolean(value)),
        SavedSettingValue::Integer(value) => Ok(Value::Integer(value)),
        SavedSettingValue::Float(value) => Ok(Value::Number(value)),
        SavedSettingValue::String(value) => Ok(Value::String(lua.create_string(value)?)),
    }
}

fn dynamic_monitor_value(
    name: &str,
    display: Option<&Arc<dyn DisplayCapture>>,
) -> Option<SavedSettingValue> {
    let display = display?;
    match name {
        "active_monitor_width" => display
            .active_monitor_bounds()
            .filter(|bounds| bounds.is_valid())
            .map(|bounds| SavedSettingValue::Integer(i64::from(bounds.width))),
        "active_monitor_height" => display
            .active_monitor_bounds()
            .filter(|bounds| bounds.is_valid())
            .map(|bounds| SavedSettingValue::Integer(i64::from(bounds.height))),
        "active_monitor_hz" => display
            .active_monitor_refresh_rate_hz()
            .map(|hz| SavedSettingValue::Integer(i64::from(hz))),
        _ => None,
    }
}

fn saved_value(
    name: &str,
    settings: Option<&Arc<dyn SavedSettingSource>>,
    display: Option<&Arc<dyn DisplayCapture>>,
) -> Option<SavedSettingValue> {
    settings
        .and_then(|settings| settings.get_saved_setting(name))
        .or_else(|| dynamic_monitor_value(name, display))
}

fn truncated_script_log(message: &[u8]) -> Vec<u8> {
    let message = if message.len() > MAX_SCRIPT_LOG_MESSAGE_BYTES {
        let prefix_length = MAX_SCRIPT_LOG_MESSAGE_BYTES.saturating_sub(LOG_TRUNCATE_SUFFIX.len());
        let mut truncated = Vec::with_capacity(MAX_SCRIPT_LOG_MESSAGE_BYTES);
        truncated.extend_from_slice(&message[..prefix_length]);
        truncated.extend_from_slice(LOG_TRUNCATE_SUFFIX);
        truncated
    } else {
        message.to_vec()
    };

    let mut output = Vec::with_capacity(SCRIPT_LOG_PREFIX.len() + message.len());
    output.extend_from_slice(SCRIPT_LOG_PREFIX);
    output.extend_from_slice(&message);
    output
}

/// Register `log`, `getPlatform`, `getSMUVersion`, and `getSavedValue`.
///
/// # Errors
///
/// Returns a Lua registration error if a global cannot be installed.
pub fn install_core_api(
    vm: &LuaScriptVm,
    environment: ScriptEnvironment,
    settings: Option<Arc<dyn SavedSettingSource>>,
    display: Option<Arc<dyn DisplayCapture>>,
    logger: Option<Arc<dyn ScriptLogSink>>,
) -> Result<(), ScriptCoreApiError> {
    let resources = (environment, settings, display, logger);

    let log_sink = resources.3.clone();
    vm.lua.globals().set(
        "log",
        vm.lua.create_function(move |lua, value: Value| {
            let Some(message) = lua.coerce_string(value)? else {
                return Err(LuaError::RuntimeError(
                    "bad argument #1 to 'log' (string expected)".to_owned(),
                ));
            };
            if let Some(logger) = &log_sink {
                logger.log_info(&truncated_script_log(&message.as_bytes()));
            }
            Ok(())
        })?,
    )?;

    vm.lua
        .globals()
        .set("getPlatform", vm.lua.create_function(|_, ()| Ok(script_platform_name()))?)?;

    let local_version = resources.0.local_version.clone();
    vm.lua
        .globals()
        .set("getSMUVersion", vm.lua.create_function(move |_, ()| Ok(local_version.clone()))?)?;

    let saved_settings = resources.1.clone();
    let saved_display = resources.2.clone();
    vm.lua.globals().set(
        "getSavedValue",
        vm.lua.create_function(move |lua, value: Value| {
            let Some(name) = lua.coerce_string(value)? else {
                return Err(LuaError::RuntimeError(
                    "bad argument #1 to 'getSavedValue' (string expected)".to_owned(),
                ));
            };
            let name = name.to_string_lossy();
            saved_value(&name, saved_settings.as_ref(), saved_display.as_ref())
                .map_or_else(|| Ok(Value::Nil), |value| push_saved_value(lua, value))
        })?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::MAX_SCRIPT_RUNTIME;
    use smu_display::{PixelColor, ScreenBounds};
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Debug, Default)]
    struct FakeSettings(HashMap<String, SavedSettingValue>);

    impl SavedSettingSource for FakeSettings {
        fn get_saved_setting(&self, name: &str) -> Option<SavedSettingValue> {
            self.0.get(name).cloned()
        }
    }

    struct FakeDisplay;

    impl DisplayCapture for FakeDisplay {
        fn active_monitor_bounds(&self) -> Option<ScreenBounds> {
            Some(ScreenBounds { x: 100, y: 200, width: 2560, height: 1440 })
        }

        fn active_monitor_refresh_rate_hz(&self) -> Option<i32> {
            Some(165)
        }

        fn sample_pixel(&self, _x: i32, _y: i32) -> Result<PixelColor, smu_display::DisplayError> {
            Ok(PixelColor::default())
        }
    }

    #[derive(Debug, Default)]
    struct FakeLogger(Mutex<Vec<Vec<u8>>>);

    impl ScriptLogSink for FakeLogger {
        fn log_info(&self, message: &[u8]) {
            self.0.lock().expect("log lock").push(message.to_vec());
        }
    }

    fn install(
        vm: &LuaScriptVm,
        settings: Option<Arc<dyn SavedSettingSource>>,
        display: Option<Arc<dyn DisplayCapture>>,
        logger: Option<Arc<dyn ScriptLogSink>>,
    ) {
        install_core_api(vm, ScriptEnvironment::new("3.4.0"), settings, display, logger)
            .expect("core API should install");
    }

    #[test]
    fn platform_and_version_globals_are_stable() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        install(&vm, None, None, None);
        assert_eq!(
            vm.eval::<String>("return getPlatform()", MAX_SCRIPT_RUNTIME)
                .expect("platform should return"),
            script_platform_name()
        );
        assert_eq!(
            vm.eval::<String>("return getSMUVersion()", MAX_SCRIPT_RUNTIME)
                .expect("version should return"),
            "3.4.0"
        );
    }

    #[test]
    fn saved_scalar_types_are_preserved() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let settings = Arc::new(FakeSettings(HashMap::from([
            ("flag".to_owned(), SavedSettingValue::Bool(true)),
            ("count".to_owned(), SavedSettingValue::Integer(-4)),
            ("ratio".to_owned(), SavedSettingValue::Float(1.25)),
            ("text".to_owned(), SavedSettingValue::String("hello".to_owned())),
        ])));
        install(&vm, Some(settings), None, None);
        let values = vm
            .eval::<(bool, i64, f64, String, bool)>(
                concat!(
                    "return getSavedValue('flag'),getSavedValue('count'),getSavedValue('ratio'),",
                    "getSavedValue('text'),getSavedValue('missing') == nil"
                ),
                MAX_SCRIPT_RUNTIME,
            )
            .expect("saved values should return");
        assert_eq!(values, (true, -4, 1.25, "hello".to_owned(), true));
    }

    #[test]
    fn active_monitor_values_come_from_display_service() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        install(&vm, None, Some(Arc::new(FakeDisplay)), None);
        assert_eq!(
            vm.eval::<(i64, i64, i64)>(
                concat!(
                    "return getSavedValue('active_monitor_width'),",
                    "getSavedValue('active_monitor_height'),getSavedValue('active_monitor_hz')"
                ),
                MAX_SCRIPT_RUNTIME,
            )
            .expect("monitor values should return"),
            (2560, 1440, 165)
        );
    }

    #[test]
    fn persisted_source_precedes_dynamic_monitor_names() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let settings = Arc::new(FakeSettings(HashMap::from([(
            "active_monitor_width".to_owned(),
            SavedSettingValue::Integer(999),
        )])));
        install(&vm, Some(settings), Some(Arc::new(FakeDisplay)), None);
        assert_eq!(
            vm.eval::<i64>("return getSavedValue('active_monitor_width')", MAX_SCRIPT_RUNTIME,)
                .expect("source value should win"),
            999
        );
    }

    #[test]
    fn log_prefix_and_truncation_are_byte_exact() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let logger = Arc::new(FakeLogger::default());
        install(&vm, None, None, Some(logger.clone()));
        vm.execute_source("log('short'); log(string.rep('x', 5000))", "log.lua")
            .expect("logging should execute");
        let messages = logger.0.lock().expect("log lock");
        assert_eq!(messages[0], b"[script] short");
        assert_eq!(messages[1].len(), SCRIPT_LOG_PREFIX.len() + MAX_SCRIPT_LOG_MESSAGE_BYTES);
        assert!(messages[1].starts_with(SCRIPT_LOG_PREFIX));
        assert!(messages[1].ends_with(LOG_TRUNCATE_SUFFIX));
        drop(messages);
    }

    #[test]
    fn missing_services_keep_nil_and_noop_behavior() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        install(&vm, None, None, None);
        vm.execute_source("log('ignored')", "no-log.lua")
            .expect("missing log sink should be a no-op");
        assert!(
            vm.eval::<bool>("return getSavedValue('anything') == nil", MAX_SCRIPT_RUNTIME)
                .expect("missing saved value should be nil")
        );
    }
}

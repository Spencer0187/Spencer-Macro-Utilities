//! Lag-switch Lua API adapters.

use crate::input_api::{ScriptExecutionMode, ScriptInputContext};
use crate::lua_runtime::LuaScriptVm;
use mlua::{Error as LuaError, Lua, Table, Value};
use smu_network::{
    LagSwitchConfig, LagSwitchControl, LagSwitchInitializationFailure, LagSwitchTargetMode,
    is_valid_ipv4_address,
};
use std::fmt;
use std::sync::Arc;

const MAX_LIST_ENTRIES: usize = 64;
const MAX_DELAY_MS: i64 = 300_000;
const MAX_DURATION_SECONDS: f64 = 3600.0;
pub(crate) const LAG_SWITCH_REQUIRES_ADMIN_WARNING: &str = "Lag switch was requested, but SMU is not running as Administrator. The script continued, but lag-switch actions were skipped.";
const LAG_SWITCH_REQUIRES_LINUX_HELPER_WARNING: &str = "Lag switch was requested, but the Linux lagswitch helper is not running. Approve the lagswitch permission prompt to continue.";
const LAG_SWITCH_INITIALIZATION_ERROR: &str = "Lag switch backend could not be initialized.";

#[derive(Debug)]
pub struct ScriptNetworkApiError(String);

impl ScriptNetworkApiError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ScriptNetworkApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ScriptNetworkApiError {}

impl From<LuaError> for ScriptNetworkApiError {
    fn from(error: LuaError) -> Self {
        Self::new(error.to_string())
    }
}

fn settings_callback_active(context: &ScriptInputContext) -> bool {
    matches!(
        context.mode(),
        ScriptExecutionMode::SettingsRender | ScriptExecutionMode::SettingsCallback
    )
}

fn check_settings_callback(context: &ScriptInputContext, function_name: &str) -> mlua::Result<()> {
    if settings_callback_active(context) {
        return Err(LuaError::RuntimeError(format!(
            "{function_name} is not available inside script settings"
        )));
    }
    Ok(())
}

fn check_cleanup_start(context: &ScriptInputContext, function_name: &str) -> mlua::Result<()> {
    if context.mode() == ScriptExecutionMode::Cleanup {
        return Err(LuaError::RuntimeError(format!(
            "{function_name} is not available inside onCleanup"
        )));
    }
    Ok(())
}

const fn target_mode_name(mode: LagSwitchTargetMode) -> &'static str {
    match mode {
        LagSwitchTargetMode::Roblox => "roblox",
        LagSwitchTargetMode::All => "all",
        LagSwitchTargetMode::Custom => "custom",
    }
}

fn table_optional_bool(table: &Table, name: &str, current: bool) -> mlua::Result<bool> {
    match table.get::<Value>(name)? {
        Value::Nil => Ok(current),
        Value::Boolean(value) => Ok(value),
        _ => Err(LuaError::RuntimeError(format!("{name} must be a boolean"))),
    }
}

fn table_optional_integer(
    lua: &Lua,
    table: &Table,
    name: &str,
    current: i32,
    minimum: i64,
    maximum: i64,
) -> mlua::Result<i32> {
    let value: Value = table.get(name)?;
    if matches!(value, Value::Nil) {
        return Ok(current);
    }
    let Some(integer) = lua.coerce_integer(value)? else {
        return Err(LuaError::RuntimeError(format!("{name} must be an integer")));
    };
    if integer < minimum || integer > maximum {
        return Err(LuaError::RuntimeError(format!("{name} is outside the allowed range")));
    }
    i32::try_from(integer)
        .map_err(|_| LuaError::RuntimeError(format!("{name} is outside the allowed range")))
}

fn table_optional_float(
    lua: &Lua,
    table: &Table,
    name: &str,
    current: f32,
    minimum: f64,
    maximum: f64,
) -> mlua::Result<f32> {
    let value: Value = table.get(name)?;
    if matches!(value, Value::Nil) {
        return Ok(current);
    }
    let Some(number) = lua.coerce_number(value)? else {
        return Err(LuaError::RuntimeError(format!("{name} must be a number")));
    };
    if !number.is_finite() {
        return Err(LuaError::RuntimeError(format!("{name} must be a finite number")));
    }
    if number < minimum || number > maximum {
        return Err(LuaError::RuntimeError(format!("{name} is outside the allowed range")));
    }
    #[allow(clippy::cast_possible_truncation)]
    Ok(number as f32)
}

fn table_optional_target_mode(
    table: &Table,
    name: &str,
    current: LagSwitchTargetMode,
) -> mlua::Result<LagSwitchTargetMode> {
    let value: Value = table.get(name)?;
    if matches!(value, Value::Nil) {
        return Ok(current);
    }
    let Value::String(text) = value else {
        return Err(LuaError::RuntimeError(format!(
            "{name} must be \"roblox\", \"all\", or \"custom\""
        )));
    };
    match text.to_string_lossy().as_ref() {
        "roblox" => Ok(LagSwitchTargetMode::Roblox),
        "all" => Ok(LagSwitchTargetMode::All),
        "custom" => Ok(LagSwitchTargetMode::Custom),
        _ => Err(LuaError::RuntimeError(format!(
            "{name} must be \"roblox\", \"all\", or \"custom\""
        ))),
    }
}

fn table_optional_ip_list(
    lua: &Lua,
    table: &Table,
    name: &str,
    current: &[String],
) -> mlua::Result<Vec<String>> {
    let value: Value = table.get(name)?;
    let Value::Table(list) = value else {
        if matches!(value, Value::Nil) {
            return Ok(current.to_vec());
        }
        return Err(LuaError::RuntimeError(format!("{name} must be a table")));
    };

    let mut values = Vec::new();
    for pair in list.pairs::<Value, Value>() {
        let (_, value) = pair?;
        if values.len() >= MAX_LIST_ENTRIES {
            return Err(LuaError::RuntimeError(format!(
                "{name} may contain at most {MAX_LIST_ENTRIES} entries"
            )));
        }
        let Some(text) = lua.coerce_string(value)? else {
            return Err(LuaError::RuntimeError(format!("{name} contains an invalid IPv4 address")));
        };
        let text = text.to_string_lossy();
        if !is_valid_ipv4_address(&text) {
            return Err(LuaError::RuntimeError(format!("{name} contains an invalid IPv4 address")));
        }
        values.push(text);
    }
    Ok(values)
}

fn table_optional_port_list(
    lua: &Lua,
    table: &Table,
    name: &str,
    current: &[i32],
) -> mlua::Result<Vec<i32>> {
    let value: Value = table.get(name)?;
    let Value::Table(list) = value else {
        if matches!(value, Value::Nil) {
            return Ok(current.to_vec());
        }
        return Err(LuaError::RuntimeError(format!("{name} must be a table")));
    };

    let mut values = Vec::new();
    for pair in list.pairs::<Value, Value>() {
        let (_, value) = pair?;
        if values.len() >= MAX_LIST_ENTRIES {
            return Err(LuaError::RuntimeError(format!(
                "{name} may contain at most {MAX_LIST_ENTRIES} entries"
            )));
        }
        let Some(port) = lua.coerce_integer(value)? else {
            return Err(LuaError::RuntimeError(format!("{name} must be an integer")));
        };
        if !(1..=65_535).contains(&port) {
            return Err(LuaError::RuntimeError(format!("{name} is outside the allowed range")));
        }
        values.push(i32::try_from(port).unwrap_or_default());
    }
    Ok(values)
}

fn validate_config_keys(table: &Table) -> mlua::Result<()> {
    const ALLOWED: [&str; 16] = [
        "hardBlockInbound",
        "hardBlockOutbound",
        "fakeLag",
        "fakeLagInbound",
        "fakeLagOutbound",
        "fakeLagDelayMs",
        "targetMode",
        "useUdp",
        "useTcp",
        "preventDisconnect",
        "autoUnblock",
        "maxDurationSeconds",
        "unblockDurationMs",
        "remoteIps",
        "remotePorts",
        "includeRobloxDynamicIps",
    ];

    for pair in table.clone().pairs::<Value, Value>() {
        let (key, _) = pair?;
        let Value::String(key) = key else {
            return Err(LuaError::RuntimeError(
                "lag-switch config keys must be strings".to_owned(),
            ));
        };
        let key = key.to_string_lossy();
        if !ALLOWED.contains(&key.as_ref()) {
            return Err(LuaError::RuntimeError(format!("unknown lag-switch config key: {key}")));
        }
    }
    Ok(())
}

fn parse_config(
    lua: &Lua,
    table: &Table,
    mut config: LagSwitchConfig,
) -> mlua::Result<LagSwitchConfig> {
    validate_config_keys(table)?;
    config.inbound_hard_block =
        table_optional_bool(table, "hardBlockInbound", config.inbound_hard_block)?;
    config.outbound_hard_block =
        table_optional_bool(table, "hardBlockOutbound", config.outbound_hard_block)?;
    config.fake_lag_enabled = table_optional_bool(table, "fakeLag", config.fake_lag_enabled)?;
    config.inbound_fake_lag =
        table_optional_bool(table, "fakeLagInbound", config.inbound_fake_lag)?;
    config.outbound_fake_lag =
        table_optional_bool(table, "fakeLagOutbound", config.outbound_fake_lag)?;
    config.fake_lag_delay_ms = table_optional_integer(
        lua,
        table,
        "fakeLagDelayMs",
        config.fake_lag_delay_ms,
        0,
        MAX_DELAY_MS,
    )?;
    config.target_mode = table_optional_target_mode(table, "targetMode", config.target_mode)?;
    config.use_udp = table_optional_bool(table, "useUdp", config.use_udp)?;
    config.use_tcp = table_optional_bool(table, "useTcp", config.use_tcp)?;
    config.prevent_disconnect =
        table_optional_bool(table, "preventDisconnect", config.prevent_disconnect)?;
    config.auto_unblock = table_optional_bool(table, "autoUnblock", config.auto_unblock)?;
    config.max_duration_seconds = table_optional_float(
        lua,
        table,
        "maxDurationSeconds",
        config.max_duration_seconds,
        0.0,
        MAX_DURATION_SECONDS,
    )?;
    config.unblock_duration_ms = table_optional_integer(
        lua,
        table,
        "unblockDurationMs",
        config.unblock_duration_ms,
        0,
        MAX_DELAY_MS,
    )?;
    config.remote_ips = table_optional_ip_list(lua, table, "remoteIps", &config.remote_ips)?;
    config.remote_ports =
        table_optional_port_list(lua, table, "remotePorts", &config.remote_ports)?;
    config.include_roblox_dynamic_ips =
        table_optional_bool(table, "includeRobloxDynamicIps", config.include_roblox_dynamic_ips)?;
    config.target_roblox_only = config.target_mode == LagSwitchTargetMode::Roblox;
    Ok(config)
}

fn push_string_list(lua: &Lua, values: &[String]) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    for (index, value) in values.iter().enumerate() {
        table.raw_set(index + 1, value.as_str())?;
    }
    Ok(table)
}

fn push_int_list(lua: &Lua, values: &[i32]) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    for (index, value) in values.iter().copied().enumerate() {
        table.raw_set(index + 1, value)?;
    }
    Ok(table)
}

fn push_config(lua: &Lua, config: &LagSwitchConfig) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.set("hardBlockInbound", config.inbound_hard_block)?;
    table.set("hardBlockOutbound", config.outbound_hard_block)?;
    table.set("fakeLag", config.fake_lag_enabled)?;
    table.set("fakeLagInbound", config.inbound_fake_lag)?;
    table.set("fakeLagOutbound", config.outbound_fake_lag)?;
    table.set("fakeLagDelayMs", config.fake_lag_delay_ms)?;
    table.set("targetMode", target_mode_name(config.target_mode))?;
    table.set("useUdp", config.use_udp)?;
    table.set("useTcp", config.use_tcp)?;
    table.set("preventDisconnect", config.prevent_disconnect)?;
    table.set("autoUnblock", config.auto_unblock)?;
    table.set("maxDurationSeconds", config.max_duration_seconds)?;
    table.set("unblockDurationMs", config.unblock_duration_ms)?;
    table.set("includeRobloxDynamicIps", config.include_roblox_dynamic_ips)?;
    table.set("remoteIps", push_string_list(lua, &config.remote_ips)?)?;
    table.set("remotePorts", push_int_list(lua, &config.remote_ports)?)?;
    Ok(table)
}

fn effective_config(backend: Option<&Arc<dyn LagSwitchControl>>) -> LagSwitchConfig {
    backend.map_or_else(LagSwitchConfig::default, |backend| backend.effective_config())
}

fn install_toggle_api(
    lua: &Lua,
    context: &ScriptInputContext,
    backend: Option<&Arc<dyn LagSwitchControl>>,
) -> mlua::Result<()> {
    let toggle_context = context.clone();
    let toggle_backend = backend.cloned();
    let function = lua.create_function(move |lua, (enabled, config): (Value, Option<Value>)| {
        check_settings_callback(&toggle_context, "lagSwitch")?;
        let enabled = !matches!(enabled, Value::Nil | Value::Boolean(false));
        if toggle_context.mode() == ScriptExecutionMode::Cleanup && enabled {
            return Err(LuaError::RuntimeError(
                "lagSwitch(true) is not available inside onCleanup".to_owned(),
            ));
        }
        if let Some(config) = config.filter(|value| !matches!(value, Value::Nil)) {
            let Value::Table(table) = config else {
                return Err(LuaError::RuntimeError("lag-switch config must be a table".to_owned()));
            };
            let current = effective_config(toggle_backend.as_ref());
            let parsed = parse_config(lua, &table, current)?;
            if let Some(backend) = &toggle_backend {
                backend.set_script_config_override(toggle_context.owner_token(), parsed);
            }
        }
        if let Some(backend) = &toggle_backend {
            if enabled && !backend.is_initialized() {
                if backend.initialize() {
                    toggle_context.clear_lag_switch_warning();
                } else {
                    match backend.initialization_failure() {
                        LagSwitchInitializationFailure::RequiresAdministrator => {
                            toggle_context
                                .set_lag_switch_warning(LAG_SWITCH_REQUIRES_ADMIN_WARNING);
                        }
                        LagSwitchInitializationFailure::RequiresHelper => {
                            toggle_context
                                .set_lag_switch_warning(LAG_SWITCH_REQUIRES_LINUX_HELPER_WARNING);
                        }
                        LagSwitchInitializationFailure::Error(message) => {
                            toggle_context.set_lag_switch_error(if message.is_empty() {
                                LAG_SWITCH_INITIALIZATION_ERROR.to_owned()
                            } else {
                                message
                            });
                        }
                    }
                    return Ok(());
                }
            }
            toggle_context.mark_lag_switch_touched();
            backend.set_script_blocking_active(toggle_context.owner_token(), enabled);
        }
        Ok(())
    })?;
    lua.globals().set("lagSwitch", function.clone())?;
    lua.globals().set("lagswitch", function)
}

fn install_config_api(
    lua: &Lua,
    context: &ScriptInputContext,
    backend: Option<&Arc<dyn LagSwitchControl>>,
) -> mlua::Result<()> {
    let get_context = context.clone();
    let get_backend = backend.cloned();
    lua.globals().set(
        "getLagSwitchConfig",
        lua.create_function(move |lua, ()| {
            check_settings_callback(&get_context, "getLagSwitchConfig")?;
            push_config(lua, &effective_config(get_backend.as_ref()))
        })?,
    )?;

    let set_context = context.clone();
    let set_backend = backend.cloned();
    lua.globals().set(
        "setLagSwitchConfig",
        lua.create_function(move |lua, value: Value| {
            check_settings_callback(&set_context, "setLagSwitchConfig")?;
            check_cleanup_start(&set_context, "setLagSwitchConfig")?;
            let Value::Table(table) = value else {
                return Err(LuaError::RuntimeError("lag-switch config must be a table".to_owned()));
            };
            let parsed = parse_config(lua, &table, effective_config(set_backend.as_ref()))?;
            if let Some(backend) = &set_backend {
                set_context.mark_lag_switch_touched();
                backend.set_script_config_override(set_context.owner_token(), parsed);
            }
            Ok(())
        })?,
    )?;

    let clear_context = context.clone();
    let clear_backend = backend.cloned();
    lua.globals().set(
        "clearLagSwitchConfig",
        lua.create_function(move |_, ()| {
            check_settings_callback(&clear_context, "clearLagSwitchConfig")?;
            if let Some(backend) = &clear_backend {
                backend.clear_script_config_override(clear_context.owner_token());
            }
            Ok(())
        })?,
    )
}

fn install_status_api(
    lua: &Lua,
    context: &ScriptInputContext,
    backend: Option<&Arc<dyn LagSwitchControl>>,
) -> mlua::Result<()> {
    let context = context.clone();
    let backend = backend.cloned();
    lua.globals().set(
        "getLagSwitchStatus",
        lua.create_function(move |lua, ()| {
            check_settings_callback(&context, "getLagSwitchStatus")?;
            let available = backend.as_ref().is_some_and(|backend| backend.is_available());
            let config = effective_config(backend.as_ref());
            let table = lua.create_table()?;
            table.set("available", available)?;
            table.set("enabled", config.enabled)?;
            table.set(
                "active",
                backend.as_ref().is_some_and(|backend| backend.is_blocking_active()),
            )?;
            table.set(
                "baseActive",
                backend.as_ref().is_some_and(|backend| backend.is_base_blocking_active()),
            )?;
            table.set("platformSupport", if available { "available" } else { "unsupported" })?;
            table.set(
                "unsupportedReason",
                backend.as_ref().map_or_else(
                    || "Network lagswitch backend is unavailable.".to_owned(),
                    |backend| backend.unsupported_reason(),
                ),
            )?;
            table.set("targetMode", target_mode_name(config.target_mode))?;
            Ok(table)
        })?,
    )
}

/// Register lag-switch globals against the modular network service.
///
/// # Errors
///
/// Returns a Lua registration error if the functions cannot be installed.
pub fn install_network_api(
    vm: &LuaScriptVm,
    context: ScriptInputContext,
    backend: Option<Arc<dyn LagSwitchControl>>,
) -> Result<(), ScriptNetworkApiError> {
    let resources = (context, backend);
    let context = &resources.0;
    let backend = resources.1.as_ref();
    install_toggle_api(&vm.lua, context, backend)?;
    install_config_api(&vm.lua, context, backend)?;
    install_status_api(&vm.lua, context, backend)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::MAX_SCRIPT_RUNTIME;
    use smu_network::LagSwitchState;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[derive(Debug)]
    struct FakeBackend {
        state: Mutex<LagSwitchState>,
        available: bool,
        initialized: AtomicBool,
        initialize_success: bool,
        initialize_failure: LagSwitchInitializationFailure,
        initialize_calls: AtomicUsize,
        reason: String,
    }

    impl Default for FakeBackend {
        fn default() -> Self {
            Self {
                state: Mutex::new(LagSwitchState::default()),
                available: true,
                initialized: AtomicBool::new(true),
                initialize_success: true,
                initialize_failure: LagSwitchInitializationFailure::Error(String::new()),
                initialize_calls: AtomicUsize::new(0),
                reason: String::new(),
            }
        }
    }

    impl FakeBackend {
        fn cold(initialize_success: bool) -> Self {
            Self { initialized: AtomicBool::new(false), initialize_success, ..Self::default() }
        }

        fn cold_with_failure(failure: LagSwitchInitializationFailure) -> Self {
            Self {
                initialized: AtomicBool::new(false),
                initialize_success: false,
                initialize_failure: failure,
                ..Self::default()
            }
        }

        fn state(&self) -> std::sync::MutexGuard<'_, LagSwitchState> {
            self.state.lock().expect("lag-switch state lock")
        }
    }

    impl LagSwitchControl for FakeBackend {
        fn initialize(&self) -> bool {
            self.initialize_calls.fetch_add(1, Ordering::AcqRel);
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
            self.initialize_failure.clone()
        }

        fn is_available(&self) -> bool {
            self.available
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
            self.reason.clone()
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
    }

    fn vm_with_backend(backend: Arc<FakeBackend>) -> (LuaScriptVm, ScriptInputContext) {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let context = ScriptInputContext::default();
        install_network_api(&vm, context.clone(), Some(backend))
            .expect("network API should install");
        (vm, context)
    }

    fn vm_with_network() -> (LuaScriptVm, ScriptInputContext, Arc<FakeBackend>) {
        let backend = Arc::new(FakeBackend::default());
        let (vm, context) = vm_with_backend(backend.clone());
        (vm, context, backend)
    }

    #[test]
    fn config_round_trip_preserves_cpp_field_names_and_defaults() {
        let (vm, _, backend) = vm_with_network();
        vm.execute_source(
            concat!(
                "setLagSwitchConfig({targetMode='custom', fakeLag=true, fakeLagDelayMs=125, ",
                "remoteIps={'001.002.003.004'}, remotePorts={80,443}, useTcp=true})"
            ),
            "config.lua",
        )
        .expect("valid config should apply");
        let config = backend.effective_config();
        assert_eq!(config.target_mode, LagSwitchTargetMode::Custom);
        assert!(!config.target_roblox_only);
        assert!(config.fake_lag_enabled);
        assert_eq!(config.fake_lag_delay_ms, 125);
        assert_eq!(config.remote_ips, ["001.002.003.004"]);
        assert_eq!(config.remote_ports, [80, 443]);
        assert!(config.use_tcp);

        let values = vm
            .eval::<(String, bool, i32, String, i32)>(
                concat!(
                    "local c=getLagSwitchConfig(); return c.targetMode,c.fakeLag,c.fakeLagDelayMs,",
                    "c.remoteIps[1],c.remotePorts[2]"
                ),
                MAX_SCRIPT_RUNTIME,
            )
            .expect("config should be readable");
        assert_eq!(values, ("custom".to_owned(), true, 125, "001.002.003.004".to_owned(), 443));
    }

    #[test]
    fn unknown_keys_and_invalid_values_match_cpp_validation() {
        let (vm, _, _) = vm_with_network();
        assert!(
            vm.execute_source("setLagSwitchConfig({wat=true})", "unknown.lua")
                .expect_err("unknown config key should fail")
                .message
                .contains("unknown lag-switch config key: wat")
        );
        assert!(
            vm.execute_source("setLagSwitchConfig({remoteIps={'256.1.1.1'}})", "bad-ip.lua")
                .expect_err("bad IP should fail")
                .message
                .contains("remoteIps contains an invalid IPv4 address")
        );
        assert!(
            vm.execute_source("setLagSwitchConfig({remotePorts={0}})", "bad-port.lua")
                .expect_err("bad port should fail")
                .message
                .contains("remotePorts is outside the allowed range")
        );
    }

    #[test]
    fn lag_switch_toggle_and_alias_use_script_owner() {
        let (vm, context, backend) = vm_with_network();
        vm.execute_source("lagswitch(true)", "enable.lua").expect("alias should enable lag switch");
        assert!(backend.is_blocking_active());
        backend.state().clear_script_state(context.owner_token() + 1);
        assert!(backend.is_blocking_active(), "wrong owner must not clear script state");
        vm.execute_source("lagSwitch(false)", "disable.lua")
            .expect("canonical name should disable lag switch");
        assert!(!backend.is_blocking_active());
    }

    #[test]
    fn lag_switch_true_cold_initializes_before_claiming_script_blocking() {
        let backend = Arc::new(FakeBackend::cold(true));
        let (vm, _) = vm_with_backend(backend.clone());

        vm.execute_source("lagSwitch(true)", "cold-enable.lua")
            .expect("successful cold initialization should keep script running");

        assert_eq!(backend.initialize_calls.load(Ordering::Acquire), 1);
        assert!(backend.is_initialized());
        assert!(backend.is_blocking_active());
    }

    #[test]
    fn failed_cold_initialization_skips_script_blocking_without_throwing() {
        let backend = Arc::new(FakeBackend::cold(false));
        let (vm, context) = vm_with_backend(backend.clone());

        vm.execute_source("lagSwitch(true)", "cold-failure.lua")
            .expect("C++ keeps the script running when backend initialization fails");

        assert_eq!(backend.initialize_calls.load(Ordering::Acquire), 1);
        assert!(!backend.is_initialized());
        assert!(!backend.is_blocking_active());
        assert!(!context.take_lag_switch_touched());
    }

    #[test]
    fn failed_cold_initialization_publishes_cpp_admin_warning_without_claiming_block() {
        let backend = Arc::new(FakeBackend::cold_with_failure(
            LagSwitchInitializationFailure::RequiresAdministrator,
        ));
        let (vm, context) = vm_with_backend(backend.clone());

        vm.execute_source("lagSwitch(true)", "cold-admin.lua")
            .expect("missing elevation must not throw into Lua");

        assert_eq!(context.lag_switch_warning(), LAG_SWITCH_REQUIRES_ADMIN_WARNING);
        assert!(context.lag_switch_error().is_empty());
        assert!(!backend.is_blocking_active());
        assert!(!context.take_lag_switch_touched());
    }

    #[test]
    fn failed_cold_initialization_publishes_cpp_linux_helper_warning() {
        let backend = Arc::new(FakeBackend::cold_with_failure(
            LagSwitchInitializationFailure::RequiresHelper,
        ));
        let (vm, context) = vm_with_backend(backend);

        vm.execute_source("lagSwitch(true)", "cold-helper.lua")
            .expect("missing helper must not throw into Lua");

        assert_eq!(context.lag_switch_warning(), LAG_SWITCH_REQUIRES_LINUX_HELPER_WARNING);
        assert!(context.lag_switch_error().is_empty());
    }

    #[test]
    fn failed_cold_initialization_preserves_backend_error_for_instance_diagnostics() {
        let backend = Arc::new(FakeBackend::cold_with_failure(
            LagSwitchInitializationFailure::Error("driver probe failed".to_owned()),
        ));
        let (vm, context) = vm_with_backend(backend);

        vm.execute_source("lagSwitch(true)", "cold-error.lua")
            .expect("backend init errors remain non-throwing like C++");

        assert!(context.lag_switch_warning().is_empty());
        assert_eq!(context.lag_switch_error(), "driver probe failed");
    }

    #[test]
    fn successful_cold_initialization_clears_stale_lag_switch_warning() {
        let backend = Arc::new(FakeBackend::cold(true));
        let (vm, context) = vm_with_backend(backend);
        context.set_lag_switch_warning("stale warning");

        vm.execute_source("lagSwitch(true)", "cold-success.lua")
            .expect("successful init should enable lag switch");

        assert!(context.lag_switch_warning().is_empty());
    }

    #[test]
    fn cleanup_can_disable_and_clear_but_cannot_enable_or_set_config() {
        let (vm, context, backend) = vm_with_network();
        vm.execute_source("lagSwitch(true)", "enable.lua").expect("normal mode should enable");
        context.set_mode(ScriptExecutionMode::Cleanup);
        assert!(
            vm.execute_source("lagSwitch(true)", "cleanup-enable.lua")
                .expect_err("cleanup enable should fail")
                .message
                .contains("lagSwitch(true) is not available inside onCleanup")
        );
        assert!(
            vm.execute_source("setLagSwitchConfig({fakeLag=true})", "cleanup-config.lua")
                .expect_err("cleanup config set should fail")
                .message
                .contains("setLagSwitchConfig is not available inside onCleanup")
        );
        vm.execute_source("clearLagSwitchConfig(); lagSwitch(false)", "cleanup-disable.lua")
            .expect("cleanup release operations should be allowed");
        assert!(!backend.is_blocking_active());
    }

    #[test]
    fn status_shape_matches_cpp_fields() {
        let (vm, _, backend) = vm_with_network();
        backend.state().set_base_blocking(true);
        let status = vm
            .eval::<(bool, bool, bool, bool, String, String, String)>(
                concat!(
                    "local s=getLagSwitchStatus(); return s.available,s.enabled,s.active,s.baseActive,",
                    "s.platformSupport,s.unsupportedReason,s.targetMode"
                ),
                MAX_SCRIPT_RUNTIME,
            )
            .expect("status should return");
        assert_eq!(
            status,
            (true, true, true, true, "available".to_owned(), String::new(), "roblox".to_owned(),)
        );
    }

    #[test]
    fn missing_backend_keeps_default_read_and_noop_write_behavior() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let context = ScriptInputContext::default();
        install_network_api(&vm, context, None)
            .expect("network API should install without backend");
        vm.execute_source(
            "setLagSwitchConfig({fakeLag=true}); lagSwitch(true); clearLagSwitchConfig()",
            "missing.lua",
        )
        .expect("writes without backend are no-ops after validation");
        let values = vm
            .eval::<(bool, bool, String)>(
                "local s=getLagSwitchStatus(); return s.available,s.active,s.unsupportedReason",
                MAX_SCRIPT_RUNTIME,
            )
            .expect("missing backend status should return");
        assert_eq!(values, (false, false, "Network lagswitch backend is unavailable.".to_owned(),));
    }
}

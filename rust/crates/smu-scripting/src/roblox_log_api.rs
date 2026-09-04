//! Roblox player-log Lua API adapter.

use crate::input_api::{ScriptExecutionMode, ScriptInputContext};
use crate::lua_runtime::{ExecutionState, LuaScriptVm, ScriptStopReason};
use mlua::{Error as LuaError, Function, Lua, MultiValue, RegistryKey, Table, Value};
use regress::Regex;
use smu_roblox::{RobloxLogSnapshot, RobloxLogSource};
use std::fmt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

pub const MAX_ROBLOX_LOG_PATTERN_BYTES: usize = 1024;
pub const MAX_ROBLOX_LOG_CALLBACKS: usize = 128;

struct RobloxLogCallback {
    pattern: Vec<u8>,
    regex: Option<Regex>,
    registry_key: RegistryKey,
}

#[derive(Default)]
struct RobloxLogCallbackState {
    dispatching: AtomicBool,
    callbacks: Mutex<Vec<RobloxLogCallback>>,
}

#[derive(Clone, Default)]
pub struct ScriptRobloxLogContext {
    state: Arc<RobloxLogCallbackState>,
}

impl fmt::Debug for ScriptRobloxLogContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScriptRobloxLogContext")
            .field("dispatching", &self.is_dispatching())
            .field("callback_count", &self.callback_count())
            .finish()
    }
}

impl ScriptRobloxLogContext {
    fn callbacks(&self) -> MutexGuard<'_, Vec<RobloxLogCallback>> {
        match self.state.callbacks.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    #[must_use]
    pub fn is_dispatching(&self) -> bool {
        self.state.dispatching.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn callback_count(&self) -> usize {
        self.callbacks().len()
    }

    /// Remove all registered callbacks from the Lua registry.
    pub fn clear_callbacks(&self, vm: &LuaScriptVm) {
        let callbacks = std::mem::take(&mut *self.callbacks());
        for callback in callbacks {
            let _ = vm.lua.remove_registry_value(callback.registry_key);
        }
    }
}

#[derive(Debug)]
pub struct ScriptRobloxLogApiError(String);

impl ScriptRobloxLogApiError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ScriptRobloxLogApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ScriptRobloxLogApiError {}

impl From<LuaError> for ScriptRobloxLogApiError {
    fn from(error: LuaError) -> Self {
        Self::new(error.to_string())
    }
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn path_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().as_bytes().to_vec()
}

const fn cpp_u64_to_lua_integer(value: u64) -> i64 {
    #[allow(clippy::cast_possible_wrap)]
    {
        value as i64
    }
}

fn push_snapshot(lua: &Lua, snapshot: &RobloxLogSnapshot) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.set("available", snapshot.available)?;
    table.set("startedNewLog", snapshot.started_new_log)?;
    table.set("path", lua.create_string(path_bytes(&snapshot.path))?)?;
    table.set("state", snapshot.state.as_str())?;
    table.set("placeId", cpp_u64_to_lua_integer(snapshot.place_id))?;
    table.set("userId", cpp_u64_to_lua_integer(snapshot.user_id))?;
    table.set("universeId", cpp_u64_to_lua_integer(snapshot.universe_id))?;
    table.set("jobId", snapshot.job_id.as_str())?;
    table.set("clientChannel", snapshot.client_channel.as_str())?;
    table.set("clientRobloxGitHash", snapshot.client_roblox_git_hash.as_str())?;
    table.set("serverRobloxGitHash", snapshot.server_roblox_git_hash.as_str())?;
    table.set("serverPrefix", snapshot.server_prefix.as_str())?;
    table.set("serverLuauVersion", snapshot.server_luau_version.as_str())?;

    let resolution = lua.create_table()?;
    resolution.set("width", snapshot.resolution_width)?;
    resolution.set("height", snapshot.resolution_height)?;
    table.set("resolution", resolution)?;

    let server = lua.create_table()?;
    server.set("type", snapshot.server.r#type.as_str())?;
    server.set("rccAddress", snapshot.server.rcc_address.as_str())?;
    server.set("rccPort", snapshot.server.rcc_port)?;
    server.set("udmuxAddress", snapshot.server.udmux_address.as_str())?;
    server.set("udmuxPort", snapshot.server.udmux_port)?;
    table.set("server", server)?;

    let lines = lua.create_table()?;
    for (index, line) in snapshot.lines.iter().enumerate() {
        lines.raw_set(index + 1, lua.create_string(line.as_bytes())?)?;
    }
    table.set("lines", lines)?;
    Ok(table)
}

fn parse_include_existing(arguments: &MultiValue) -> mlua::Result<bool> {
    if arguments.len() > 1 {
        return Err(LuaError::RuntimeError(
            "readRobloxLog accepts at most one argument".to_owned(),
        ));
    }
    let Some(value) = arguments.front() else {
        return Ok(false);
    };
    match value {
        Value::Nil => Ok(false),
        Value::Boolean(value) => Ok(*value),
        _ => Err(LuaError::RuntimeError("includeExisting must be a boolean".to_owned())),
    }
}

fn settings_active(context: &ScriptInputContext) -> bool {
    matches!(
        context.mode(),
        ScriptExecutionMode::SettingsRender | ScriptExecutionMode::SettingsCallback
    )
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

fn callback_matches(callback: &RobloxLogCallback, line: &str) -> bool {
    callback.regex.as_ref().map_or_else(
        || contains_bytes(line.as_bytes(), &callback.pattern),
        |regex| regex.find(line).is_some(),
    )
}

struct DispatchFlag<'a>(&'a AtomicBool);

impl Drop for DispatchFlag<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn call_roblox_log_callback(
    lua: &Lua,
    context: &ScriptRobloxLogContext,
    callback: &RobloxLogCallback,
    line: &[u8],
) -> mlua::Result<()> {
    context.state.dispatching.store(true, Ordering::Release);
    let _flag = DispatchFlag(&context.state.dispatching);
    let function: Function = lua.registry_value(&callback.registry_key)?;
    function.call::<()>(lua.create_string(line)?)
}

fn dispatch_snapshot(
    lua: &Lua,
    execution: &Arc<ExecutionState>,
    context: &ScriptRobloxLogContext,
    snapshot: &RobloxLogSnapshot,
) -> mlua::Result<()> {
    let callbacks = context.callbacks();
    for line in &snapshot.lines {
        for callback in callbacks.iter() {
            if !callback_matches(callback, line) {
                continue;
            }
            call_roblox_log_callback(lua, context, callback, line.as_bytes())?;
            if execution.reason() != ScriptStopReason::None {
                return Ok(());
            }
        }
    }
    Ok(())
}

fn read_and_dispatch(
    lua: &Lua,
    execution: &Arc<ExecutionState>,
    context: &ScriptRobloxLogContext,
    source: Option<&Arc<dyn RobloxLogSource>>,
    include_existing: bool,
) -> mlua::Result<RobloxLogSnapshot> {
    if context.is_dispatching() {
        return Err(LuaError::RuntimeError(
            "cannot read Roblox logs while dispatching a Roblox log callback".to_owned(),
        ));
    }
    let snapshot =
        source.map_or_else(RobloxLogSnapshot::default, |source| source.read_log(include_existing));
    dispatch_snapshot(lua, execution, context, &snapshot)?;
    Ok(snapshot)
}

fn compile_regex(pattern: &[u8]) -> Result<Regex, LuaError> {
    let text = std::str::from_utf8(pattern)
        .map_err(|error| LuaError::RuntimeError(format!("invalid Roblox log regex: {error}")))?;
    Regex::new(text)
        .map_err(|error| LuaError::RuntimeError(format!("invalid Roblox log regex: {error}")))
}

fn register_callback(
    lua: &Lua,
    context: &ScriptRobloxLogContext,
    pattern_value: Value,
    function: Function,
    regex_value: Option<&Value>,
) -> mlua::Result<()> {
    if context.is_dispatching() {
        return Err(LuaError::RuntimeError(
            "cannot register Roblox log callbacks while dispatching a Roblox log callback"
                .to_owned(),
        ));
    }
    let Some(pattern) = lua.coerce_string(pattern_value)? else {
        return Err(LuaError::RuntimeError(
            "pattern must contain 1 to 1024 non-NUL bytes".to_owned(),
        ));
    };
    let pattern = pattern.as_bytes().to_vec();
    if pattern.is_empty() || pattern.len() > MAX_ROBLOX_LOG_PATTERN_BYTES || pattern.contains(&0) {
        return Err(LuaError::RuntimeError(format!(
            "pattern must contain 1 to {MAX_ROBLOX_LOG_PATTERN_BYTES} non-NUL bytes"
        )));
    }
    let use_regex = match regex_value {
        None | Some(Value::Nil) => false,
        Some(Value::Boolean(value)) => *value,
        Some(_) => {
            return Err(LuaError::RuntimeError("useRegex must be a boolean".to_owned()));
        }
    };
    let regex = if use_regex { Some(compile_regex(&pattern)?) } else { None };

    let mut callbacks = context.callbacks();
    if callbacks.len() >= MAX_ROBLOX_LOG_CALLBACKS {
        return Err(LuaError::RuntimeError(
            "script registered too many Roblox log callbacks".to_owned(),
        ));
    }
    let registry_key = lua.create_registry_value(function)?;
    callbacks.push(RobloxLogCallback { pattern, regex, registry_key });
    drop(callbacks);
    Ok(())
}

fn install_read_api(
    vm: &LuaScriptVm,
    context: &ScriptRobloxLogContext,
    source: Option<&Arc<dyn RobloxLogSource>>,
) -> mlua::Result<Function> {
    let read_context = context.clone();
    let read_source = source.cloned();
    let execution = Arc::clone(&vm.state);
    vm.lua.create_function(move |lua, arguments: MultiValue| {
        let include_existing = parse_include_existing(&arguments)?;
        let snapshot = read_and_dispatch(
            lua,
            &execution,
            &read_context,
            read_source.as_ref(),
            include_existing,
        )?;
        push_snapshot(lua, &snapshot)
    })
}

fn install_callback_api(
    vm: &LuaScriptVm,
    execution_context: &ScriptInputContext,
    log_context: &ScriptRobloxLogContext,
    source: Option<&Arc<dyn RobloxLogSource>>,
    table: &Table,
) -> mlua::Result<()> {
    let register_execution = execution_context.clone();
    let register_context = log_context.clone();
    table.set(
        "onLine",
        vm.lua.create_function(
            move |lua, (pattern, function, use_regex): (Value, Function, Option<Value>)| {
                if settings_active(&register_execution) {
                    return Err(LuaError::RuntimeError(
                        "robloxLog.onLine is not available inside script settings".to_owned(),
                    ));
                }
                if register_execution.mode() == ScriptExecutionMode::Cleanup {
                    return Err(LuaError::RuntimeError(
                        "robloxLog.onLine is not available inside onCleanup".to_owned(),
                    ));
                }
                register_callback(lua, &register_context, pattern, function, use_regex.as_ref())
            },
        )?,
    )?;

    let listen_execution_context = execution_context.clone();
    let listen_log_context = log_context.clone();
    let listen_source = source.cloned();
    let execution = Arc::clone(&vm.state);
    table.set(
        "listenUntilCancelled",
        vm.lua.create_function(move |lua, poll_value: Option<Value>| {
            if settings_active(&listen_execution_context) {
                return Err(LuaError::RuntimeError(
                    "robloxLog.listenUntilCancelled is not available inside script settings"
                        .to_owned(),
                ));
            }
            if listen_execution_context.mode() == ScriptExecutionMode::Cleanup {
                return Err(LuaError::RuntimeError(
                    "robloxLog.listenUntilCancelled is not available inside onCleanup".to_owned(),
                ));
            }
            if listen_log_context.is_dispatching() {
                return Err(LuaError::RuntimeError(
                    "cannot listen for Roblox logs while dispatching a Roblox log callback"
                        .to_owned(),
                ));
            }
            let poll_ms = match poll_value {
                None | Some(Value::Nil) => 50_i64,
                Some(value) => {
                    let Some(value) = lua.coerce_integer(value)? else {
                        return Err(LuaError::RuntimeError("pollMs must be an integer".to_owned()));
                    };
                    value.clamp(10, 1000)
                }
            };

            while execution.reason() == ScriptStopReason::None {
                let snapshot = listen_source
                    .as_ref()
                    .map_or_else(RobloxLogSnapshot::default, |source| source.read_log(false));
                dispatch_snapshot(lua, &execution, &listen_log_context, &snapshot)?;
                if execution.reason() != ScriptStopReason::None {
                    break;
                }
                execution.wait_coarse(Duration::from_millis(
                    u64::try_from(poll_ms).unwrap_or_default(),
                ))?;
            }
            Ok(())
        })?,
    )
}

/// Register `readRobloxLog` plus `robloxLog.read/onLine/listenUntilCancelled`.
///
/// # Errors
///
/// Returns a Lua registration error if the functions/table cannot be installed.
pub fn install_roblox_log_api(
    vm: &LuaScriptVm,
    execution_context: ScriptInputContext,
    log_context: ScriptRobloxLogContext,
    source: Option<Arc<dyn RobloxLogSource>>,
) -> Result<(), ScriptRobloxLogApiError> {
    let resources = (execution_context, log_context, source);
    let read = install_read_api(vm, &resources.1, resources.2.as_ref())?;
    vm.lua.globals().set("readRobloxLog", read.clone())?;
    let roblox_log = vm.lua.create_table()?;
    roblox_log.set("read", read)?;
    install_callback_api(vm, &resources.0, &resources.1, resources.2.as_ref(), &roblox_log)?;
    vm.lua.globals().set("robloxLog", roblox_log)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::{LuaExecutionErrorKind, MAX_SCRIPT_RUNTIME};
    use serde::Deserialize;
    use smu_roblox::RobloxLogServerInfo;
    use std::collections::VecDeque;

    #[derive(Debug)]
    struct FakeSource {
        calls: Mutex<Vec<bool>>,
        snapshots: Mutex<VecDeque<RobloxLogSnapshot>>,
    }

    impl FakeSource {
        fn repeating(snapshot: RobloxLogSnapshot) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                snapshots: Mutex::new(VecDeque::from([snapshot])),
            }
        }
    }

    impl RobloxLogSource for FakeSource {
        fn read_log(&self, include_existing: bool) -> RobloxLogSnapshot {
            self.calls.lock().expect("log calls lock").push(include_existing);
            let mut snapshots = self.snapshots.lock().expect("snapshot lock");
            if snapshots.len() > 1 {
                return snapshots.pop_front().unwrap_or_default();
            }
            snapshots.front().cloned().unwrap_or_default()
        }
    }

    fn snapshot_with_lines(lines: &[&str]) -> RobloxLogSnapshot {
        RobloxLogSnapshot {
            available: true,
            started_new_log: true,
            path: "/tmp/Player.log".into(),
            lines: lines.iter().map(|line| (*line).to_owned()).collect(),
            state: "in_game".to_owned(),
            place_id: u64::MAX,
            user_id: 123,
            universe_id: 456,
            job_id: "job".to_owned(),
            client_channel: "LIVE".to_owned(),
            client_roblox_git_hash: "client".to_owned(),
            server_roblox_git_hash: "server".to_owned(),
            server_prefix: "prefix".to_owned(),
            server_luau_version: "0.650".to_owned(),
            resolution_width: 1920,
            resolution_height: 1080,
            server: RobloxLogServerInfo {
                r#type: "rcc".to_owned(),
                rcc_address: "1.2.3.4".to_owned(),
                rcc_port: 53640,
                udmux_address: String::new(),
                udmux_port: 0,
            },
        }
    }

    fn install(
        vm: &LuaScriptVm,
        execution: &ScriptInputContext,
        context: &ScriptRobloxLogContext,
        source: Option<Arc<dyn RobloxLogSource>>,
    ) {
        install_roblox_log_api(vm, execution.clone(), context.clone(), source)
            .expect("Roblox log API should install");
    }

    #[test]
    fn snapshot_shape_matches_cpp_fields_and_signed_integer_cast() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let source =
            Arc::new(FakeSource::repeating(snapshot_with_lines(&["line one", "line two"])));
        install(
            &vm,
            &ScriptInputContext::default(),
            &ScriptRobloxLogContext::default(),
            Some(source.clone()),
        );
        let core = vm
            .eval::<(bool, bool, String, String, i64, i64, i64, String)>(
                concat!(
                    "local s=readRobloxLog(true); return s.available,s.startedNewLog,s.path,s.state,",
                    "s.placeId,s.userId,s.universeId,s.jobId"
                ),
                MAX_SCRIPT_RUNTIME,
            )
            .expect("core snapshot fields should be readable");
        assert_eq!(
            core,
            (
                true,
                true,
                "/tmp/Player.log".to_owned(),
                "in_game".to_owned(),
                -1,
                123,
                456,
                "job".to_owned(),
            )
        );
        let detail = vm
            .eval::<(u16, u16, String, String, u16, String, String)>(
                concat!(
                    "local s=readRobloxLog(false); return s.resolution.width,s.resolution.height,",
                    "s.server.type,s.server.rccAddress,s.server.rccPort,s.lines[1],s.lines[2]"
                ),
                MAX_SCRIPT_RUNTIME,
            )
            .expect("nested snapshot fields should be readable");
        assert_eq!(
            detail,
            (
                1920,
                1080,
                "rcc".to_owned(),
                "1.2.3.4".to_owned(),
                53640,
                "line one".to_owned(),
                "line two".to_owned(),
            )
        );
        assert_eq!(*source.calls.lock().expect("log calls lock"), [true, false]);
    }

    #[test]
    fn table_read_alias_uses_same_source() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let source = Arc::new(FakeSource::repeating(snapshot_with_lines(&[])));
        install(
            &vm,
            &ScriptInputContext::default(),
            &ScriptRobloxLogContext::default(),
            Some(source.clone()),
        );
        assert!(
            vm.eval::<bool>("return robloxLog.read().available", MAX_SCRIPT_RUNTIME)
                .expect("table read alias should work")
        );
        assert_eq!(*source.calls.lock().expect("log calls lock"), [false]);
    }

    #[test]
    fn argument_validation_matches_cpp() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        install(&vm, &ScriptInputContext::default(), &ScriptRobloxLogContext::default(), None);
        assert!(
            vm.execute_source("readRobloxLog(true, false)", "too-many.lua")
                .expect_err("too many arguments should fail")
                .message
                .contains("readRobloxLog accepts at most one argument")
        );
        assert!(
            vm.execute_source("readRobloxLog(1)", "type.lua")
                .expect_err("non-boolean includeExisting should fail")
                .message
                .contains("includeExisting must be a boolean")
        );
    }

    #[test]
    fn missing_source_returns_default_unavailable_snapshot() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        install(&vm, &ScriptInputContext::default(), &ScriptRobloxLogContext::default(), None);
        let values = vm
            .eval::<(bool, bool, String, String, usize)>(
                "local s=readRobloxLog(); return s.available,s.startedNewLog,s.path,s.state,#s.lines",
                MAX_SCRIPT_RUNTIME,
            )
            .expect("missing source should return default snapshot");
        assert_eq!(values, (false, false, String::new(), String::new(), 0));
    }

    #[test]
    fn literal_callbacks_dispatch_line_major_in_registration_order() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let context = ScriptRobloxLogContext::default();
        let source = Arc::new(FakeSource::repeating(snapshot_with_lines(&["alpha beta", "beta"])));
        install(&vm, &ScriptInputContext::default(), &context, Some(source));
        vm.execute_source(
            concat!(
                "events={}; ",
                "robloxLog.onLine('alpha', function(line) table.insert(events, 'A:'..line) end); ",
                "robloxLog.onLine('beta', function(line) table.insert(events, 'B:'..line) end); ",
                "readRobloxLog()"
            ),
            "callbacks.lua",
        )
        .expect("literal callbacks should dispatch");
        assert_eq!(
            vm.eval::<(String, String, String)>(
                "return events[1],events[2],events[3]",
                MAX_SCRIPT_RUNTIME,
            )
            .expect("events should be recorded"),
            ("A:alpha beta".to_owned(), "B:alpha beta".to_owned(), "B:beta".to_owned(),)
        );
        assert_eq!(context.callback_count(), 2);
    }

    #[derive(Debug, Deserialize)]
    struct RegexFixture {
        cases: Vec<RegexCase>,
    }

    #[derive(Debug, Deserialize)]
    struct RegexCase {
        name: String,
        pattern: String,
        text: String,
        valid: bool,
        matches: bool,
    }

    #[test]
    fn ecmascript_regex_corpus_matches_cpp_std_regex_oracle() {
        let fixture: RegexFixture = serde_json::from_str(include_str!(
            "../../../../compat/scripting/roblox_regex_cases.json"
        ))
        .expect("C++ Roblox regex fixture should parse");
        for case in fixture.cases {
            match Regex::new(&case.pattern) {
                Ok(regex) => {
                    assert!(case.valid, "case {} should be invalid in C++", case.name);
                    assert_eq!(
                        regex.find(&case.text).is_some(),
                        case.matches,
                        "case {} match result",
                        case.name
                    );
                }
                Err(_) => assert!(!case.valid, "case {} should compile like C++", case.name),
            }
        }
    }

    #[test]
    fn regex_callbacks_match_and_invalid_regex_is_rejected() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let source = Arc::new(FakeSource::repeating(snapshot_with_lines(&["server 123"])));
        install(
            &vm,
            &ScriptInputContext::default(),
            &ScriptRobloxLogContext::default(),
            Some(source),
        );
        assert!(
            vm.eval::<bool>(
                concat!(
                    "matched=false; robloxLog.onLine('server [0-9]+', function() matched=true end, true); ",
                    "readRobloxLog(); return matched"
                ),
                MAX_SCRIPT_RUNTIME,
            )
            .expect("regex callback should dispatch")
        );
        assert!(
            vm.execute_source("robloxLog.onLine('[', function() end, true)", "bad-regex.lua",)
                .expect_err("invalid regex should fail")
                .message
                .contains("invalid Roblox log regex:")
        );
    }

    #[test]
    fn callback_reentrancy_guards_match_cpp() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let source = Arc::new(FakeSource::repeating(snapshot_with_lines(&["trigger"])));
        install(
            &vm,
            &ScriptInputContext::default(),
            &ScriptRobloxLogContext::default(),
            Some(source),
        );
        let read_error = vm
            .execute_source(
                "robloxLog.onLine('trigger', function() readRobloxLog() end); readRobloxLog()",
                "reentrant-read.lua",
            )
            .expect_err("callback read must fail");
        assert!(
            read_error
                .message
                .contains("cannot read Roblox logs while dispatching a Roblox log callback")
        );
    }

    #[test]
    fn settings_cleanup_and_pattern_limits_match_cpp() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let execution = ScriptInputContext::default();
        let context = ScriptRobloxLogContext::default();
        install(&vm, &execution, &context, None);

        execution.set_mode(ScriptExecutionMode::SettingsCallback);
        assert!(
            vm.execute_source("robloxLog.onLine('x', function() end)", "settings.lua")
                .expect_err("settings callback should reject registration")
                .message
                .contains("robloxLog.onLine is not available inside script settings")
        );
        execution.set_mode(ScriptExecutionMode::Cleanup);
        assert!(
            vm.execute_source("robloxLog.onLine('x', function() end)", "cleanup.lua")
                .expect_err("cleanup should reject registration")
                .message
                .contains("robloxLog.onLine is not available inside onCleanup")
        );
        execution.set_mode(ScriptExecutionMode::Normal);
        assert!(
            vm.execute_source("robloxLog.onLine('', function() end)", "empty.lua")
                .expect_err("empty pattern should fail")
                .message
                .contains("pattern must contain 1 to 1024 non-NUL bytes")
        );
        assert!(
            vm.execute_source(
                "robloxLog.onLine(string.rep('x',1025), function() end)",
                "long.lua",
            )
            .expect_err("long pattern should fail")
            .message
            .contains("pattern must contain 1 to 1024 non-NUL bytes")
        );
    }

    #[test]
    fn callback_limit_is_128() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let context = ScriptRobloxLogContext::default();
        install(&vm, &ScriptInputContext::default(), &context, None);
        vm.execute_source("for i=1,128 do robloxLog.onLine('x', function() end) end", "limit.lua")
            .expect("128 callbacks should register");
        assert_eq!(context.callback_count(), 128);
        assert!(
            vm.execute_source("robloxLog.onLine('x', function() end)", "overflow.lua")
                .expect_err("129th callback should fail")
                .message
                .contains("script registered too many Roblox log callbacks")
        );
        context.clear_callbacks(&vm);
        assert_eq!(context.callback_count(), 0);
    }

    #[test]
    fn listen_until_cancelled_polls_and_dispatches_before_wait() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let source = Arc::new(FakeSource::repeating(snapshot_with_lines(&["tick"])));
        install(
            &vm,
            &ScriptInputContext::default(),
            &ScriptRobloxLogContext::default(),
            Some(source.clone()),
        );
        vm.execute_source(
            concat!(
                "count=0; robloxLog.onLine('tick', function() count=count+1; ",
                "if count >= 1 then error('done') end end)"
            ),
            "register.lua",
        )
        .expect("listener callback should register");
        let error = vm
            .execute_source("robloxLog.listenUntilCancelled(10)", "listen.lua")
            .expect_err("callback failure should stop listener");
        assert!(error.message.contains("done"));
        assert_eq!(
            vm.eval::<i64>("return count", MAX_SCRIPT_RUNTIME).expect("count should be available"),
            1
        );
        assert_eq!(*source.calls.lock().expect("log calls lock"), [false]);
    }

    #[test]
    fn listener_cancellation_uses_script_stop_reason() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let source = Arc::new(FakeSource::repeating(snapshot_with_lines(&[])));
        install(
            &vm,
            &ScriptInputContext::default(),
            &ScriptRobloxLogContext::default(),
            Some(source),
        );
        let stop = vm.stop_handle();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(5));
            stop.request_cancel();
        });
        let error = vm
            .execute_source("robloxLog.listenUntilCancelled(1000)", "cancel-listen.lua")
            .expect_err("cancellation should stop listener");
        thread.join().expect("cancel thread should finish");
        assert_eq!(error.kind, LuaExecutionErrorKind::Cancelled);
    }
}

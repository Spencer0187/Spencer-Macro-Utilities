//! Roblox player-log Lua API adapter.

use crate::lua_runtime::LuaScriptVm;
use mlua::{Error as LuaError, Lua, MultiValue, Table, Value};
use smu_roblox::{RobloxLogSnapshot, RobloxLogSource};
use std::fmt;
use std::path::Path;
use std::sync::Arc;

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

/// Register `readRobloxLog` and `robloxLog.read` against the modular Roblox log service.
///
/// # Errors
///
/// Returns a Lua registration error if the functions/table cannot be installed.
pub fn install_roblox_log_api(
    vm: &LuaScriptVm,
    source: Option<Arc<dyn RobloxLogSource>>,
) -> Result<(), ScriptRobloxLogApiError> {
    let read_source = source;
    let read = vm.lua.create_function(move |lua, arguments: MultiValue| {
        let include_existing = parse_include_existing(&arguments)?;
        let snapshot = read_source
            .as_ref()
            .map_or_else(RobloxLogSnapshot::default, |source| source.read_log(include_existing));
        push_snapshot(lua, &snapshot)
    })?;

    vm.lua.globals().set("readRobloxLog", read.clone())?;
    let roblox_log = vm.lua.create_table()?;
    roblox_log.set("read", read)?;
    vm.lua.globals().set("robloxLog", roblox_log)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::MAX_SCRIPT_RUNTIME;
    use smu_roblox::RobloxLogServerInfo;
    use std::sync::Mutex;

    #[derive(Debug)]
    struct FakeSource {
        calls: Mutex<Vec<bool>>,
        snapshot: RobloxLogSnapshot,
    }

    impl RobloxLogSource for FakeSource {
        fn read_log(&self, include_existing: bool) -> RobloxLogSnapshot {
            self.calls.lock().expect("log calls lock").push(include_existing);
            self.snapshot.clone()
        }
    }

    fn snapshot() -> RobloxLogSnapshot {
        RobloxLogSnapshot {
            available: true,
            started_new_log: true,
            path: "/tmp/Player.log".into(),
            lines: vec!["line one".to_owned(), "line two".to_owned()],
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

    #[test]
    fn snapshot_shape_matches_cpp_fields_and_signed_integer_cast() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let source = Arc::new(FakeSource { calls: Mutex::new(Vec::new()), snapshot: snapshot() });
        install_roblox_log_api(&vm, Some(source.clone())).expect("Roblox log API should install");
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
        let source = Arc::new(FakeSource { calls: Mutex::new(Vec::new()), snapshot: snapshot() });
        install_roblox_log_api(&vm, Some(source.clone())).expect("Roblox log API should install");
        assert!(
            vm.eval::<bool>("return robloxLog.read().available", MAX_SCRIPT_RUNTIME)
                .expect("table read alias should work")
        );
        assert_eq!(*source.calls.lock().expect("log calls lock"), [false]);
    }

    #[test]
    fn argument_validation_matches_cpp() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        install_roblox_log_api(&vm, None).expect("Roblox log API should install");
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
        install_roblox_log_api(&vm, None).expect("Roblox log API should install");
        let values = vm
            .eval::<(bool, bool, String, String, usize)>(
                "local s=readRobloxLog(); return s.available,s.startedNewLog,s.path,s.state,#s.lines",
                MAX_SCRIPT_RUNTIME,
            )
            .expect("missing source should return default snapshot");
        assert_eq!(values, (false, false, String::new(), String::new(), 0));
    }
}

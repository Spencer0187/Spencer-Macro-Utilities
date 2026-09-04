//! Restricted Lua 5.4 runtime for imported scripts.
//!
//! This is the VM/lifecycle layer only. Platform-sensitive SMU APIs are registered through separate
//! service adapters so the Lua state itself never reaches application globals.

use crate::memory_limit_bytes;
use mlua::chunk::ChunkMode;
use mlua::{Error as LuaError, HookTriggers, Lua, LuaOptions, StdLib, Value, VmState};
use std::fmt;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const MAX_SCRIPT_RUNTIME: Duration = Duration::from_secs(30);
pub const MAX_SETTINGS_RUNTIME: Duration = Duration::from_secs(5);
pub const MAX_CLEANUP_RUNTIME: Duration = Duration::from_secs(2);
pub const LUA_HOOK_INSTRUCTION_COUNT: u32 = 10_000;
pub const MAX_SINGLE_SLEEP_MICROS: i64 = 24 * 60 * 60 * 1_000_000;
pub const DEFAULT_SHOULD_YIELD_MICROS: i64 = 1_000;
const PRECISE_SLEEP_SPIN_TAIL: Duration = Duration::from_micros(600);
const LONG_WAIT_SLICE: Duration = Duration::from_hours(24);

const SAFE_WRAPPERS: &str = r"
local raw_pcall = pcall
local raw_xpcall = xpcall
local raw_resume = coroutine.resume
local raw_create = coroutine.create
local raw_pack = table.pack
local raw_unpack = table.unpack
local check_stop = __smu_check_stop
_G.__smu_check_stop = nil

pcall = function(...)
    local results = raw_pack(raw_pcall(...))
    check_stop()
    return raw_unpack(results, 1, results.n)
end

xpcall = function(...)
    local results = raw_pack(raw_xpcall(...))
    check_stop()
    return raw_unpack(results, 1, results.n)
end

coroutine.resume = function(...)
    local results = raw_pack(raw_resume(...))
    check_stop()
    return raw_unpack(results, 1, results.n)
end

coroutine.wrap = function(fn)
    local co = raw_create(fn)
    return function(...)
        local results = raw_pack(coroutine.resume(co, ...))
        if not results[1] then
            error(results[2], 2)
        end
        return raw_unpack(results, 2, results.n)
    end
end
";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum ScriptStopReason {
    #[default]
    None = 0,
    Timeout = 1,
    Cancelled = 2,
}

impl ScriptStopReason {
    const fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Timeout,
            2 => Self::Cancelled,
            _ => Self::None,
        }
    }

    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::Timeout => "script execution timed out",
            Self::Cancelled => "script execution cancelled",
            Self::None => "script execution failed",
        }
    }
}

#[derive(Debug)]
struct BudgetData {
    active: bool,
    active_started: Instant,
    deadline: Option<Instant>,
    limit: Duration,
}

impl Default for BudgetData {
    fn default() -> Self {
        Self {
            active: false,
            active_started: Instant::now(),
            deadline: None,
            limit: MAX_SCRIPT_RUNTIME,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct ExecutionState {
    reason: AtomicU8,
    budget: Mutex<BudgetData>,
    wake: Condvar,
}

impl ExecutionState {
    fn lock_budget(&self) -> MutexGuard<'_, BudgetData> {
        match self.budget.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    pub(crate) fn reason(&self) -> ScriptStopReason {
        ScriptStopReason::from_u8(self.reason.load(Ordering::Acquire))
    }

    pub(crate) fn set_reason(&self, reason: ScriptStopReason) {
        let _ = self.reason.compare_exchange(
            ScriptStopReason::None as u8,
            reason as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        self.wake.notify_all();
    }

    fn begin(&self, limit: Duration) {
        self.reason.store(ScriptStopReason::None as u8, Ordering::Release);
        let now = Instant::now();
        let mut budget = self.lock_budget();
        budget.active = true;
        budget.active_started = now;
        budget.limit = limit;
        budget.deadline = now.checked_add(limit);
    }

    fn finish(&self) {
        let _ = self.check_deadline();
        let mut budget = self.lock_budget();
        budget.active = false;
        budget.deadline = None;
    }

    pub(crate) fn check_deadline(&self) -> Result<(), LuaError> {
        let reason = self.reason();
        if reason != ScriptStopReason::None {
            return Err(LuaError::RuntimeError(reason.message().to_owned()));
        }

        let timed_out = {
            let budget = self.lock_budget();
            budget.active && budget.deadline.is_some_and(|deadline| Instant::now() > deadline)
        };
        if timed_out {
            self.set_reason(ScriptStopReason::Timeout);
            return Err(LuaError::RuntimeError(ScriptStopReason::Timeout.message().to_owned()));
        }
        Ok(())
    }

    fn pause(&self) -> Result<(), LuaError> {
        self.check_deadline()?;
        let mut budget = self.lock_budget();
        budget.active = false;
        budget.deadline = None;
        drop(budget);
        Ok(())
    }

    fn resume(&self) -> Result<(), LuaError> {
        let reason = self.reason();
        if reason != ScriptStopReason::None {
            return Err(LuaError::RuntimeError(reason.message().to_owned()));
        }
        let now = Instant::now();
        let mut budget = self.lock_budget();
        let limit = if budget.limit.is_zero() { MAX_SCRIPT_RUNTIME } else { budget.limit };
        budget.active = true;
        budget.active_started = now;
        budget.deadline = now.checked_add(limit);
        drop(budget);
        Ok(())
    }

    fn should_yield(&self, threshold: Duration) -> bool {
        if self.reason() != ScriptStopReason::None {
            return true;
        }
        let budget = self.lock_budget();
        budget.active
            && Instant::now().saturating_duration_since(budget.active_started) >= threshold
    }

    pub(crate) fn wait_coarse(&self, duration: Duration) -> Result<(), LuaError> {
        self.pause()?;
        let wake_time = Instant::now().checked_add(duration);
        let mut guard = self.lock_budget();
        loop {
            let reason = self.reason();
            if reason != ScriptStopReason::None {
                return Err(LuaError::RuntimeError(reason.message().to_owned()));
            }

            let wait_duration = match wake_time {
                Some(target) => {
                    let remaining = target.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        drop(guard);
                        return self.resume();
                    }
                    remaining
                }
                None => LONG_WAIT_SLICE,
            };

            let waited = self.wake.wait_timeout(guard, wait_duration);
            let (next_guard, _) = match waited {
                Ok(pair) => pair,
                Err(poisoned) => poisoned.into_inner(),
            };
            guard = next_guard;
        }
    }

    fn wait_precise(&self, duration: Duration) -> Result<(), LuaError> {
        self.pause()?;
        let start = Instant::now();
        let target = start.checked_add(duration);
        let Some(target) = target else {
            return self.wait_coarse(duration);
        };

        if duration > PRECISE_SLEEP_SPIN_TAIL {
            let coarse_target = target.checked_sub(PRECISE_SLEEP_SPIN_TAIL).unwrap_or(start);
            let mut guard = self.lock_budget();
            loop {
                let reason = self.reason();
                if reason != ScriptStopReason::None {
                    return Err(LuaError::RuntimeError(reason.message().to_owned()));
                }
                let remaining = coarse_target.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    drop(guard);
                    break;
                }
                let waited = self.wake.wait_timeout(guard, remaining);
                let (next_guard, _) = match waited {
                    Ok(pair) => pair,
                    Err(poisoned) => poisoned.into_inner(),
                };
                guard = next_guard;
            }
        }

        while Instant::now() < target {
            let reason = self.reason();
            if reason != ScriptStopReason::None {
                return Err(LuaError::RuntimeError(reason.message().to_owned()));
            }
            std::hint::spin_loop();
        }
        self.resume()
    }
}

#[derive(Clone, Debug)]
pub struct LuaStopHandle {
    state: Arc<ExecutionState>,
}

impl LuaStopHandle {
    pub fn request_cancel(&self) {
        self.state.set_reason(ScriptStopReason::Cancelled);
    }

    #[must_use]
    pub fn reason(&self) -> ScriptStopReason {
        self.state.reason()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LuaExecutionErrorKind {
    Timeout,
    Cancelled,
    Memory,
    Syntax,
    Runtime,
    Initialization,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LuaExecutionError {
    pub kind: LuaExecutionErrorKind,
    pub message: String,
}

impl LuaExecutionError {
    fn new(kind: LuaExecutionErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into() }
    }
}

impl fmt::Display for LuaExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for LuaExecutionError {}

fn map_lua_error(error: LuaError, reason: ScriptStopReason) -> LuaExecutionError {
    match reason {
        ScriptStopReason::Timeout => LuaExecutionError::new(
            LuaExecutionErrorKind::Timeout,
            ScriptStopReason::Timeout.message(),
        ),
        ScriptStopReason::Cancelled => LuaExecutionError::new(
            LuaExecutionErrorKind::Cancelled,
            ScriptStopReason::Cancelled.message(),
        ),
        ScriptStopReason::None => match error {
            LuaError::MemoryError(message) => {
                LuaExecutionError::new(LuaExecutionErrorKind::Memory, message)
            }
            LuaError::SyntaxError { message, .. } => {
                LuaExecutionError::new(LuaExecutionErrorKind::Syntax, message)
            }
            other => LuaExecutionError::new(LuaExecutionErrorKind::Runtime, other.to_string()),
        },
    }
}

fn checked_integer(
    lua: &Lua,
    value: Value,
    minimum: i64,
    maximum: i64,
    name: &str,
) -> mlua::Result<i64> {
    let Some(value) = lua.coerce_integer(value)? else {
        return Err(LuaError::RuntimeError(format!("{name} must be an integer")));
    };
    Ok(value.clamp(minimum, maximum))
}

fn monotonic_micros() -> i64 {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    let elapsed = ORIGIN.get_or_init(Instant::now).elapsed().as_micros();
    i64::try_from(elapsed).unwrap_or(i64::MAX)
}

fn unix_timestamp_seconds() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
        Err(error) => {
            let seconds = i64::try_from(error.duration().as_secs()).unwrap_or(i64::MAX);
            seconds.saturating_neg()
        }
    }
}

fn install_foundation_api(lua: &Lua, state: &Arc<ExecutionState>) -> mlua::Result<()> {
    let globals = lua.globals();

    globals.set("nowMicros", lua.create_function(|_, ()| Ok(monotonic_micros()))?)?;
    globals.set("getUnixTimestamp", lua.create_function(|_, ()| Ok(unix_timestamp_seconds()))?)?;

    let check_state = Arc::clone(state);
    globals
        .set("__smu_check_stop", lua.create_function(move |_, ()| check_state.check_deadline())?)?;

    let cancelled_state = Arc::clone(state);
    globals.set(
        "isCancelled",
        lua.create_function(move |_, ()| Ok(cancelled_state.reason() != ScriptStopReason::None))?,
    )?;

    let throw_state = Arc::clone(state);
    globals
        .set("throwIfCancelled", lua.create_function(move |_, ()| throw_state.check_deadline())?)?;

    let checkpoint_state = Arc::clone(state);
    globals.set(
        "checkpoint",
        lua.create_function(move |_, ()| {
            checkpoint_state.check_deadline()?;
            checkpoint_state.pause()?;
            std::thread::yield_now();
            checkpoint_state.resume()
        })?,
    )?;

    let yield_state = Arc::clone(state);
    globals.set(
        "shouldYield",
        lua.create_function(move |lua, threshold: Option<Value>| {
            let micros = match threshold {
                Some(Value::Nil) | None => DEFAULT_SHOULD_YIELD_MICROS,
                Some(value) => {
                    checked_integer(lua, value, 0, MAX_SINGLE_SLEEP_MICROS, "threshold")?
                }
            };
            Ok(yield_state
                .should_yield(Duration::from_micros(u64::try_from(micros).unwrap_or_default())))
        })?,
    )?;

    let sleep_state = Arc::clone(state);
    globals.set(
        "sleep",
        lua.create_function(move |lua, value: Value| {
            sleep_state.check_deadline()?;
            let millis = checked_integer(lua, value, 0, i64::MAX, "sleep duration")?;
            sleep_state
                .wait_coarse(Duration::from_millis(u64::try_from(millis).unwrap_or(u64::MAX)))
        })?,
    )?;

    let sleep_micros_state = Arc::clone(state);
    globals.set(
        "sleepMicros",
        lua.create_function(move |lua, value: Value| {
            sleep_micros_state.check_deadline()?;
            let micros = checked_integer(lua, value, 0, MAX_SINGLE_SLEEP_MICROS, "sleep duration")?;
            sleep_micros_state
                .wait_precise(Duration::from_micros(u64::try_from(micros).unwrap_or_default()))
        })?,
    )?;

    let until_state = Arc::clone(state);
    globals.set(
        "sleepUntilCancelled",
        lua.create_function(move |lua, value: Value| {
            let millis = checked_integer(lua, value, 0, i64::MAX, "sleep duration")?;
            if until_state.reason() != ScriptStopReason::None {
                return Ok(true);
            }
            match until_state
                .wait_coarse(Duration::from_millis(u64::try_from(millis).unwrap_or(u64::MAX)))
            {
                Ok(()) => Ok(false),
                Err(_) => Ok(true),
            }
        })?,
    )?;

    Ok(())
}

fn restrict_base_library(lua: &Lua) -> mlua::Result<()> {
    let globals = lua.globals();
    for name in ["dofile", "loadfile", "load", "collectgarbage"] {
        globals.set(name, Value::Nil)?;
    }
    Ok(())
}

fn install_safe_wrappers(lua: &Lua) -> mlua::Result<()> {
    lua.load(SAFE_WRAPPERS).set_name("=smu-safe-runtime-wrappers").exec()
}

pub struct LuaScriptVm {
    pub(crate) lua: Lua,
    pub(crate) state: Arc<ExecutionState>,
    memory_limit_bytes: usize,
}

impl fmt::Debug for LuaScriptVm {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LuaScriptVm")
            .field("stop_reason", &self.state.reason())
            .field("memory_limit_bytes", &self.memory_limit_bytes)
            .finish_non_exhaustive()
    }
}

impl LuaScriptVm {
    /// Create the same restricted library set as the C++ `ScriptInstance`.
    ///
    /// # Errors
    ///
    /// Returns an initialization error if Lua cannot create/configure the state.
    pub fn new(requested_memory_limit_mb: Option<usize>) -> Result<Self, LuaExecutionError> {
        let libraries =
            StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8 | StdLib::COROUTINE;
        let lua = Lua::new_with(libraries, LuaOptions::new()).map_err(|error| {
            LuaExecutionError::new(LuaExecutionErrorKind::Initialization, error.to_string())
        })?;
        let memory_limit_bytes = memory_limit_bytes(requested_memory_limit_mb);
        lua.set_memory_limit(memory_limit_bytes).map_err(|error| {
            LuaExecutionError::new(LuaExecutionErrorKind::Initialization, error.to_string())
        })?;

        restrict_base_library(&lua).map_err(|error| {
            LuaExecutionError::new(LuaExecutionErrorKind::Initialization, error.to_string())
        })?;

        let state = Arc::new(ExecutionState::default());
        let hook_state = Arc::clone(&state);
        lua.set_global_hook(
            HookTriggers::new().every_nth_instruction(LUA_HOOK_INSTRUCTION_COUNT),
            move |_, _| {
                hook_state.check_deadline()?;
                Ok(VmState::Continue)
            },
        )
        .map_err(|error| {
            LuaExecutionError::new(LuaExecutionErrorKind::Initialization, error.to_string())
        })?;

        install_foundation_api(&lua, &state).map_err(|error| {
            LuaExecutionError::new(LuaExecutionErrorKind::Initialization, error.to_string())
        })?;
        install_safe_wrappers(&lua).map_err(|error| {
            LuaExecutionError::new(LuaExecutionErrorKind::Initialization, error.to_string())
        })?;

        Ok(Self { lua, state, memory_limit_bytes })
    }

    #[must_use]
    pub fn stop_handle(&self) -> LuaStopHandle {
        LuaStopHandle { state: Arc::clone(&self.state) }
    }

    #[must_use]
    pub const fn memory_limit_bytes(&self) -> usize {
        self.memory_limit_bytes
    }

    #[must_use]
    pub fn memory_used_bytes(&self) -> usize {
        self.lua.used_memory()
    }

    #[must_use]
    pub fn has_function(&self, name: &str) -> bool {
        self.lua.globals().get::<Value>(name).is_ok_and(|value| matches!(value, Value::Function(_)))
    }

    fn run_budgeted<T>(
        &self,
        limit: Duration,
        operation: impl FnOnce(&Lua) -> mlua::Result<T>,
    ) -> Result<T, LuaExecutionError> {
        self.state.begin(limit);
        let result = operation(&self.lua);
        self.state.finish();
        let reason = self.state.reason();
        match result {
            Ok(value) if reason == ScriptStopReason::None => Ok(value),
            Ok(_) => {
                Err(map_lua_error(LuaError::RuntimeError(reason.message().to_owned()), reason))
            }
            Err(error) => Err(map_lua_error(error, reason)),
        }
    }

    /// Execute raw source bytes while explicitly rejecting Lua bytecode chunks.
    ///
    /// # Errors
    ///
    /// Returns syntax, runtime, memory, cancellation, or timeout failures.
    pub fn execute_text_bytes_with_limit(
        &self,
        source: &[u8],
        chunk_name: &str,
        limit: Duration,
    ) -> Result<(), LuaExecutionError> {
        self.run_budgeted(limit, |lua| {
            lua.load(source).set_name(chunk_name).set_mode(ChunkMode::Text).exec()
        })
    }

    /// Execute a plain-text Lua chunk under the normal 30-second script budget.
    ///
    /// # Errors
    ///
    /// Returns syntax, runtime, memory, cancellation, or timeout failures.
    pub fn execute_source(&self, source: &str, chunk_name: &str) -> Result<(), LuaExecutionError> {
        self.execute_source_with_limit(source, chunk_name, MAX_SCRIPT_RUNTIME)
    }

    /// Execute a Lua chunk with an explicit budget. Exposed for compatibility tests and settings / cleanup callers.
    ///
    /// # Errors
    ///
    /// Returns syntax, runtime, memory, cancellation, or timeout failures.
    pub fn execute_source_with_limit(
        &self,
        source: &str,
        chunk_name: &str,
        limit: Duration,
    ) -> Result<(), LuaExecutionError> {
        self.run_budgeted(limit, |lua| lua.load(source).set_name(chunk_name).exec())
    }

    /// Call a global Lua function with no arguments.
    ///
    /// # Errors
    ///
    /// Returns an error when the global is missing/not callable or its execution fails.
    pub fn call_no_args(&self, name: &str, limit: Duration) -> Result<(), LuaExecutionError> {
        self.run_budgeted(limit, |lua| {
            let function = lua.globals().get::<mlua::Function>(name)?;
            function.call::<()>(())
        })
    }

    /// Call `onCleanup(reason)` under the historical two-second cleanup budget.
    ///
    /// # Errors
    ///
    /// Returns an error when the callback is missing/not callable or its execution fails.
    pub fn call_cleanup(&self, reason: &str) -> Result<(), LuaExecutionError> {
        self.run_budgeted(MAX_CLEANUP_RUNTIME, |lua| {
            let function = lua.globals().get::<mlua::Function>("onCleanup")?;
            function.call::<()>(reason)
        })
    }

    /// Evaluate a small expression/value under a supplied budget. Primarily useful for service integration tests.
    ///
    /// # Errors
    ///
    /// Propagates Lua execution failures.
    pub fn eval<T: mlua::FromLuaMulti>(
        &self,
        source: &str,
        limit: Duration,
    ) -> Result<T, LuaExecutionError> {
        self.run_budgeted(limit, |lua| lua.load(source).eval::<T>())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn restricted_libraries_match_cpp_surface() {
        let vm = LuaScriptVm::new(None).expect("restricted Lua VM should initialize");
        let values: (String, String, String, String, String, String, String, String, String) = vm
            .eval(
                "return type(table), type(string), type(math), type(utf8), type(coroutine), type(io), type(os), type(package), type(debug)",
                Duration::from_secs(1),
            )
            .expect("library type query should run");
        assert_eq!(
            values,
            (
                "table".to_owned(),
                "table".to_owned(),
                "table".to_owned(),
                "table".to_owned(),
                "table".to_owned(),
                "nil".to_owned(),
                "nil".to_owned(),
                "nil".to_owned(),
                "nil".to_owned(),
            )
        );
        let removed: (String, String, String, String) = vm
            .eval(
                "return type(dofile), type(loadfile), type(load), type(collectgarbage)",
                Duration::from_secs(1),
            )
            .expect("removed base API query should run");
        assert_eq!(
            removed,
            ("nil".to_owned(), "nil".to_owned(), "nil".to_owned(), "nil".to_owned(),)
        );
    }

    #[test]
    fn ordinary_script_chunk_and_callback_execute() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        vm.execute_source("value = 40; function onExecute() value = value + 2 end", "test.lua")
            .expect("script should load");
        assert!(vm.has_function("onExecute"));
        vm.call_no_args("onExecute", Duration::from_secs(1)).expect("callback should run");
        assert_eq!(vm.eval::<i64>("return value", Duration::from_secs(1)), Ok(42));
    }

    #[test]
    fn instruction_hook_stops_infinite_loop() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let error = vm
            .execute_source_with_limit("while true do end", "timeout.lua", Duration::from_millis(5))
            .expect_err("infinite loop should time out");
        assert_eq!(error.kind, LuaExecutionErrorKind::Timeout);
        assert_eq!(error.message, "script execution timed out");
    }

    #[test]
    fn pcall_cannot_swallow_timeout() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let error = vm
            .execute_source_with_limit(
                "local ok = pcall(function() while true do end end); survived = ok",
                "pcall-timeout.lua",
                Duration::from_millis(5),
            )
            .expect_err("pcall must not swallow SMU timeout");
        assert_eq!(error.kind, LuaExecutionErrorKind::Timeout);
    }

    #[test]
    fn coroutine_resume_cannot_swallow_timeout() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let error = vm
            .execute_source_with_limit(
                "local co = coroutine.create(function() while true do end end); local ok = coroutine.resume(co); survived = ok",
                "coroutine-timeout.lua",
                Duration::from_millis(5),
            )
            .expect_err("coroutine.resume must not swallow SMU timeout");
        assert_eq!(error.kind, LuaExecutionErrorKind::Timeout);
    }

    #[test]
    fn memory_limit_is_enforced_by_vm_allocator() {
        let vm = LuaScriptVm::new(Some(16)).expect("Lua VM should initialize");
        let error = vm
            .execute_source(
                "local giant = string.rep('x', 20 * 1024 * 1024); return #giant",
                "memory.lua",
            )
            .expect_err("allocation above 16 MiB budget should fail");
        assert_eq!(error.kind, LuaExecutionErrorKind::Memory);
    }

    #[test]
    fn sleep_pauses_and_resets_execution_budget_like_cpp() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        vm.execute_source_with_limit(
            "local start = nowMicros(); sleep(10); checkpoint(); elapsed = nowMicros() - start",
            "sleep.lua",
            Duration::from_millis(5),
        )
        .expect("sleep/checkpoint time must not consume the active execution budget");
        let elapsed = vm
            .eval::<i64>("return elapsed", Duration::from_secs(1))
            .expect("elapsed value should exist");
        assert!(elapsed >= 10_000);
    }

    #[test]
    fn stop_handle_interrupts_blocking_sleep() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let stop = vm.stop_handle();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            stop.request_cancel();
        });
        let error = vm
            .execute_source("sleep(5000)", "cancel.lua")
            .expect_err("cancel should interrupt sleep");
        thread.join().expect("cancellation thread should finish");
        assert_eq!(error.kind, LuaExecutionErrorKind::Cancelled);
    }

    #[test]
    fn numeric_strings_follow_lua_tointegerx_compatibility() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        vm.execute_source("sleep('0'); sleepMicros('-5')", "coercion.lua")
            .expect("numeric strings should coerce and negative sleep should clamp to zero");
        let error = vm
            .execute_source("sleep('not-a-number')", "bad-coercion.lua")
            .expect_err("non-numeric string must fail");
        assert!(error.message.contains("sleep duration must be an integer"));
    }
}

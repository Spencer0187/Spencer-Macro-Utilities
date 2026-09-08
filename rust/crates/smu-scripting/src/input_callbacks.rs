//! Script input callback and hotkey-mirroring API.

use crate::input_api::{
    ScriptInputContext, check_cleanup_start, check_managed_target, check_settings_callback,
    dispatch_actions, hotkey_from_value, parse_strict_text, strict_option,
};
use crate::lua_runtime::{ExecutionState, LuaScriptVm, ScriptStopReason};
use mlua::{Error as LuaError, Function, Lua, RegistryKey, Table, Value};
use smu_core::Hotkey;
use smu_input::{HotkeyMatch, InputDevice, is_hotkey_pressed};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

pub const MAX_INPUT_CALLBACKS: usize = 128;
pub const DEFAULT_INPUT_CALLBACK_SCAN_MS: i64 = 2;
pub const MIN_INPUT_CALLBACK_SCAN_MS: i64 = 1;
pub const MAX_INPUT_CALLBACK_SCAN_MS: i64 = 50;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HotkeyCallbackKind {
    Pressed,
    Released,
    Changed,
}

struct HotkeyCallback {
    kind: HotkeyCallbackKind,
    hotkey: Hotkey,
    registry_key: RegistryKey,
    strict: bool,
    initialized: bool,
    last_down: bool,
}

#[derive(Default)]
struct CallbackState {
    dispatching: AtomicBool,
    callbacks: Mutex<Vec<HotkeyCallback>>,
}

#[derive(Clone, Default)]
pub struct ScriptInputCallbackContext {
    state: Arc<CallbackState>,
}

impl fmt::Debug for ScriptInputCallbackContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScriptInputCallbackContext")
            .field("dispatching", &self.is_dispatching())
            .field("callback_count", &self.callback_count())
            .finish()
    }
}

impl ScriptInputCallbackContext {
    fn callbacks(&self) -> MutexGuard<'_, Vec<HotkeyCallback>> {
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

    pub fn clear_callbacks(&self, vm: &LuaScriptVm) {
        let callbacks = std::mem::take(&mut *self.callbacks());
        for callback in callbacks {
            let _ = vm.lua.remove_registry_value(callback.registry_key);
        }
    }

    /// Poll registered callbacks once using C++ first-poll seeding and edge semantics.
    ///
    /// # Errors
    ///
    /// Returns the first Lua callback failure.
    pub fn poll_once(
        &self,
        vm: &LuaScriptVm,
        device: Option<&Arc<dyn InputDevice>>,
    ) -> Result<(), ScriptInputCallbackApiError> {
        poll_once_lua(&vm.lua, &vm.state, self, device)
            .map_err(|error| ScriptInputCallbackApiError::new(error.to_string()))
    }
}

#[derive(Debug)]
pub struct ScriptInputCallbackApiError(String);

impl ScriptInputCallbackApiError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ScriptInputCallbackApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ScriptInputCallbackApiError {}

impl From<LuaError> for ScriptInputCallbackApiError {
    fn from(error: LuaError) -> Self {
        Self::new(error.to_string())
    }
}

struct DispatchGuard<'a>(&'a AtomicBool);

impl Drop for DispatchGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn callback_down(device: Option<&Arc<dyn InputDevice>>, hotkey: Hotkey, strict: bool) -> bool {
    device.as_ref().is_some_and(|device| {
        is_hotkey_pressed(
            device.as_ref(),
            hotkey,
            if strict { HotkeyMatch::Strict } else { HotkeyMatch::Loose },
        )
    })
}

fn dispatch_callback(
    lua: &Lua,
    context: &ScriptInputCallbackContext,
    callback: &HotkeyCallback,
    down: bool,
) -> mlua::Result<()> {
    let function: Function = lua.registry_value(&callback.registry_key)?;
    context.state.dispatching.store(true, Ordering::Release);
    let _guard = DispatchGuard(&context.state.dispatching);
    match callback.kind {
        HotkeyCallbackKind::Changed => function.call::<()>((down, callback.hotkey.raw())),
        HotkeyCallbackKind::Pressed | HotkeyCallbackKind::Released => function.call::<()>(()),
    }
}

fn poll_once_lua(
    lua: &Lua,
    execution: &Arc<ExecutionState>,
    context: &ScriptInputCallbackContext,
    device: Option<&Arc<dyn InputDevice>>,
) -> mlua::Result<()> {
    let mut callbacks = context.callbacks();
    for callback in callbacks.iter_mut() {
        let down = callback_down(device, callback.hotkey, callback.strict);
        if !callback.initialized {
            callback.initialized = true;
            callback.last_down = down;
            continue;
        }
        if down == callback.last_down {
            continue;
        }

        let fire = callback.kind == HotkeyCallbackKind::Changed
            || (callback.kind == HotkeyCallbackKind::Pressed && down)
            || (callback.kind == HotkeyCallbackKind::Released && !down);
        callback.last_down = down;
        if !fire {
            continue;
        }

        dispatch_callback(lua, context, callback, down)?;
        if execution.reason() != ScriptStopReason::None {
            break;
        }
    }
    drop(callbacks);
    Ok(())
}

fn store_callback(
    lua: &Lua,
    callback_context: &ScriptInputCallbackContext,
    kind: HotkeyCallbackKind,
    hotkey: Hotkey,
    function: Function,
    strict: bool,
) -> mlua::Result<()> {
    let mut callbacks = callback_context.callbacks();
    if callbacks.len() >= MAX_INPUT_CALLBACKS {
        return Err(LuaError::RuntimeError(
            "script registered too many input callbacks".to_owned(),
        ));
    }
    let registry_key = lua.create_registry_value(function)?;
    callbacks.push(HotkeyCallback {
        kind,
        hotkey,
        registry_key,
        strict,
        initialized: false,
        last_down: false,
    });
    drop(callbacks);
    Ok(())
}

fn install_registration_api(
    vm: &LuaScriptVm,
    input: &Table,
    execution_context: &ScriptInputContext,
    callback_context: &ScriptInputCallbackContext,
) -> mlua::Result<()> {
    for (name, kind) in [
        ("onPressed", HotkeyCallbackKind::Pressed),
        ("onReleased", HotkeyCallbackKind::Released),
        ("onChanged", HotkeyCallbackKind::Changed),
    ] {
        let execution_context = execution_context.clone();
        let callback_context = callback_context.clone();
        let function_name = format!("input.{name}");
        input.set(
            name,
            vm.lua.create_function(
                move |lua, (hotkey, function, option): (Value, Function, Option<Value>)| {
                    check_settings_callback(&execution_context, &function_name)?;
                    check_cleanup_start(&execution_context, &function_name)?;
                    if callback_context.is_dispatching() {
                        return Err(LuaError::RuntimeError(
                            "cannot register input callbacks while dispatching input callbacks"
                                .to_owned(),
                        ));
                    }
                    let hotkey = hotkey_from_value(hotkey, "hotkey")?;
                    let strict = strict_option(
                        option,
                        execution_context.strict_hotkey_matching(),
                        "options",
                    )?;
                    store_callback(lua, &callback_context, kind, hotkey, function, strict)
                },
            )?,
        )?;
    }
    Ok(())
}

fn listener_scan_ms(lua: &Lua, value: Option<Value>) -> mlua::Result<i64> {
    let Some(value) = value else {
        return Ok(DEFAULT_INPUT_CALLBACK_SCAN_MS);
    };
    if matches!(value, Value::Nil) {
        return Ok(DEFAULT_INPUT_CALLBACK_SCAN_MS);
    }
    let Some(value) = lua.coerce_integer(value)? else {
        return Err(LuaError::RuntimeError("scanMs must be an integer".to_owned()));
    };
    Ok(value.clamp(MIN_INPUT_CALLBACK_SCAN_MS, MAX_INPUT_CALLBACK_SCAN_MS))
}

fn install_listener_api(
    vm: &LuaScriptVm,
    input: &Table,
    execution_context: &ScriptInputContext,
    callback_context: &ScriptInputCallbackContext,
    device: Option<&Arc<dyn InputDevice>>,
) -> mlua::Result<()> {
    let execution_context = execution_context.clone();
    let callback_context = callback_context.clone();
    let device = device.cloned();
    let execution = Arc::clone(&vm.state);
    input.set(
        "listenUntilCancelled",
        vm.lua.create_function(move |lua, scan_value: Option<Value>| {
            check_settings_callback(&execution_context, "input.listenUntilCancelled")?;
            check_cleanup_start(&execution_context, "input.listenUntilCancelled")?;
            if callback_context.is_dispatching() {
                return Err(LuaError::RuntimeError(
                    "cannot listen for input callbacks while dispatching input callbacks"
                        .to_owned(),
                ));
            }
            let scan_ms = listener_scan_ms(lua, scan_value)?;
            while execution.reason() == ScriptStopReason::None {
                poll_once_lua(lua, &execution, &callback_context, device.as_ref())?;
                if execution.reason() != ScriptStopReason::None {
                    break;
                }
                execution.wait_coarse(Duration::from_millis(
                    u64::try_from(scan_ms).unwrap_or_default(),
                ))?;
            }
            Ok(())
        })?,
    )
}

fn table_truthy(table: &Table, name: &str) -> mlua::Result<bool> {
    Ok(!matches!(table.get::<Value>(name)?, Value::Nil | Value::Boolean(false)))
}

fn mirror_strict_option(
    table: Option<&Table>,
    execution_context: &ScriptInputContext,
) -> mlua::Result<(bool, bool)> {
    let Some(table) = table else {
        return Ok((false, execution_context.strict_hotkey_matching()));
    };
    let allow_script_target = table_truthy(table, "allowScriptHotkeyTarget")?;
    let mut strict = execution_context.strict_hotkey_matching();
    let strict_value: Value = table.get("strict")?;
    if !matches!(strict_value, Value::Nil) {
        strict = strict_option(Some(strict_value), strict, "strict")?;
    }
    let source_strict: Value = table.get("sourceStrict")?;
    if !matches!(source_strict, Value::Nil) {
        strict = strict_option(Some(source_strict), strict, "sourceStrict")?;
    }
    let mode: Value = table.get("mode")?;
    if !matches!(mode, Value::Nil) {
        let Value::String(text) = mode else {
            return Err(LuaError::RuntimeError("mode must be \"strict\" or \"loose\"".to_owned()));
        };
        strict = parse_strict_text(&text.to_string_lossy(), "mode")?;
    }
    Ok((allow_script_target, strict))
}

fn install_mirror_api(
    vm: &LuaScriptVm,
    input: &Table,
    execution_context: &ScriptInputContext,
    callback_context: &ScriptInputCallbackContext,
    device: Option<&Arc<dyn InputDevice>>,
) -> mlua::Result<()> {
    let execution_context = execution_context.clone();
    let callback_context = callback_context.clone();
    let device = device.cloned();
    input.set(
        "mirror",
        vm.lua.create_function(
            move |lua, (source_value, target_value, options): (Value, Value, Option<Value>)| {
                check_settings_callback(&execution_context, "input.mirror")?;
                check_cleanup_start(&execution_context, "input.mirror")?;
                if callback_context.is_dispatching() {
                    return Err(LuaError::RuntimeError(
                        "cannot register input callbacks while dispatching input callbacks"
                            .to_owned(),
                    ));
                }
                let source = hotkey_from_value(source_value, "sourceHotkey")?;
                let target = hotkey_from_value(target_value, "targetHotkey")?;
                check_managed_target(target, "targetHotkey")?;
                let options = match options {
                    None | Some(Value::Nil) => None,
                    Some(Value::Table(table)) => Some(table),
                    Some(_) => {
                        return Err(LuaError::RuntimeError(
                            "bad argument #3 to 'mirror' (table expected)".to_owned(),
                        ));
                    }
                };
                let (allow_script_target, strict) =
                    mirror_strict_option(options.as_ref(), &execution_context)?;
                let script_hotkey = execution_context.script_hotkey();
                if !allow_script_target
                    && script_hotkey.is_bound()
                    && script_hotkey == target
                {
                    return Err(LuaError::RuntimeError(
                        "input.mirror target cannot be the script activation hotkey unless allowScriptHotkeyTarget is true"
                            .to_owned(),
                    ));
                }

                let mirror_execution = execution_context.clone();
                let mirror_device = device.clone();
                let function = lua.create_function(move |_, down: bool| {
                    let result = mirror_execution.set_managed_hotkey_held(target, down);
                    dispatch_actions(mirror_device.as_ref(), &result.actions);
                    Ok(())
                })?;
                store_callback(
                    lua,
                    &callback_context,
                    HotkeyCallbackKind::Changed,
                    source,
                    function,
                    strict,
                )
            },
        )?,
    )
}

/// Install edge-triggered input callbacks, listener polling, and managed hotkey mirroring.
///
/// `install_input_api` should normally run first so these fields extend the existing `input` table.
///
/// # Errors
///
/// Returns a Lua registration error if the callback surface cannot be installed.
pub fn install_input_callback_api(
    vm: &LuaScriptVm,
    execution_context: &ScriptInputContext,
    callback_context: &ScriptInputCallbackContext,
    device: Option<&Arc<dyn InputDevice>>,
) -> Result<(), ScriptInputCallbackApiError> {
    let input = match vm.lua.globals().get::<Value>("input")? {
        Value::Table(table) => table,
        Value::Nil => vm.lua.create_table()?,
        _ => {
            return Err(ScriptInputCallbackApiError::new("global input value is not a table"));
        }
    };
    install_registration_api(vm, &input, execution_context, callback_context)?;
    install_listener_api(vm, &input, execution_context, callback_context, device)?;
    install_mirror_api(vm, &input, execution_context, callback_context, device)?;
    vm.lua.globals().set("input", input)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::{LuaExecutionErrorKind, ScriptExecutionMode, install_input_api};
    use smu_core::KeyCode;
    use smu_input::{InputInjector, KeyState};
    use std::sync::Mutex;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Event {
        Hold(KeyCode),
        Release(KeyCode),
    }

    #[derive(Debug, Default)]
    struct FakeInput {
        pressed: Mutex<Vec<KeyCode>>,
        events: Mutex<Vec<Event>>,
    }

    impl FakeInput {
        fn set_pressed(&self, keys: &[KeyCode]) {
            *self.pressed.lock().expect("pressed lock") = keys.to_vec();
        }

        fn events(&self) -> Vec<Event> {
            self.events.lock().expect("events lock").clone()
        }
    }

    impl KeyState for FakeInput {
        fn is_pressed(&self, key: KeyCode) -> bool {
            self.pressed.lock().expect("pressed lock").contains(&key)
        }
    }

    impl InputInjector for FakeInput {
        fn hold_key(&self, key: KeyCode, _extended: bool) {
            self.events.lock().expect("events lock").push(Event::Hold(key));
        }

        fn release_key(&self, key: KeyCode, _extended: bool) {
            self.events.lock().expect("events lock").push(Event::Release(key));
        }

        fn move_mouse(&self, _dx: i32, _dy: i32) {}
        fn mouse_wheel(&self, _delta: i32) {}
    }

    fn installed() -> (LuaScriptVm, ScriptInputContext, ScriptInputCallbackContext, Arc<FakeInput>)
    {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let execution = ScriptInputContext::default();
        let callbacks = ScriptInputCallbackContext::default();
        let input = Arc::new(FakeInput::default());
        install_input_api(&vm, execution.clone(), Some(input.clone()), None)
            .expect("input API should install");
        let device: Arc<dyn InputDevice> = input.clone();
        install_input_callback_api(&vm, &execution, &callbacks, Some(&device))
            .expect("callback API should install");
        (vm, execution, callbacks, input)
    }

    #[test]
    fn first_poll_only_seeds_state_and_changed_fires_on_edge() {
        let (vm, _, callbacks, input) = installed();
        vm.execute_source(
            "events={}; input.onChanged('A', function(down,key) table.insert(events,{down,key}) end)",
            "register.lua",
        )
        .expect("callback should register");
        let device: Arc<dyn InputDevice> = input.clone();
        callbacks.poll_once(&vm, Some(&device)).expect("seed poll should succeed");
        assert_eq!(
            vm.eval::<usize>("return #events", Duration::from_secs(1))
                .expect("event count should return"),
            0
        );

        input.set_pressed(&[KeyCode::A]);
        callbacks.poll_once(&vm, Some(&device)).expect("press edge should dispatch");
        assert_eq!(
            vm.eval::<(bool, u32)>("return events[1][1], events[1][2]", Duration::from_secs(1),)
                .expect("changed args should be recorded"),
            (true, KeyCode::A.raw())
        );
    }

    #[test]
    fn pressed_and_released_callbacks_filter_opposite_edges() {
        let (vm, _, callbacks, input) = installed();
        vm.execute_source(
            concat!(
                "pressed=0; released=0; ",
                "input.onPressed('A', function() pressed=pressed+1 end); ",
                "input.onReleased('A', function() released=released+1 end)"
            ),
            "register.lua",
        )
        .expect("callbacks should register");
        let device: Arc<dyn InputDevice> = input.clone();
        callbacks.poll_once(&vm, Some(&device)).expect("seed poll");
        input.set_pressed(&[KeyCode::A]);
        callbacks.poll_once(&vm, Some(&device)).expect("press poll");
        input.set_pressed(&[]);
        callbacks.poll_once(&vm, Some(&device)).expect("release poll");
        assert_eq!(
            vm.eval::<(i64, i64)>("return pressed,released", Duration::from_secs(1))
                .expect("counts should return"),
            (1, 1)
        );
    }

    #[test]
    fn callback_registration_limit_is_128() {
        let (vm, _, callbacks, _) = installed();
        vm.execute_source("for i=1,128 do input.onChanged('A', function() end) end", "limit.lua")
            .expect("128 callbacks should register");
        assert_eq!(callbacks.callback_count(), MAX_INPUT_CALLBACKS);
        assert!(
            vm.execute_source("input.onChanged('A', function() end)", "overflow.lua")
                .expect_err("129th callback should fail")
                .message
                .contains("script registered too many input callbacks")
        );
    }

    #[test]
    fn callback_error_resets_dispatch_guard() {
        let (vm, _, callbacks, input) = installed();
        vm.execute_source(
            "input.onChanged('A', function() error('callback failed') end)",
            "register.lua",
        )
        .expect("callback should register");
        let device: Arc<dyn InputDevice> = input.clone();
        callbacks.poll_once(&vm, Some(&device)).expect("seed poll");
        input.set_pressed(&[KeyCode::A]);
        let error = callbacks.poll_once(&vm, Some(&device)).expect_err("callback should fail");
        assert!(error.to_string().contains("callback failed"));
        assert!(!callbacks.is_dispatching());
    }

    #[test]
    fn callback_cannot_register_another_callback_while_dispatching() {
        let (vm, _, callbacks, input) = installed();
        vm.execute_source(
            concat!(
                "input.onChanged('A', function() ",
                "input.onChanged('B', function() end) end)"
            ),
            "register.lua",
        )
        .expect("callback should register");
        let device: Arc<dyn InputDevice> = input.clone();
        callbacks.poll_once(&vm, Some(&device)).expect("seed poll");
        input.set_pressed(&[KeyCode::A]);
        assert!(
            callbacks
                .poll_once(&vm, Some(&device))
                .expect_err("reentrant registration should fail")
                .to_string()
                .contains("cannot register input callbacks while dispatching input callbacks")
        );
    }

    #[test]
    fn mirror_drives_managed_target_on_source_edges() {
        let (vm, _, callbacks, input) = installed();
        vm.execute_source("input.mirror('A', 'Ctrl+B')", "mirror.lua")
            .expect("mirror should register");
        let device: Arc<dyn InputDevice> = input.clone();
        callbacks.poll_once(&vm, Some(&device)).expect("seed poll");
        input.set_pressed(&[KeyCode::A]);
        callbacks.poll_once(&vm, Some(&device)).expect("press mirror");
        input.set_pressed(&[]);
        callbacks.poll_once(&vm, Some(&device)).expect("release mirror");
        assert_eq!(
            input.events(),
            [
                Event::Hold(KeyCode::CONTROL),
                Event::Hold(KeyCode::new(u32::from(b'B'))),
                Event::Release(KeyCode::new(u32::from(b'B'))),
                Event::Release(KeyCode::CONTROL),
            ]
        );
    }

    #[test]
    fn mirror_rejects_script_activation_target_without_opt_in() {
        let (vm, execution, _, _) = installed();
        execution.set_script_hotkey(Hotkey::new(0x2_0041));
        assert!(
            vm.execute_source("input.mirror('B','Ctrl+A')", "reject.lua")
                .expect_err("activation target should be rejected")
                .message
                .contains(
                    "input.mirror target cannot be the script activation hotkey unless allowScriptHotkeyTarget is true"
                )
        );
        vm.execute_source("input.mirror('B','Ctrl+A',{allowScriptHotkeyTarget=true})", "allow.lua")
            .expect("explicit opt-in should allow activation target");
    }

    #[test]
    fn settings_and_cleanup_restrict_registration_and_listener() {
        let (vm, execution, _, _) = installed();
        execution.set_mode(ScriptExecutionMode::SettingsRender);
        assert!(
            vm.execute_source("input.onPressed('A', function() end)", "settings.lua")
                .expect_err("settings should reject registration")
                .message
                .contains("input.onPressed is not available inside script settings")
        );
        execution.set_mode(ScriptExecutionMode::Cleanup);
        assert!(
            vm.execute_source("input.listenUntilCancelled()", "cleanup.lua")
                .expect_err("cleanup should reject listener")
                .message
                .contains("input.listenUntilCancelled is not available inside onCleanup")
        );
    }

    #[test]
    fn listener_scan_value_is_clamped() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        assert_eq!(
            listener_scan_ms(&vm.lua, None).expect("default scan delay should parse"),
            DEFAULT_INPUT_CALLBACK_SCAN_MS
        );
        assert_eq!(
            listener_scan_ms(&vm.lua, Some(Value::Integer(-100)))
                .expect("negative scan delay should clamp"),
            MIN_INPUT_CALLBACK_SCAN_MS
        );
        assert_eq!(
            listener_scan_ms(&vm.lua, Some(Value::Integer(500)))
                .expect("large scan delay should clamp"),
            MAX_INPUT_CALLBACK_SCAN_MS
        );
    }

    #[test]
    fn listener_cancellation_propagates_script_stop_reason() {
        let (vm, _, _, _) = installed();
        let stop = vm.stop_handle();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(5));
            stop.request_cancel();
        });
        let error = vm
            .execute_source("input.listenUntilCancelled(50)", "listen.lua")
            .expect_err("cancellation should stop listener");
        thread.join().expect("cancel thread should finish");
        assert_eq!(error.kind, LuaExecutionErrorKind::Cancelled);
    }

    #[test]
    fn clear_callbacks_removes_registry_entries_and_state() {
        let (vm, _, callbacks, _) = installed();
        vm.execute_source("input.onChanged('A', function() end)", "register.lua")
            .expect("callback should register");
        assert_eq!(callbacks.callback_count(), 1);
        callbacks.clear_callbacks(&vm);
        assert_eq!(callbacks.callback_count(), 0);
    }
}

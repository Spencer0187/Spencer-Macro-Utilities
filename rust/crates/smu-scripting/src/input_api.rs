//! Input-facing Lua API adapters.

use crate::lua_runtime::{ExecutionState, LuaScriptVm};
use crate::{ManagedHotkeys, ManagedKeyAction, parse_script_hotkey, parse_script_key_name};
use mlua::{Error as LuaError, Lua, Table, Value};
use smu_core::{Hotkey, KeyCode};
use smu_input::{HotkeyMatch, InputDevice, compatibility_key_action, is_hotkey_pressed};
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

const MAX_INPUT_DELAY_MS: i64 = 300_000;
const MAX_TYPE_TEXT_BYTES: usize = 4096;
const SCRIPT_HOTKEY_INTEGER_MAX: i64 = 2_147_483_647;
const HOTKEY_KEY_MASK: u32 = 0xFFFF;
static NEXT_SCRIPT_OWNER_TOKEN: AtomicUsize = AtomicUsize::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScriptExecutionMode {
    Normal,
    SettingsRender,
    SettingsCallback,
    Cleanup,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u32)]
pub enum ScriptMouseMotionMode {
    #[default]
    Raw = 0,
    Absolute = 1,
}

impl ScriptMouseMotionMode {
    const fn from_u32(value: u32) -> Self {
        if value == Self::Absolute as u32 { Self::Absolute } else { Self::Raw }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Absolute => "absolute",
        }
    }
}
impl ScriptExecutionMode {
    const fn as_u32(self) -> u32 {
        match self {
            Self::Normal => 0,
            Self::SettingsRender => 1,
            Self::SettingsCallback => 2,
            Self::Cleanup => 3,
        }
    }

    const fn from_u32(value: u32) -> Self {
        match value {
            1 => Self::SettingsRender,
            2 => Self::SettingsCallback,
            3 => Self::Cleanup,
            _ => Self::Normal,
        }
    }
}

#[derive(Debug)]
struct ScriptInputState {
    owner_token: usize,
    mode: AtomicU32,
    strict_hotkey_matching: AtomicBool,
    script_hotkey: AtomicU32,
    mouse_motion_mode: AtomicU32,
    managed: Mutex<ManagedHotkeys>,
}

impl Default for ScriptInputState {
    fn default() -> Self {
        Self {
            owner_token: NEXT_SCRIPT_OWNER_TOKEN.fetch_add(1, Ordering::Relaxed),
            mode: AtomicU32::new(ScriptExecutionMode::Normal.as_u32()),
            strict_hotkey_matching: AtomicBool::new(false),
            script_hotkey: AtomicU32::new(0),
            mouse_motion_mode: AtomicU32::new(ScriptMouseMotionMode::Raw as u32),
            managed: Mutex::new(ManagedHotkeys::default()),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ScriptInputContext {
    state: Arc<ScriptInputState>,
}

impl ScriptInputContext {
    #[must_use]
    pub fn owner_token(&self) -> usize {
        self.state.owner_token
    }

    pub fn set_mode(&self, mode: ScriptExecutionMode) {
        self.state.mode.store(mode.as_u32(), Ordering::Release);
    }

    #[must_use]
    pub fn mode(&self) -> ScriptExecutionMode {
        ScriptExecutionMode::from_u32(self.state.mode.load(Ordering::Acquire))
    }

    pub fn set_strict_hotkey_matching(&self, strict: bool) {
        self.state.strict_hotkey_matching.store(strict, Ordering::Release);
    }

    #[must_use]
    pub fn strict_hotkey_matching(&self) -> bool {
        self.state.strict_hotkey_matching.load(Ordering::Acquire)
    }

    pub fn set_script_hotkey(&self, hotkey: Hotkey) {
        self.state.script_hotkey.store(hotkey.raw(), Ordering::Release);
    }

    #[must_use]
    pub fn script_hotkey(&self) -> Hotkey {
        Hotkey::new(self.state.script_hotkey.load(Ordering::Acquire))
    }

    pub fn set_mouse_motion_mode(&self, mode: ScriptMouseMotionMode) {
        self.state.mouse_motion_mode.store(mode as u32, Ordering::Release);
    }

    #[must_use]
    pub fn mouse_motion_mode(&self) -> ScriptMouseMotionMode {
        ScriptMouseMotionMode::from_u32(self.state.mouse_motion_mode.load(Ordering::Acquire))
    }

    fn managed(&self) -> MutexGuard<'_, ManagedHotkeys> {
        match self.state.managed.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

#[derive(Debug)]
pub struct ScriptInputApiError(String);

impl ScriptInputApiError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ScriptInputApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ScriptInputApiError {}

impl From<LuaError> for ScriptInputApiError {
    fn from(error: LuaError) -> Self {
        Self::new(error.to_string())
    }
}

fn settings_callback_active(context: &ScriptInputContext) -> bool {
    context.mode() == ScriptExecutionMode::SettingsCallback
}

fn press_restricted(context: &ScriptInputContext) -> bool {
    matches!(
        context.mode(),
        ScriptExecutionMode::SettingsRender | ScriptExecutionMode::SettingsCallback
    )
}

fn cleanup_restricted(context: &ScriptInputContext) -> bool {
    context.mode() == ScriptExecutionMode::Cleanup
}

const fn lua_truthy(value: &Value) -> bool {
    !matches!(value, Value::Nil | Value::Boolean(false))
}

fn key_from_value(lua: &Lua, value: Value) -> mlua::Result<KeyCode> {
    if let Value::Integer(integer) = value
        && (0..=SCRIPT_HOTKEY_INTEGER_MAX).contains(&integer)
    {
        let raw = u32::try_from(integer).unwrap_or_default();
        let key = KeyCode::new(raw);
        if key.raw() & HOTKEY_KEY_MASK != 0 {
            return Ok(key);
        }
    }

    let Some(text) = lua.coerce_string(value)? else {
        return Err(LuaError::RuntimeError("invalid key: <null>".to_owned()));
    };
    let text = text.to_string_lossy();
    parse_script_key_name(&text)
        .ok_or_else(|| LuaError::RuntimeError(format!("invalid key: {text}")))
}

fn hotkey_from_value(value: Value, argument_name: &str) -> mlua::Result<Hotkey> {
    let parsed = match value {
        Value::Integer(integer) if (0..=SCRIPT_HOTKEY_INTEGER_MAX).contains(&integer) => {
            let raw = u32::try_from(integer).unwrap_or_default();
            let hotkey = Hotkey::new(raw);
            hotkey.is_bound().then_some(hotkey)
        }
        Value::String(text) => text.to_str().ok().and_then(|text| parse_script_hotkey(&text)),
        _ => None,
    };
    parsed.ok_or_else(|| {
        LuaError::RuntimeError(format!(
            "{argument_name} must be a bound hotkey integer or hotkey string"
        ))
    })
}

fn parse_strict_text(value: &str, argument_name: &str) -> mlua::Result<bool> {
    match value {
        "strict" | "exact" => Ok(true),
        "loose" | "default" => Ok(false),
        _ => {
            Err(LuaError::RuntimeError(format!("{argument_name} must be \"strict\" or \"loose\"")))
        }
    }
}

fn strict_option(value: Option<Value>, default: bool, argument_name: &str) -> mlua::Result<bool> {
    let Some(value) = value else {
        return Ok(default);
    };
    match value {
        Value::Nil => Ok(default),
        Value::Boolean(value) => Ok(value),
        Value::String(text) => parse_strict_text(&text.to_string_lossy(), argument_name),
        Value::Table(table) => strict_table_option(&table, default),
        _ => Err(LuaError::RuntimeError(format!(
            "{argument_name} must be a boolean, \"strict\"/\"loose\", or options table"
        ))),
    }
}

fn strict_table_option(table: &Table, default: bool) -> mlua::Result<bool> {
    let strict: Value = table.get("strict")?;
    if !matches!(strict, Value::Nil) {
        return Ok(lua_truthy(&strict));
    }
    let mode: Value = table.get("mode")?;
    if matches!(mode, Value::Nil) {
        return Ok(default);
    }
    let Value::String(text) = mode else {
        return Err(LuaError::RuntimeError("mode must be \"strict\" or \"loose\"".to_owned()));
    };
    parse_strict_text(&text.to_string_lossy(), "mode")
}

const fn is_mouse_related(key: KeyCode) -> bool {
    matches!(
        key,
        KeyCode::LBUTTON
            | KeyCode::RBUTTON
            | KeyCode::MBUTTON
            | KeyCode::XBUTTON1
            | KeyCode::XBUTTON2
            | KeyCode::MOUSE_WHEEL_UP
            | KeyCode::MOUSE_WHEEL_DOWN
    )
}

fn check_managed_target(hotkey: Hotkey, argument_name: &str) -> mlua::Result<()> {
    if matches!(hotkey.key(), KeyCode::MOUSE_WHEEL_UP | KeyCode::MOUSE_WHEEL_DOWN) {
        return Err(LuaError::RuntimeError(format!(
            "{argument_name} cannot be a mouse wheel action"
        )));
    }
    Ok(())
}

fn dispatch_actions(device: Option<&Arc<dyn InputDevice>>, actions: &[ManagedKeyAction]) {
    let Some(device) = device else {
        return;
    };
    for action in actions {
        match action {
            ManagedKeyAction::Hold(key) => device.hold_key(*key, false),
            ManagedKeyAction::Release(key) => device.release_key(*key, false),
        }
    }
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
    if cleanup_restricted(context) {
        return Err(LuaError::RuntimeError(format!(
            "{function_name} is not available inside onCleanup"
        )));
    }
    Ok(())
}

fn install_type_text_api(
    lua: &Lua,
    state: &Arc<ExecutionState>,
    context: &ScriptInputContext,
    device: Option<&Arc<dyn InputDevice>>,
) -> mlua::Result<()> {
    let state = Arc::clone(state);
    let context = context.clone();
    let device = device.cloned();
    lua.globals().set(
        "typeText",
        lua.create_function(move |lua, (text_value, delay_value): (Value, Option<Value>)| {
            if matches!(
                context.mode(),
                ScriptExecutionMode::SettingsRender | ScriptExecutionMode::SettingsCallback
            ) {
                return Err(LuaError::RuntimeError(
                    "typeText is not available inside script settings".to_owned(),
                ));
            }
            check_cleanup_start(&context, "typeText")?;
            state.check_deadline()?;

            let Some(text) = lua.coerce_string(text_value)? else {
                return Err(LuaError::RuntimeError(
                    "bad argument #1 to 'typeText' (string expected)".to_owned(),
                ));
            };
            let bytes = text.as_bytes();
            if bytes.len() > MAX_TYPE_TEXT_BYTES {
                return Err(LuaError::RuntimeError(format!(
                    "typeText text is too long (max {MAX_TYPE_TEXT_BYTES} bytes)"
                )));
            }
            let delay = match delay_value {
                None | Some(Value::Nil) => 30,
                Some(value) => {
                    let Some(integer) = lua.coerce_integer(value)? else {
                        return Err(LuaError::RuntimeError("delay must be an integer".to_owned()));
                    };
                    integer.clamp(0, MAX_INPUT_DELAY_MS)
                }
            };
            let Some(device) = device.as_ref() else {
                return Ok(());
            };

            for &byte in &bytes {
                state.check_deadline()?;
                let action = compatibility_key_action(byte);
                if !action.valid {
                    continue;
                }
                if action.needs_shift {
                    device.hold_key(KeyCode::SHIFT, false);
                }
                device.hold_key(action.key, false);
                let waited = state
                    .wait_coarse(Duration::from_millis(u64::try_from(delay).unwrap_or_default()));
                device.release_key(action.key, false);
                if action.needs_shift {
                    device.release_key(KeyCode::SHIFT, false);
                }
                waited?;
            }
            Ok(())
        })?,
    )
}

fn install_press_function(
    lua: &Lua,
    state: &Arc<ExecutionState>,
    context: &ScriptInputContext,
    device: Option<&Arc<dyn InputDevice>>,
    name: &'static str,
    mouse_only: bool,
) -> mlua::Result<()> {
    let state = Arc::clone(state);
    let context = context.clone();
    let device = device.cloned();
    lua.globals().set(
        name,
        lua.create_function(move |lua, (key_value, delay_value): (Value, Option<Value>)| {
            if press_restricted(&context) {
                return Err(LuaError::RuntimeError(format!(
                    "{name} is not available inside script settings"
                )));
            }
            check_cleanup_start(&context, name)?;
            state.check_deadline()?;
            let key = key_from_value(lua, key_value)?;
            if mouse_only && !is_mouse_related(key) {
                return Err(LuaError::RuntimeError(format!(
                    "{name} only accepts mouse buttons or mouse wheel actions"
                )));
            }
            let delay = match delay_value {
                None | Some(Value::Nil) => 50,
                Some(value) => {
                    let Some(integer) = lua.coerce_integer(value)? else {
                        return Err(LuaError::RuntimeError("delay must be an integer".to_owned()));
                    };
                    integer.clamp(0, MAX_INPUT_DELAY_MS)
                }
            };
            let Some(device) = device.as_ref() else {
                return Ok(());
            };
            device.hold_key(key, false);
            let waited =
                state.wait_coarse(Duration::from_millis(u64::try_from(delay).unwrap_or_default()));
            device.release_key(key, false);
            waited
        })?,
    )
}

fn install_direct_action_api(
    lua: &Lua,
    context: &ScriptInputContext,
    device: Option<&Arc<dyn InputDevice>>,
) -> mlua::Result<()> {
    let hold_context = context.clone();
    let hold_device = device.cloned();
    lua.globals().set(
        "holdKey",
        lua.create_function(move |lua, value: Value| {
            check_settings_callback(&hold_context, "holdKey")?;
            check_cleanup_start(&hold_context, "holdKey")?;
            let key = key_from_value(lua, value)?;
            if let Some(device) = &hold_device {
                device.hold_key(key, false);
            }
            Ok(())
        })?,
    )?;

    let release_context = context.clone();
    let release_device = device.cloned();
    lua.globals().set(
        "releaseKey",
        lua.create_function(move |lua, value: Value| {
            check_settings_callback(&release_context, "releaseKey")?;
            let key = key_from_value(lua, value)?;
            if let Some(device) = &release_device {
                device.release_key(key, false);
            }
            Ok(())
        })?,
    )?;

    let wheel_context = context.clone();
    let wheel_device = device.cloned();
    lua.globals().set(
        "mouseWheel",
        lua.create_function(move |lua, value: Value| {
            check_settings_callback(&wheel_context, "mouseWheel")?;
            check_cleanup_start(&wheel_context, "mouseWheel")?;
            let Some(delta) = lua.coerce_integer(value)? else {
                return Err(LuaError::RuntimeError("delta must be an integer".to_owned()));
            };
            let delta = i32::try_from(delta).map_err(|_| {
                LuaError::RuntimeError("delta is outside the allowed range".to_owned())
            })?;
            if let Some(device) = &wheel_device {
                device.mouse_wheel(delta);
            }
            Ok(())
        })?,
    )
}

fn install_direct_state_api(
    lua: &Lua,
    context: &ScriptInputContext,
    device: Option<&Arc<dyn InputDevice>>,
) -> mlua::Result<()> {
    let pressed_context = context.clone();
    let pressed_device = device.cloned();
    lua.globals().set(
        "isKeyPressed",
        lua.create_function(move |lua, value: Value| {
            check_settings_callback(&pressed_context, "isKeyPressed")?;
            let key = key_from_value(lua, value)?;
            Ok(pressed_device.as_ref().is_some_and(|device| device.is_pressed(key)))
        })?,
    )?;

    let hotkey_context = context.clone();
    let hotkey_device = device.cloned();
    lua.globals().set(
        "isHotkeyPressed",
        lua.create_function(move |_, (value, option): (Value, Option<Value>)| {
            check_settings_callback(&hotkey_context, "isHotkeyPressed")?;
            let hotkey = hotkey_from_value(value, "hotkey")?;
            let strict =
                strict_option(option, hotkey_context.strict_hotkey_matching(), "match mode")?;
            Ok(hotkey_device.as_ref().is_some_and(|device| {
                is_hotkey_pressed(
                    device.as_ref(),
                    hotkey,
                    if strict { HotkeyMatch::Strict } else { HotkeyMatch::Loose },
                )
            }))
        })?,
    )?;

    let script_hotkey_context = context.clone();
    lua.globals().set(
        "getScriptHotkey",
        lua.create_function(move |_, ()| {
            let hotkey = script_hotkey_context.script_hotkey();
            Ok(hotkey.is_bound().then_some(hotkey.raw()))
        })?,
    )
}

fn install_managed_action_api(
    lua: &Lua,
    input: &Table,
    context: &ScriptInputContext,
    device: Option<&Arc<dyn InputDevice>>,
) -> mlua::Result<()> {
    let hold_context = context.clone();
    let hold_device = device.cloned();
    input.set(
        "hold",
        lua.create_function(move |_, value: Value| {
            check_settings_callback(&hold_context, "input.hold")?;
            check_cleanup_start(&hold_context, "input.hold")?;
            let hotkey = hotkey_from_value(value, "hotkey")?;
            check_managed_target(hotkey, "hotkey")?;
            let result = hold_context.managed().hold(hotkey);
            dispatch_actions(hold_device.as_ref(), &result.actions);
            Ok(())
        })?,
    )?;

    let release_context = context.clone();
    let release_device = device.cloned();
    input.set(
        "release",
        lua.create_function(move |_, value: Value| {
            check_settings_callback(&release_context, "input.release")?;
            let hotkey = hotkey_from_value(value, "hotkey")?;
            check_managed_target(hotkey, "hotkey")?;
            let result = release_context.managed().release(hotkey);
            dispatch_actions(release_device.as_ref(), &result.actions);
            Ok(())
        })?,
    )?;

    let set_context = context.clone();
    let set_device = device.cloned();
    input.set(
        "setHeld",
        lua.create_function(move |_, (value, down): (Value, Value)| {
            check_settings_callback(&set_context, "input.setHeld")?;
            let hotkey = hotkey_from_value(value, "hotkey")?;
            check_managed_target(hotkey, "hotkey")?;
            let down = lua_truthy(&down);
            if down {
                check_cleanup_start(&set_context, "input.setHeld(true)")?;
            }
            let result = set_context.managed().set_held(hotkey, down);
            dispatch_actions(set_device.as_ref(), &result.actions);
            Ok(result.down)
        })?,
    )?;

    let toggle_context = context.clone();
    let toggle_device = device.cloned();
    input.set(
        "toggleHeld",
        lua.create_function(move |_, value: Value| {
            check_settings_callback(&toggle_context, "input.toggleHeld")?;
            check_cleanup_start(&toggle_context, "input.toggleHeld")?;
            let hotkey = hotkey_from_value(value, "hotkey")?;
            check_managed_target(hotkey, "hotkey")?;
            let result = toggle_context.managed().toggle(hotkey);
            dispatch_actions(toggle_device.as_ref(), &result.actions);
            Ok(result.down)
        })?,
    )
}

fn install_managed_state_api(
    lua: &Lua,
    input: &Table,
    context: &ScriptInputContext,
    device: Option<&Arc<dyn InputDevice>>,
) -> mlua::Result<()> {
    let release_all_context = context.clone();
    let release_all_device = device.cloned();
    input.set(
        "releaseAllManaged",
        lua.create_function(move |_, ()| {
            check_settings_callback(&release_all_context, "input.releaseAllManaged")?;
            let keys = release_all_context.managed().release_all();
            if let Some(device) = &release_all_device {
                for key in keys {
                    device.release_key(key, false);
                }
            }
            Ok(())
        })?,
    )?;

    let is_pressed_context = context.clone();
    let is_pressed_device = device.cloned();
    input.set(
        "isPressed",
        lua.create_function(move |_, (value, option): (Value, Option<Value>)| {
            check_settings_callback(&is_pressed_context, "input.isPressed")?;
            let hotkey = hotkey_from_value(value, "hotkey")?;
            let strict =
                strict_option(option, is_pressed_context.strict_hotkey_matching(), "match mode")?;
            Ok(is_pressed_device.as_ref().is_some_and(|device| {
                is_hotkey_pressed(
                    device.as_ref(),
                    hotkey,
                    if strict { HotkeyMatch::Strict } else { HotkeyMatch::Loose },
                )
            }))
        })?,
    )?;

    let set_mode_context = context.clone();
    input.set(
        "setHotkeyMode",
        lua.create_function(move |_, value: Option<Value>| {
            check_settings_callback(&set_mode_context, "input.setHotkeyMode")?;
            let strict = strict_option(value, set_mode_context.strict_hotkey_matching(), "mode")?;
            set_mode_context.set_strict_hotkey_matching(strict);
            Ok(if strict { "strict" } else { "loose" })
        })?,
    )?;

    let get_mode_context = context.clone();
    input.set(
        "getHotkeyMode",
        lua.create_function(move |_, ()| {
            Ok(if get_mode_context.strict_hotkey_matching() { "strict" } else { "loose" })
        })?,
    )
}

fn install_managed_input_table(
    lua: &Lua,
    context: &ScriptInputContext,
    device: Option<&Arc<dyn InputDevice>>,
) -> mlua::Result<()> {
    let input = lua.create_table()?;
    install_managed_action_api(lua, &input, context, device)?;
    install_managed_state_api(lua, &input, context, device)?;
    lua.globals().set("input", input)
}

/// Register the input subset of the historical global/script API against modular input services.
///
/// # Errors
///
/// Returns a Lua registration error if the VM cannot create or install one of the functions.
pub fn install_input_api(
    vm: &LuaScriptVm,
    context: ScriptInputContext,
    device: Option<Arc<dyn InputDevice>>,
) -> Result<(), ScriptInputApiError> {
    let resources = (context, device);
    let context = &resources.0;
    let device = resources.1.as_ref();
    let lua = &vm.lua;
    install_press_function(lua, &vm.state, context, device, "pressKey", false)?;
    install_press_function(lua, &vm.state, context, device, "clickMouse", true)?;
    install_type_text_api(lua, &vm.state, context, device)?;
    install_direct_action_api(lua, context, device)?;
    install_direct_state_api(lua, context, device)?;
    install_managed_input_table(lua, context, device)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::{LuaExecutionErrorKind, MAX_SCRIPT_RUNTIME};
    use smu_input::{InputInjector, KeyState};
    use std::sync::Mutex;

    #[derive(Debug, Clone, Eq, PartialEq)]
    enum Event {
        Hold(KeyCode),
        Release(KeyCode),
        Wheel(i32),
    }

    #[derive(Debug, Default)]
    struct FakeInput {
        pressed: Mutex<Vec<KeyCode>>,
        events: Mutex<Vec<Event>>,
    }

    impl FakeInput {
        fn press_physical(&self, keys: &[KeyCode]) {
            *self.pressed.lock().expect("pressed lock") = keys.to_vec();
        }

        fn events(&self) -> Vec<Event> {
            self.events.lock().expect("event lock").clone()
        }
    }

    impl KeyState for FakeInput {
        fn is_pressed(&self, key: KeyCode) -> bool {
            self.pressed.lock().expect("pressed lock").contains(&key)
        }
    }

    impl InputInjector for FakeInput {
        fn hold_key(&self, key: KeyCode, _extended: bool) {
            self.events.lock().expect("event lock").push(Event::Hold(key));
        }

        fn release_key(&self, key: KeyCode, _extended: bool) {
            self.events.lock().expect("event lock").push(Event::Release(key));
        }

        fn move_mouse(&self, _dx: i32, _dy: i32) {}

        fn mouse_wheel(&self, delta: i32) {
            self.events.lock().expect("event lock").push(Event::Wheel(delta));
        }
    }

    fn vm_with_input() -> (LuaScriptVm, ScriptInputContext, Arc<FakeInput>) {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let context = ScriptInputContext::default();
        let input = Arc::new(FakeInput::default());
        install_input_api(&vm, context.clone(), Some(input.clone()))
            .expect("input API should install");
        (vm, context, input)
    }

    #[test]
    fn press_key_releases_after_zero_delay() {
        let (vm, _, input) = vm_with_input();
        vm.execute_source("pressKey('A', 0)", "press.lua").expect("pressKey should execute");
        assert_eq!(input.events(), [Event::Hold(KeyCode::A), Event::Release(KeyCode::A)]);
    }

    #[test]
    fn click_mouse_rejects_keyboard_key() {
        let (vm, _, _) = vm_with_input();
        let error = vm
            .execute_source("clickMouse('A', 0)", "click.lua")
            .expect_err("keyboard key should fail mouse-only API");
        assert!(
            error.message.contains("clickMouse only accepts mouse buttons or mouse wheel actions")
        );
    }

    #[test]
    fn direct_hold_is_forbidden_in_cleanup_but_release_is_allowed() {
        let (vm, context, input) = vm_with_input();
        context.set_mode(ScriptExecutionMode::Cleanup);
        let error = vm
            .execute_source("holdKey('A')", "cleanup.lua")
            .expect_err("cleanup cannot start holds");
        assert!(error.message.contains("holdKey is not available inside onCleanup"));
        vm.execute_source("releaseKey('A')", "cleanup-release.lua")
            .expect("cleanup release should be allowed");
        assert_eq!(input.events(), [Event::Release(KeyCode::A)]);
    }

    #[test]
    fn managed_hotkeys_share_modifier_reference_counts_through_lua() {
        let (vm, _, input) = vm_with_input();
        vm.execute_source(
            "input.hold('Ctrl+A'); input.hold('Ctrl+B'); input.release('Ctrl+A'); input.release('Ctrl+B')",
            "managed.lua",
        )
        .expect("managed hotkey sequence should run");
        assert_eq!(
            input.events(),
            [
                Event::Hold(KeyCode::CONTROL),
                Event::Hold(KeyCode::A),
                Event::Hold(KeyCode::new(u32::from(b'B'))),
                Event::Release(KeyCode::A),
                Event::Release(KeyCode::new(u32::from(b'B'))),
                Event::Release(KeyCode::CONTROL),
            ]
        );
    }

    #[test]
    fn strict_hotkey_mode_uses_physical_state_service() {
        let (vm, context, input) = vm_with_input();
        input.press_physical(&[KeyCode::CONTROL, KeyCode::SHIFT, KeyCode::A]);
        assert!(
            vm.eval::<bool>("return isHotkeyPressed('Ctrl+A')", MAX_SCRIPT_RUNTIME)
                .expect("loose match should evaluate")
        );
        context.set_strict_hotkey_matching(true);
        assert!(
            !vm.eval::<bool>("return isHotkeyPressed('Ctrl+A')", MAX_SCRIPT_RUNTIME)
                .expect("strict match should evaluate")
        );
        assert_eq!(
            vm.eval::<String>("return input.setHotkeyMode('loose')", MAX_SCRIPT_RUNTIME)
                .expect("mode setter should run"),
            "loose"
        );
    }

    #[test]
    fn settings_callback_blocks_input_side_effects() {
        let (vm, context, input) = vm_with_input();
        context.set_mode(ScriptExecutionMode::SettingsCallback);
        let error = vm
            .execute_source("input.hold('A')", "settings.lua")
            .expect_err("settings callback must reject input side effects");
        assert!(error.message.contains("input.hold is not available inside script settings"));
        assert!(input.events().is_empty());
    }

    #[test]
    fn missing_backend_matches_cpp_noop_and_false_behavior() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let context = ScriptInputContext::default();
        install_input_api(&vm, context, None).expect("input API should install without backend");
        vm.execute_source("pressKey('A', 1000)", "no-backend.lua")
            .expect("pressKey without backend is a no-op");
        assert!(
            !vm.eval::<bool>("return isKeyPressed('A')", MAX_SCRIPT_RUNTIME)
                .expect("isKeyPressed should evaluate")
        );
    }

    #[test]
    fn cancellation_during_press_releases_key_before_error() {
        let (vm, _, input) = vm_with_input();
        let stop = vm.stop_handle();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(5));
            stop.request_cancel();
        });
        let error = vm
            .execute_source("pressKey('A', 5000)", "cancel-press.lua")
            .expect_err("cancel should interrupt press wait");
        thread.join().expect("cancel thread should finish");
        assert_eq!(error.kind, LuaExecutionErrorKind::Cancelled);
        assert_eq!(input.events(), [Event::Hold(KeyCode::A), Event::Release(KeyCode::A)]);
    }

    #[test]
    fn type_text_matches_cpp_us_layout_fallback_and_shift_order() {
        let (vm, _, input) = vm_with_input();
        vm.execute_source("typeText('aA?', 0)", "type.lua").expect("typeText should execute");
        assert_eq!(
            input.events(),
            [
                Event::Hold(KeyCode::A),
                Event::Release(KeyCode::A),
                Event::Hold(KeyCode::SHIFT),
                Event::Hold(KeyCode::A),
                Event::Release(KeyCode::A),
                Event::Release(KeyCode::SHIFT),
                Event::Hold(KeyCode::SHIFT),
                Event::Hold(KeyCode::OEM_2),
                Event::Release(KeyCode::OEM_2),
                Event::Release(KeyCode::SHIFT),
            ]
        );
    }

    #[test]
    fn type_text_skips_unmapped_bytes_and_caps_input_length() {
        let (vm, _, input) = vm_with_input();
        vm.execute_source("typeText(string.char(255) .. '1', 0)", "bytes.lua")
            .expect("unmapped byte should be skipped");
        assert_eq!(
            input.events(),
            [
                Event::Hold(KeyCode::new(u32::from(b'1'))),
                Event::Release(KeyCode::new(u32::from(b'1'))),
            ]
        );
        let error = vm
            .execute_source("typeText(string.rep('x', 4097), 0)", "too-long.lua")
            .expect_err("overlong text should fail");
        assert!(error.message.contains("typeText text is too long (max 4096 bytes)"));
    }

    #[test]
    fn type_text_cancellation_releases_current_key_and_shift() {
        let (vm, _, input) = vm_with_input();
        let stop = vm.stop_handle();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(5));
            stop.request_cancel();
        });
        let error = vm
            .execute_source("typeText('A', 5000)", "cancel-type.lua")
            .expect_err("cancel should interrupt typeText wait");
        thread.join().expect("cancel thread should finish");
        assert_eq!(error.kind, LuaExecutionErrorKind::Cancelled);
        assert_eq!(
            input.events(),
            [
                Event::Hold(KeyCode::SHIFT),
                Event::Hold(KeyCode::A),
                Event::Release(KeyCode::A),
                Event::Release(KeyCode::SHIFT),
            ]
        );
    }

    #[test]
    fn script_hotkey_returns_nil_when_unbound() {
        let (vm, context, _) = vm_with_input();
        assert!(
            vm.eval::<bool>("return getScriptHotkey() == nil", MAX_SCRIPT_RUNTIME)
                .expect("unbound hotkey should be nil")
        );
        context.set_script_hotkey(Hotkey::new(0x2_0041));
        assert_eq!(
            vm.eval::<u32>("return getScriptHotkey()", MAX_SCRIPT_RUNTIME)
                .expect("bound hotkey should be returned"),
            0x2_0041
        );
    }

    #[test]
    fn wheel_api_forwards_signed_delta() {
        let (vm, _, input) = vm_with_input();
        vm.execute_source("mouseWheel(-120)", "wheel.lua").expect("mouseWheel should execute");
        assert_eq!(input.events(), [Event::Wheel(-120)]);
    }
}

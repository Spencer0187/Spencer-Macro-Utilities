//! Lua `ui` table and script settings-state synchronization.

use crate::input_api::{ScriptExecutionMode, ScriptInputContext};
use crate::lua_runtime::{LuaExecutionError, LuaScriptVm, MAX_SETTINGS_RUNTIME};
use crate::persistence::{
    MAX_UI_ID_BYTES, MAX_UI_STATE_ENTRIES, MAX_UI_STATE_STRING_BYTES, SCRIPT_UNBOUND_HOTKEY,
    is_safe_ui_state_key, normalize_script_hotkey,
};
use crate::settings_ui::{MAX_UI_DIMENSION, MAX_UI_DROPDOWN_ITEMS, ScriptSettingsUiContext};
use crate::{ScriptStopReason, parse_script_hotkey};
use mlua::{Error as LuaError, Function, Lua, Table, Value};
use serde_json::{Map, Number, Value as JsonValue};
use smu_core::Hotkey;
use std::collections::HashSet;
use std::fmt;

const FLOAT_MAX_AS_F64: f64 = f32::MAX as f64;
const SCRIPT_INTEGER_MAX: i64 = 2_147_483_647;

#[derive(Debug)]
pub struct ScriptSettingsUiApiError(String);

impl ScriptSettingsUiApiError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ScriptSettingsUiApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ScriptSettingsUiApiError {}

impl From<LuaError> for ScriptSettingsUiApiError {
    fn from(error: LuaError) -> Self {
        Self::new(error.to_string())
    }
}

fn lua_string(lua: &Lua, value: Value, name: &str) -> mlua::Result<String> {
    let Some(value) = lua.coerce_string(value)? else {
        return Err(LuaError::RuntimeError(format!("{name} must be a string")));
    };
    Ok(value.to_string_lossy())
}

fn clamped_string(value: &str) -> String {
    if value.len() <= MAX_UI_STATE_STRING_BYTES {
        return value.to_owned();
    }
    let mut boundary = MAX_UI_STATE_STRING_BYTES;
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value[..boundary].to_owned()
}

fn clamp_dynamic_text(value: String) -> String {
    if value.len() <= MAX_UI_STATE_STRING_BYTES {
        return value;
    }
    let mut boundary = value.len() - MAX_UI_STATE_STRING_BYTES;
    while boundary < value.len() && !value.is_char_boundary(boundary) {
        boundary += 1;
    }
    value[boundary..].to_owned()
}

fn ui_id(lua: &Lua, value: Value) -> mlua::Result<String> {
    let Some(value) = lua.coerce_string(value)? else {
        return Err(LuaError::RuntimeError("UI id must be a string".to_owned()));
    };
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return Err(LuaError::RuntimeError("UI id must not be empty".to_owned()));
    }
    if bytes.len() > MAX_UI_ID_BYTES {
        return Err(LuaError::RuntimeError("UI id is too long".to_owned()));
    }
    if bytes.contains(&0) {
        return Err(LuaError::RuntimeError("UI id must not contain embedded NUL bytes".to_owned()));
    }
    String::from_utf8(bytes.to_vec())
        .map_err(|_| LuaError::RuntimeError("UI id must be valid UTF-8".to_owned()))
}

fn optional_dimension(lua: &Lua, value: Option<Value>, name: &str) -> mlua::Result<f32> {
    let Some(value) = value else {
        return Ok(0.0);
    };
    if matches!(value, Value::Nil) {
        return Ok(0.0);
    }
    let Some(number) = lua.coerce_number(value)? else {
        return Err(LuaError::RuntimeError(format!("{name} must be a number")));
    };
    if !number.is_finite() {
        return Err(LuaError::RuntimeError(format!("{name} must be a finite number")));
    }
    if number <= 0.0 {
        return Ok(0.0);
    }
    if number > f64::from(MAX_UI_DIMENSION) {
        return Ok(MAX_UI_DIMENSION);
    }
    #[allow(clippy::cast_possible_truncation)]
    Ok(number as f32)
}

fn optional_finite_number(
    lua: &Lua,
    value: Option<Value>,
    default: f64,
    name: &str,
) -> mlua::Result<f64> {
    let Some(value) = value else {
        return Ok(default);
    };
    if matches!(value, Value::Nil) {
        return Ok(default);
    }
    let Some(number) = lua.coerce_number(value)? else {
        return Err(LuaError::RuntimeError(format!("{name} must be a number")));
    };
    if !number.is_finite() {
        return Err(LuaError::RuntimeError(format!("{name} must be a finite number")));
    }
    Ok(number)
}

fn optional_i32(lua: &Lua, value: Option<Value>, default: i32, name: &str) -> mlua::Result<i32> {
    let Some(value) = value else {
        return Ok(default);
    };
    if matches!(value, Value::Nil) {
        return Ok(default);
    }
    let Some(integer) = lua.coerce_integer(value)? else {
        return Err(LuaError::RuntimeError(format!("{name} must be an integer")));
    };
    i32::try_from(integer)
        .map_err(|_| LuaError::RuntimeError(format!("{name} is outside the allowed range")))
}

fn consume_control(context: &ScriptSettingsUiContext) -> mlua::Result<()> {
    if context.try_consume_control() {
        return Ok(());
    }
    Err(LuaError::RuntimeError("script settings created too many UI controls".to_owned()))
}

fn register_id(context: &ScriptSettingsUiContext, id: &str) -> mlua::Result<()> {
    if context.try_register_id(id) {
        return Ok(());
    }
    Err(LuaError::RuntimeError("script settings created too many unique UI IDs".to_owned()))
}

fn json_to_lua(lua: &Lua, value: &JsonValue) -> mlua::Result<Option<Value>> {
    let value = match value {
        JsonValue::Bool(value) => Value::Boolean(*value),
        JsonValue::Number(number) => {
            if let Some(value) = number.as_i64() {
                Value::Integer(value)
            } else if let Some(value) = number.as_u64().and_then(|value| i64::try_from(value).ok())
            {
                Value::Integer(value)
            } else if let Some(value) = number.as_f64().filter(|value| value.is_finite()) {
                Value::Number(value)
            } else {
                return Ok(None);
            }
        }
        JsonValue::String(value) => Value::String(lua.create_string(clamped_string(value))?),
        JsonValue::Null | JsonValue::Array(_) | JsonValue::Object(_) => return Ok(None),
    };
    Ok(Some(value))
}

fn lua_to_json(value: Value) -> Option<JsonValue> {
    match value {
        Value::Boolean(value) => Some(JsonValue::Bool(value)),
        Value::Integer(value) => Some(JsonValue::Number(Number::from(value))),
        Value::Number(value) if value.is_finite() => Number::from_f64(value).map(JsonValue::Number),
        Value::String(value) => Some(JsonValue::String(clamped_string(&value.to_string_lossy()))),
        Value::Nil
        | Value::LightUserData(_)
        | Value::Table(_)
        | Value::Function(_)
        | Value::Thread(_)
        | Value::UserData(_)
        | Value::Error(_)
        | Value::Other(_)
        | Value::Number(_) => None,
    }
}

fn sync_setting(lua: &Lua, id: &str, value: &JsonValue) -> mlua::Result<()> {
    let globals = lua.globals();
    let settings = if let Value::Table(table) = globals.get::<Value>("settings")? {
        table
    } else {
        let table = lua.create_table()?;
        globals.set("settings", table.clone())?;
        table
    };
    if let Some(value) = json_to_lua(lua, value)? {
        settings.set(id, value)?;
    }
    Ok(())
}

fn update_setting(
    lua: &Lua,
    context: &ScriptSettingsUiContext,
    id: &str,
    value: &JsonValue,
) -> mlua::Result<()> {
    context.set_stored(id, value.clone());
    sync_setting(lua, id, value)
}

fn sync_settings_table(lua: &Lua, context: &ScriptSettingsUiContext) -> mlua::Result<()> {
    let settings = lua.create_table()?;
    if let Some(object) = context.ui_state().as_object() {
        for (key, value) in object {
            if let Some(value) = json_to_lua(lua, value)? {
                settings.set(key.as_str(), value)?;
            }
        }
    }
    lua.globals().set("settings", settings)
}

/// Copy scalar values from the Lua `settings` table back into persisted UI state.
///
/// # Errors
///
/// Returns a Lua table-access error.
pub fn sync_ui_state_from_lua_settings(
    vm: &LuaScriptVm,
    context: &ScriptSettingsUiContext,
) -> Result<(), ScriptSettingsUiApiError> {
    let Value::Table(settings) = vm.lua.globals().get::<Value>("settings")? else {
        return Ok(());
    };

    let mut updates = Map::new();
    for pair in settings.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let Value::String(key) = key else {
            continue;
        };
        let key = key.to_string_lossy();
        if !is_safe_ui_state_key(&key) || updates.len() >= MAX_UI_STATE_ENTRIES {
            continue;
        }
        if let Some(value) = lua_to_json(value) {
            updates.insert(key, value);
        }
    }
    for (key, value) in updates {
        context.set_stored(&key, value);
    }
    Ok(())
}

fn stored_bool(context: &ScriptSettingsUiContext, id: &str, default: bool) -> (bool, bool) {
    let stored = context.stored(id);
    let exists = stored.is_some();
    let value = stored.and_then(|value| value.as_bool()).unwrap_or(default);
    (exists, value)
}

fn stored_i32(context: &ScriptSettingsUiContext, id: &str, default: i32) -> (bool, i32) {
    let stored = context.stored(id);
    let exists = stored.is_some();
    let value = stored
        .and_then(|value| {
            value
                .as_i64()
                .map(|value| value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)))
                .and_then(|value| i32::try_from(value).ok())
                .or_else(|| {
                    value
                        .as_u64()
                        .map(|value| value.min(i32::MAX as u64))
                        .and_then(|value| i32::try_from(value).ok())
                })
        })
        .unwrap_or(default);
    (exists, value)
}

fn stored_float(context: &ScriptSettingsUiContext, id: &str, default: f64) -> (bool, f64) {
    let stored = context.stored(id);
    let exists = stored.is_some();
    let value = stored
        .and_then(|value| value.as_f64())
        .filter(|value| value.is_finite())
        .unwrap_or(default)
        .clamp(-FLOAT_MAX_AS_F64, FLOAT_MAX_AS_F64);
    (exists, value)
}

fn stored_string(context: &ScriptSettingsUiContext, id: &str, default: &str) -> (bool, String) {
    let stored = context.stored(id);
    let exists = stored.is_some();
    let value = stored
        .and_then(|value| value.as_str().map(clamped_string))
        .unwrap_or_else(|| clamped_string(default));
    (exists, value)
}

fn pending_bool(context: &ScriptSettingsUiContext, id: &str) -> Option<bool> {
    context.take_pending_value(id).and_then(|value| value.as_bool())
}

fn pending_i32(context: &ScriptSettingsUiContext, id: &str) -> Option<i32> {
    context.take_pending_value(id).and_then(|value| {
        value
            .as_i64()
            .and_then(|value| i32::try_from(value).ok())
            .or_else(|| value.as_u64().and_then(|value| i32::try_from(value).ok()))
    })
}

fn pending_f64(context: &ScriptSettingsUiContext, id: &str) -> Option<f64> {
    context
        .take_pending_value(id)
        .and_then(|value| value.as_f64())
        .filter(|value| value.is_finite())
}

fn pending_string(context: &ScriptSettingsUiContext, id: &str) -> Option<String> {
    context.take_pending_value(id).and_then(|value| value.as_str().map(clamped_string))
}

fn dropdown_items(lua: &Lua, value: Value) -> mlua::Result<Vec<String>> {
    let Value::Table(table) = value else {
        return Err(LuaError::RuntimeError("dropdown items must be a table".to_owned()));
    };
    let raw_count = table.raw_len();
    if raw_count == 0 {
        return Err(LuaError::RuntimeError("dropdown items must not be empty".to_owned()));
    }
    if raw_count > MAX_UI_DROPDOWN_ITEMS {
        return Err(LuaError::RuntimeError("dropdown items exceed the allowed limit".to_owned()));
    }

    let mut items = Vec::with_capacity(raw_count);
    let mut seen = HashSet::with_capacity(raw_count);
    for index in 1..=raw_count {
        let value: Value = table.raw_get(index)?;
        let Some(value) = lua.coerce_string(value)? else {
            return Err(LuaError::RuntimeError(format!("dropdown item {index} must be a string")));
        };
        let bytes = value.as_bytes();
        if bytes.is_empty() {
            return Err(LuaError::RuntimeError(format!("dropdown item {index} must not be empty")));
        }
        if bytes.len() > MAX_UI_STATE_STRING_BYTES {
            return Err(LuaError::RuntimeError(format!("dropdown item {index} is too long")));
        }
        if bytes.contains(&0) {
            return Err(LuaError::RuntimeError(format!(
                "dropdown item {index} must not contain embedded NUL bytes"
            )));
        }
        let item = value.to_string_lossy();
        if !seen.insert(item.clone()) {
            return Err(LuaError::RuntimeError(format!(
                "dropdown item {index} duplicates an earlier item"
            )));
        }
        items.push(item);
    }
    Ok(items)
}

fn dropdown_default(value: Option<Value>, items: &[String]) -> mlua::Result<String> {
    let Some(value) = value else {
        return Ok(items[0].clone());
    };
    match value {
        Value::Nil => Ok(items[0].clone()),
        Value::Integer(index) => {
            let Ok(index) = usize::try_from(index) else {
                return Err(LuaError::RuntimeError(
                    "default dropdown index is outside the allowed range".to_owned(),
                ));
            };
            if index == 0 || index > items.len() {
                return Err(LuaError::RuntimeError(
                    "default dropdown index is outside the allowed range".to_owned(),
                ));
            }
            Ok(items[index - 1].clone())
        }
        Value::String(value) => {
            let bytes = value.as_bytes();
            if bytes.len() <= MAX_UI_STATE_STRING_BYTES && !bytes.contains(&0) {
                let value = value.to_string_lossy();
                if items.contains(&value) {
                    return Ok(value);
                }
            }
            Ok(items[0].clone())
        }
        _ => Err(LuaError::RuntimeError(
            "default dropdown value must be a string, option index, or nil".to_owned(),
        )),
    }
}

fn default_keybind(value: Option<Value>) -> Hotkey {
    let Some(value) = value else {
        return Hotkey::new(SCRIPT_UNBOUND_HOTKEY);
    };
    match value {
        Value::Integer(value) if (0..=SCRIPT_INTEGER_MAX).contains(&value) => {
            normalize_script_hotkey(u32::try_from(value).unwrap_or_default())
        }
        Value::String(value) => parse_script_hotkey(&value.to_string_lossy()).map_or_else(
            || Hotkey::new(SCRIPT_UNBOUND_HOTKEY),
            |hotkey| normalize_script_hotkey(hotkey.raw()),
        ),
        _ => Hotkey::new(SCRIPT_UNBOUND_HOTKEY),
    }
}

fn stored_keybind(context: &ScriptSettingsUiContext, id: &str, default: Hotkey) -> (bool, Hotkey) {
    let stored = context.stored(id);
    let exists = stored.is_some();
    let value = stored
        .and_then(|value| {
            value
                .as_i64()
                .filter(|value| (0..=SCRIPT_INTEGER_MAX).contains(value))
                .and_then(|value| u32::try_from(value).ok())
                .or_else(|| {
                    value
                        .as_u64()
                        .filter(|value| i32::try_from(*value).is_ok())
                        .and_then(|value| u32::try_from(value).ok())
                })
        })
        .map_or_else(|| default, normalize_script_hotkey);
    (exists, value)
}

fn resolve_button_callback(lua: &Lua, value: Value) -> mlua::Result<Function> {
    if let Value::Function(function) = value {
        return Ok(function);
    }
    let Value::String(path) = value else {
        return Err(LuaError::RuntimeError(
            "button callback must be a function or string path".to_owned(),
        ));
    };
    let bytes = path.as_bytes();
    if bytes.is_empty() {
        return Err(LuaError::RuntimeError("button callback path must not be empty".to_owned()));
    }
    if bytes.contains(&0) {
        return Err(LuaError::RuntimeError(
            "button callback path must not contain embedded NUL bytes".to_owned(),
        ));
    }
    let path = path.to_string_lossy();
    let mut segments = path.split('.');
    let Some(first) = segments.next().filter(|segment| !segment.is_empty()) else {
        return Err(LuaError::RuntimeError(
            "button callback path must use non-empty dot-separated names".to_owned(),
        ));
    };
    let mut current: Value = lua.globals().get(first)?;
    for segment in segments {
        if segment.is_empty() {
            return Err(LuaError::RuntimeError(
                "button callback path must use non-empty dot-separated names".to_owned(),
            ));
        }
        let Value::Table(table) = current else {
            return Err(LuaError::RuntimeError(format!(
                "button callback path '{path}' does not resolve to a function"
            )));
        };
        current = table.get(segment)?;
    }
    let Value::Function(function) = current else {
        return Err(LuaError::RuntimeError(format!(
            "button callback path '{path}' does not resolve to a function"
        )));
    };
    Ok(function)
}

fn install_text_and_separator(
    lua: &Lua,
    ui: &Table,
    context: &ScriptSettingsUiContext,
) -> mlua::Result<()> {
    let text_context = context.clone();
    ui.set(
        "text",
        lua.create_function(move |lua, (text, width): (Value, Option<Value>)| {
            consume_control(&text_context)?;
            let text = clamped_string(&lua_string(lua, text, "text")?);
            let width = optional_dimension(lua, width, "width")?;
            text_context.record(|capture| capture.record_text(text, width));
            Ok(())
        })?,
    )?;

    let separator_context = context.clone();
    ui.set(
        "separator",
        lua.create_function(move |lua, height: Option<Value>| {
            consume_control(&separator_context)?;
            let height = optional_dimension(lua, height, "height")?;
            separator_context.record(|capture| capture.record_separator(height));
            Ok(())
        })?,
    )
}

fn install_checkbox(
    lua: &Lua,
    ui: &Table,
    execution: &ScriptInputContext,
    context: &ScriptSettingsUiContext,
) -> mlua::Result<()> {
    let execution = execution.clone();
    let context = context.clone();
    ui.set(
        "checkbox",
        lua.create_function(
            move |lua, (id, label, default, width): (Value, Value, Option<Value>, Option<Value>)| {
                consume_control(&context)?;
                let id = ui_id(lua, id)?;
                let label = lua_string(lua, label, "label")?;
                let default = default.is_some_and(|value| !matches!(value, Value::Nil | Value::Boolean(false)));
                let width = optional_dimension(lua, width, "width")?;
                context.record(|capture| capture.record_checkbox(id.clone(), label, default, width));
                register_id(&context, &id)?;

                let (exists, mut value) = stored_bool(&context, &id, default);
                if !exists {
                    update_setting(lua, &context, &id, &JsonValue::Bool(value))?;
                }
                if execution.mode() == ScriptExecutionMode::SettingsRender
                    && let Some(next) = pending_bool(&context, &id)
                {
                    value = next;
                    update_setting(lua, &context, &id, &JsonValue::Bool(value))?;
                }
                Ok(value)
            },
        )?,
    )
}

fn install_slider_int(
    lua: &Lua,
    ui: &Table,
    execution: &ScriptInputContext,
    context: &ScriptSettingsUiContext,
) -> mlua::Result<()> {
    let int_execution = execution.clone();
    let int_context = context.clone();
    ui.set(
        "sliderInt",
        lua.create_function(
            move |lua,
                  (id, label, default, minimum, maximum, width): (
                Value,
                Value,
                Option<Value>,
                Option<Value>,
                Option<Value>,
                Option<Value>,
            )| {
                consume_control(&int_context)?;
                let id = ui_id(lua, id)?;
                let label = lua_string(lua, label, "label")?;
                let default = optional_i32(lua, default, 0, "default value")?;
                let mut minimum = optional_i32(lua, minimum, default, "min value")?;
                let mut maximum = optional_i32(lua, maximum, default, "max value")?;
                if minimum > maximum {
                    std::mem::swap(&mut minimum, &mut maximum);
                }
                let width = optional_dimension(lua, width, "width")?;
                int_context.record(|capture| {
                    capture.record_slider_int(id.clone(), label, default, minimum, maximum, width);
                });
                register_id(&int_context, &id)?;
                let (exists, mut value) = stored_i32(&int_context, &id, default);
                if !exists {
                    update_setting(
                        lua,
                        &int_context,
                        &id,
                        &JsonValue::Number(Number::from(value)),
                    )?;
                }
                if int_execution.mode() == ScriptExecutionMode::SettingsRender
                    && let Some(next) = pending_i32(&int_context, &id)
                {
                    value = next.clamp(minimum, maximum);
                    update_setting(
                        lua,
                        &int_context,
                        &id,
                        &JsonValue::Number(Number::from(value)),
                    )?;
                }
                Ok(value)
            },
        )?,
    )
}

fn install_slider_float(
    lua: &Lua,
    ui: &Table,
    execution: &ScriptInputContext,
    context: &ScriptSettingsUiContext,
) -> mlua::Result<()> {
    let float_execution = execution.clone();
    let float_context = context.clone();
    ui.set(
        "sliderFloat",
        lua.create_function(
            move |lua,
                  (id, label, default, minimum, maximum, width): (
                Value,
                Value,
                Option<Value>,
                Option<Value>,
                Option<Value>,
                Option<Value>,
            )| {
                consume_control(&float_context)?;
                let id = ui_id(lua, id)?;
                let label = lua_string(lua, label, "label")?;
                let default = optional_finite_number(lua, default, 0.0, "default value")?;
                let mut minimum = optional_finite_number(lua, minimum, default, "min value")?;
                let mut maximum = optional_finite_number(lua, maximum, default, "max value")?;
                if minimum > maximum {
                    std::mem::swap(&mut minimum, &mut maximum);
                }
                minimum = minimum.clamp(-FLOAT_MAX_AS_F64, FLOAT_MAX_AS_F64);
                maximum = maximum.clamp(-FLOAT_MAX_AS_F64, FLOAT_MAX_AS_F64);
                let width = optional_dimension(lua, width, "width")?;
                float_context.record(|capture| {
                    capture.record_slider_float(
                        id.clone(),
                        label,
                        default,
                        minimum,
                        maximum,
                        width,
                    );
                });
                register_id(&float_context, &id)?;
                let (exists, mut value) = stored_float(&float_context, &id, default);
                if !exists {
                    update_setting(
                        lua,
                        &float_context,
                        &id,
                        &JsonValue::Number(
                            Number::from_f64(value).unwrap_or_else(|| Number::from(0)),
                        ),
                    )?;
                }
                if float_execution.mode() == ScriptExecutionMode::SettingsRender
                    && let Some(next) = pending_f64(&float_context, &id)
                {
                    #[allow(clippy::cast_possible_truncation)]
                    let rendered = f64::from(next.clamp(minimum, maximum) as f32);
                    value = rendered;
                    let number = Number::from_f64(value).unwrap_or_else(|| Number::from(0));
                    update_setting(lua, &float_context, &id, &JsonValue::Number(number))?;
                }
                Ok(value)
            },
        )?,
    )
}

fn install_dropdown(
    lua: &Lua,
    ui: &Table,
    execution: &ScriptInputContext,
    context: &ScriptSettingsUiContext,
) -> mlua::Result<()> {
    let execution = execution.clone();
    let context = context.clone();
    ui.set(
        "dropdown",
        lua.create_function(
            move |lua,
                  (id, label, items, default, width): (
                Value,
                Value,
                Value,
                Option<Value>,
                Option<Value>,
            )| {
                consume_control(&context)?;
                let id = ui_id(lua, id)?;
                let label = lua_string(lua, label, "label")?;
                let items = dropdown_items(lua, items)?;
                let default = dropdown_default(default, &items)?;
                let width = optional_dimension(lua, width, "width")?;
                context.record(|capture| {
                    capture.record_dropdown(
                        id.clone(),
                        label,
                        items.clone(),
                        default.clone(),
                        width,
                    );
                });
                register_id(&context, &id)?;

                let (exists, mut stored) = stored_string(&context, &id, &default);
                if !items.contains(&stored) {
                    stored.clone_from(&default);
                    update_setting(lua, &context, &id, &JsonValue::String(stored.clone()))?;
                } else if !exists {
                    update_setting(lua, &context, &id, &JsonValue::String(stored.clone()))?;
                }
                if execution.mode() == ScriptExecutionMode::SettingsRender
                    && let Some(next) =
                        pending_string(&context, &id).filter(|value| items.contains(value))
                {
                    stored = next;
                    update_setting(lua, &context, &id, &JsonValue::String(stored.clone()))?;
                }
                Ok(stored)
            },
        )?,
    )
}

fn install_textboxes(
    lua: &Lua,
    ui: &Table,
    execution: &ScriptInputContext,
    context: &ScriptSettingsUiContext,
) -> mlua::Result<()> {
    let textbox_execution = execution.clone();
    let textbox_context = context.clone();
    ui.set(
        "textbox",
        lua.create_function(
            move |lua,
                  (id, label, default, width, height): (
                Value,
                Value,
                Option<Value>,
                Option<Value>,
                Option<Value>,
            )| {
                consume_control(&textbox_context)?;
                let id = ui_id(lua, id)?;
                let label = lua_string(lua, label, "label")?;
                let default = match default {
                    None | Some(Value::Nil) => String::new(),
                    Some(value) => clamped_string(&lua_string(lua, value, "default value")?),
                };
                let width = optional_dimension(lua, width, "width")?;
                let height = optional_dimension(lua, height, "height")?;
                textbox_context.record(|capture| {
                    capture.record_textbox(id.clone(), label, default.clone(), width, height);
                });
                register_id(&textbox_context, &id)?;
                let (exists, mut stored) = stored_string(&textbox_context, &id, &default);
                if !exists {
                    update_setting(lua, &textbox_context, &id, &JsonValue::String(stored.clone()))?;
                }
                if textbox_execution.mode() == ScriptExecutionMode::SettingsRender
                    && let Some(next) = pending_string(&textbox_context, &id)
                {
                    stored = next;
                    update_setting(lua, &textbox_context, &id, &JsonValue::String(stored.clone()))?;
                }
                Ok(stored)
            },
        )?,
    )?;

    let dynamic_context = context.clone();
    ui.set(
        "dynamicTextbox",
        lua.create_function(
            move |lua,
                  (id, label, default, width, height): (
                Value,
                Value,
                Option<Value>,
                Option<Value>,
                Option<Value>,
            )| {
                consume_control(&dynamic_context)?;
                let id = ui_id(lua, id)?;
                let label = lua_string(lua, label, "label")?;
                let default = match default {
                    None | Some(Value::Nil) => String::new(),
                    Some(value) => clamped_string(&lua_string(lua, value, "default value")?),
                };
                let width = optional_dimension(lua, width, "width")?;
                let height = optional_dimension(lua, height, "height")?;
                dynamic_context.record(|capture| {
                    capture.record_dynamic_textbox(
                        id.clone(),
                        label,
                        default.clone(),
                        width,
                        height,
                    );
                });
                register_id(&dynamic_context, &id)?;
                let (exists, stored) = stored_string(&dynamic_context, &id, &default);
                let stored = clamp_dynamic_text(stored);
                if !exists {
                    update_setting(lua, &dynamic_context, &id, &JsonValue::String(stored.clone()))?;
                }
                Ok(stored)
            },
        )?,
    )?;

    let set_context = context.clone();
    ui.set(
        "setDynamicText",
        lua.create_function(move |lua, (id, text): (Value, Value)| {
            let id = ui_id(lua, id)?;
            let text = clamped_string(&lua_string(lua, text, "text")?);
            register_id(&set_context, &id)?;
            let value = clamp_dynamic_text(text);
            let _ = set_context.set_transient(&id, &value);
            update_setting(lua, &set_context, &id, &JsonValue::String(value))?;
            Ok(true)
        })?,
    )
}

fn install_keybind(
    lua: &Lua,
    ui: &Table,
    execution: &ScriptInputContext,
    context: &ScriptSettingsUiContext,
) -> mlua::Result<()> {
    let execution = execution.clone();
    let context = context.clone();
    let function = lua.create_function(
        move |lua, (id, label, default, width): (Value, Value, Option<Value>, Option<Value>)| {
            consume_control(&context)?;
            let id = ui_id(lua, id)?;
            let label = lua_string(lua, label, "label")?;
            let default = default_keybind(default);
            let width = optional_dimension(lua, width, "width")?;
            context.record(|capture| capture.record_keybind(id.clone(), label, default, width));
            register_id(&context, &id)?;
            let (exists, stored) = stored_keybind(&context, &id, default);
            if !exists {
                update_setting(lua, &context, &id, &JsonValue::Number(Number::from(stored.raw())))?;
            }

            let mut value = context.keybind_value(&id, stored);
            if value != stored {
                value = stored;
                context.set_keybind_value(&id, value);
            }
            if execution.mode() == ScriptExecutionMode::SettingsRender
                && let Some(next) = pending_i32(&context, &id)
            {
                let raw = u32::try_from(next).unwrap_or_default();
                value = normalize_script_hotkey(raw);
                context.set_keybind_value(&id, value);
            }
            let normalized = normalize_script_hotkey(value.raw());
            if normalized != stored {
                update_setting(
                    lua,
                    &context,
                    &id,
                    &JsonValue::Number(Number::from(normalized.raw())),
                )?;
                context.set_keybind_value(&id, normalized);
            }
            Ok(normalized.raw())
        },
    )?;
    ui.set("keybind", function.clone())?;
    ui.set("hotkey", function.clone())?;
    ui.set("keyCombo", function)
}

fn install_button(
    lua: &Lua,
    ui: &Table,
    execution: &ScriptInputContext,
    context: &ScriptSettingsUiContext,
) -> mlua::Result<()> {
    let execution = execution.clone();
    let context = context.clone();
    ui.set(
        "button",
        lua.create_function(move |lua, arguments: mlua::MultiValue| {
            consume_control(&context)?;
            let id = ui_id(lua, arguments.front().cloned().unwrap_or(Value::Nil))?;
            let label = lua_string(lua, arguments.get(1).cloned().unwrap_or(Value::Nil), "label")?;

            let third = arguments.get(2).cloned();
            let callback = third
                .as_ref()
                .is_some_and(|value| matches!(value, Value::Function(_) | Value::String(_)))
                .then_some(2_usize);
            let has_explicit_callback_slot =
                callback.is_some() || (arguments.len() >= 5 && matches!(third, Some(Value::Nil)));
            let (width_index, height_index) =
                if has_explicit_callback_slot { (3_usize, 4_usize) } else { (2_usize, 3_usize) };
            let width = optional_dimension(lua, arguments.get(width_index).cloned(), "width")?;
            let height = optional_dimension(lua, arguments.get(height_index).cloned(), "height")?;
            context.record(|capture| capture.record_button(id.clone(), label, width, height));
            register_id(&context, &id)?;

            let pressed = execution.mode() == ScriptExecutionMode::SettingsRender
                && context.take_button_press(&id);
            if pressed && let Some(index) = callback {
                let callback = resolve_button_callback(
                    lua,
                    arguments.get(index).cloned().unwrap_or(Value::Nil),
                )?;
                callback.call::<()>(id)?;
            }
            Ok(pressed)
        })?,
    )
}

/// Install the historical `settings` global and `ui` table.
///
/// # Errors
///
/// Returns a Lua registration error if state/functions cannot be installed.
pub fn install_settings_ui_api(
    vm: &LuaScriptVm,
    execution_context: &ScriptInputContext,
    context: &ScriptSettingsUiContext,
) -> Result<(), ScriptSettingsUiApiError> {
    sync_settings_table(&vm.lua, context)?;
    let ui = vm.lua.create_table()?;
    install_text_and_separator(&vm.lua, &ui, context)?;
    install_checkbox(&vm.lua, &ui, execution_context, context)?;
    install_slider_int(&vm.lua, &ui, execution_context, context)?;
    install_slider_float(&vm.lua, &ui, execution_context, context)?;
    install_dropdown(&vm.lua, &ui, execution_context, context)?;
    install_textboxes(&vm.lua, &ui, execution_context, context)?;
    install_keybind(&vm.lua, &ui, execution_context, context)?;
    install_button(&vm.lua, &ui, execution_context, context)?;
    vm.lua.globals().set("ui", ui)?;
    Ok(())
}

/// Run `onSettings` with the same five-second budget and capture commit behavior as C++.
///
/// Returns `Ok(false)` when the script does not define `onSettings`.
///
/// # Errors
///
/// Returns the Lua callback failure after restoring normal execution mode and retaining the last
/// successfully committed control capture.
pub fn call_on_settings(
    vm: &LuaScriptVm,
    execution_context: &ScriptInputContext,
    context: &ScriptSettingsUiContext,
    render_mode: bool,
) -> Result<bool, LuaExecutionError> {
    if !vm.has_function("onSettings") {
        return Ok(false);
    }
    context.begin_capture();
    let previous_mode = execution_context.mode();
    execution_context.set_mode(if render_mode {
        ScriptExecutionMode::SettingsRender
    } else {
        ScriptExecutionMode::SettingsCallback
    });
    let result = vm.call_no_args("onSettings", MAX_SETTINGS_RUNTIME);
    execution_context.set_mode(previous_mode);
    context.finish_capture(result.is_ok());
    if result.is_ok()
        && vm.stop_handle().reason() == ScriptStopReason::None
        && let Err(error) = sync_ui_state_from_lua_settings(vm, context)
    {
        return Err(LuaExecutionError {
            kind: crate::LuaExecutionErrorKind::Runtime,
            message: error.to_string(),
        });
    }
    result.map(|()| true)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::SettingsUiControlKind;
    use serde_json::json;

    fn installed(
        initial: &JsonValue,
    ) -> (LuaScriptVm, ScriptInputContext, ScriptSettingsUiContext) {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let execution = ScriptInputContext::default();
        let context = ScriptSettingsUiContext::from_ui_state(initial);
        install_settings_ui_api(&vm, &execution, &context).expect("settings UI API should install");
        (vm, execution, context)
    }

    #[test]
    fn initial_settings_table_contains_only_supported_scalars() {
        let (vm, _, _) = installed(&json!({
            "flag": true,
            "count": 5,
            "ratio": 1.25,
            "text": "hello",
            "array": [1]
        }));
        assert_eq!(
            vm.eval::<(bool, i64, f64, String, bool)>(
                "return settings.flag,settings.count,settings.ratio,settings.text,settings.array==nil",
                MAX_SETTINGS_RUNTIME,
            )
            .expect("settings should be readable"),
            (true, 5, 1.25, "hello".to_owned(), true)
        );
    }

    #[test]
    fn non_render_settings_call_initializes_values_and_captures_controls() {
        let (vm, execution, context) = installed(&json!({}));
        vm.execute_source(
            concat!(
                "function onSettings() ",
                "a=ui.checkbox('flag','Flag',true,50); ",
                "b=ui.sliderInt('count','Count',5,0,10,100); ",
                "c=ui.sliderFloat('ratio','Ratio',1.5,0,2); ",
                "d=ui.dropdown('choice','Choice',{'one','two'},2); ",
                "e=ui.textbox('text','Text','hello'); ",
                "f=ui.dynamicTextbox('dyn','Dynamic','tail'); ",
                "g=ui.keybind('bind','Bind','Ctrl+A'); ",
                "ui.text('hello'); ui.separator(2); ui.button('go','Go') end"
            ),
            "settings.lua",
        )
        .expect("settings callback should load");
        assert_eq!(call_on_settings(&vm, &execution, &context, false), Ok(true));
        assert_eq!(context.controls().len(), 10);
        assert_eq!(context.controls()[0].kind, SettingsUiControlKind::Checkbox);
        assert_eq!(context.controls()[7].kind, SettingsUiControlKind::Text);
        assert_eq!(
            context.ui_state(),
            json!({
                "bind": 131_137,
                "choice": "two",
                "count": 5,
                "dyn": "tail",
                "flag": true,
                "ratio": 1.5,
                "text": "hello"
            })
        );
        assert_eq!(
            vm.eval::<(bool, i64, f64, String, String, String, u32)>(
                "return a,b,c,d,e,f,g",
                MAX_SETTINGS_RUNTIME,
            )
            .expect("UI values should be returned"),
            (true, 5, 1.5, "two".to_owned(), "hello".to_owned(), "tail".to_owned(), 131_137)
        );
    }

    #[test]
    fn wrong_stored_types_return_defaults_without_rewriting_existing_values() {
        let (vm, execution, context) = installed(&json!({
            "flag": "wrong",
            "count": "wrong",
            "ratio": "wrong",
            "text": false
        }));
        vm.execute_source(
            concat!(
                "function onSettings() ",
                "a=ui.checkbox('flag','Flag',true); ",
                "b=ui.sliderInt('count','Count',7); ",
                "c=ui.sliderFloat('ratio','Ratio',2.5); ",
                "d=ui.textbox('text','Text','fallback') end"
            ),
            "wrong-types.lua",
        )
        .expect("callback should load");
        call_on_settings(&vm, &execution, &context, false).expect("settings call should run");
        assert_eq!(
            vm.eval::<(bool, i64, f64, String)>("return a,b,c,d", MAX_SETTINGS_RUNTIME)
                .expect("defaults should return"),
            (true, 7, 2.5, "fallback".to_owned())
        );
        assert_eq!(
            context.ui_state(),
            json!({"flag":"wrong","count":"wrong","ratio":"wrong","text":false})
        );
    }

    #[test]
    fn render_mode_consumes_queued_edits_and_button_callback() {
        let (vm, execution, context) = installed(&json!({}));
        vm.execute_source(
            concat!(
                "clicked=''; function handlers_go(id) clicked=id end; ",
                "handlers={go=function(id) clicked='path:'..id end}; ",
                "function onSettings() ",
                "flag=ui.checkbox('flag','Flag',false); ",
                "count=ui.sliderInt('count','Count',0,-5,5); ",
                "choice=ui.dropdown('choice','Choice',{'one','two'},1); ",
                "pressed=ui.button('go','Go','handlers.go',80,20) end"
            ),
            "render.lua",
        )
        .expect("callback should load");
        call_on_settings(&vm, &execution, &context, false).expect("seed settings should run");
        context.queue_value("flag", json!(true));
        context.queue_value("count", json!(50));
        context.queue_value("choice", json!("two"));
        context.queue_button_press("go");
        call_on_settings(&vm, &execution, &context, true).expect("render settings should run");
        assert_eq!(
            vm.eval::<(bool, i64, String, bool, String)>(
                "return flag,count,choice,pressed,clicked",
                MAX_SETTINGS_RUNTIME,
            )
            .expect("render values should return"),
            (true, 5, "two".to_owned(), true, "path:go".to_owned())
        );
    }

    #[test]
    fn failed_capture_retains_previous_controls() {
        let (vm, execution, context) = installed(&json!({}));
        vm.execute_source(
            "fail=false; function onSettings() ui.text(fail and 'new' or 'old'); if fail then error('boom') end end",
            "capture.lua",
        )
        .expect("callback should load");
        call_on_settings(&vm, &execution, &context, false).expect("initial capture should succeed");
        assert_eq!(context.controls()[0].text, "old");
        vm.execute_source("fail=true", "toggle.lua").expect("flag should set");
        assert!(call_on_settings(&vm, &execution, &context, false).is_err());
        assert_eq!(context.controls()[0].text, "old");
        assert_eq!(execution.mode(), ScriptExecutionMode::Normal);
    }

    #[test]
    fn arbitrary_settings_writes_sync_back_after_callback() {
        let (vm, execution, context) = installed(&json!({"old":1}));
        vm.execute_source(
            concat!(
                "function onSettings() settings.bool=true; settings.num=4.5; ",
                "settings.text=string.rep('x',5000); settings.bad={1}; settings['']=3 end"
            ),
            "sync.lua",
        )
        .expect("callback should load");
        call_on_settings(&vm, &execution, &context, false).expect("settings sync should run");
        let state = context.ui_state();
        let object = state.as_object().expect("state should be an object");
        assert_eq!(object["bool"], json!(true));
        assert_eq!(object["num"], json!(4.5));
        assert_eq!(object["text"].as_str().map(str::len), Some(MAX_UI_STATE_STRING_BYTES));
        assert!(!object.contains_key("bad"));
        assert!(!object.contains_key(""));
        assert_eq!(object["old"], json!(1));
    }

    #[test]
    fn dropdown_validation_and_reset_match_cpp_rules() {
        let (vm, execution, context) = installed(&json!({"choice":"missing"}));
        vm.execute_source(
            "function onSettings() choice=ui.dropdown('choice','Choice',{'one','two'},'two') end",
            "dropdown.lua",
        )
        .expect("callback should load");
        call_on_settings(&vm, &execution, &context, false).expect("dropdown should run");
        assert_eq!(
            vm.eval::<String>("return choice", MAX_SETTINGS_RUNTIME).expect("choice should return"),
            "two"
        );
        assert_eq!(context.ui_state(), json!({"choice":"two"}));

        for source in [
            "ui.dropdown('x','X',{})",
            "ui.dropdown('x','X',{'a','a'})",
            "ui.dropdown('x','X',{'a'},2)",
        ] {
            let error = vm
                .execute_source(source, "invalid-dropdown.lua")
                .expect_err("invalid dropdown should fail");
            assert!(!error.message.is_empty());
        }
    }

    #[test]
    fn set_dynamic_text_does_not_consume_control_budget() {
        let (vm, _, context) = installed(&json!({}));
        vm.execute_source("for i=1,600 do ui.setDynamicText('dyn','value') end", "dynamic.lua")
            .expect("setDynamicText should not consume control budget");
        assert_eq!(context.ui_state(), json!({"dyn":"value"}));
    }

    #[test]
    fn control_budget_is_checked_before_argument_validation() {
        let (vm, execution, context) = installed(&json!({}));
        vm.execute_source(
            concat!(
                "function onSettings() for i=1,512 do ui.text('x') end; ",
                "ui.checkbox('',nil) end"
            ),
            "budget.lua",
        )
        .expect("callback should load");
        let error = call_on_settings(&vm, &execution, &context, false)
            .expect_err("513th control should fail budget first");
        assert!(error.message.contains("script settings created too many UI controls"));
    }

    #[test]
    fn unique_id_budget_counts_existing_state_keys() {
        let mut initial = Map::new();
        for index in 0..MAX_UI_STATE_ENTRIES {
            initial.insert(format!("id-{index}"), json!(true));
        }
        let (vm, _, context) = installed(&JsonValue::Object(initial));
        let error = vm
            .execute_source("ui.setDynamicText('overflow','x')", "ids.lua")
            .expect_err("new id over state budget should fail");
        assert!(error.message.contains("script settings created too many unique UI IDs"));
        assert_eq!(context.ui_state().as_object().map(Map::len), Some(MAX_UI_STATE_ENTRIES));
    }

    #[test]
    fn button_callback_path_validation_is_deferred_until_press() {
        let (vm, execution, context) = installed(&json!({}));
        vm.execute_source(
            "function onSettings() pressed=ui.button('go','Go','missing.path') end",
            "button.lua",
        )
        .expect("callback should load");
        call_on_settings(&vm, &execution, &context, false)
            .expect("non-render capture must not resolve callback path");
        context.queue_button_press("go");
        assert!(
            call_on_settings(&vm, &execution, &context, true)
                .expect_err("pressed button should resolve missing path")
                .message
                .contains("button callback path 'missing.path' does not resolve to a function")
        );
    }

    #[test]
    fn keybind_aliases_share_state_and_unbound_sentinel() {
        let (vm, _, context) = installed(&json!({}));
        assert_eq!(
            vm.eval::<(u32, u32, u32)>(
                "return ui.keybind('a','A'),ui.hotkey('b','B','bad'),ui.keyCombo('c','C',131137)",
                MAX_SETTINGS_RUNTIME,
            )
            .expect("keybind aliases should return"),
            (SCRIPT_UNBOUND_HOTKEY, SCRIPT_UNBOUND_HOTKEY, 131_137)
        );
        assert_eq!(context.controls().len(), 0, "capture is inactive outside onSettings");
    }

    #[test]
    fn dimensions_clamp_and_negative_values_become_zero() {
        let (vm, _, _) = installed(&json!({}));
        assert_eq!(
            optional_dimension(&vm.lua, Some(Value::Number(-1.0)), "width")
                .expect("negative dimension should parse")
                .to_bits(),
            0.0_f32.to_bits()
        );
        assert_eq!(
            optional_dimension(&vm.lua, Some(Value::Number(50_000.0)), "width")
                .expect("large dimension should parse")
                .to_bits(),
            MAX_UI_DIMENSION.to_bits()
        );
    }
}

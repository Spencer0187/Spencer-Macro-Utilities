//! Pointer/display Lua API adapters.

use crate::input_api::{ScriptExecutionMode, ScriptInputContext, ScriptMouseMotionMode};
use crate::lua_runtime::{LuaScriptVm, LuaStopHandle, ScriptStopReason};
use mlua::{Error as LuaError, Lua, Value};
use smu_display::{
    CoordinateMode, DisplayCapture, DisplayError, PixelColor, resolve_absolute_point,
    resolve_pixel_rect, sample_pixel_rect,
};
use smu_input::InputDevice;
use smu_profile::{SavedSettingSource, SavedSettingValue};
use std::fmt;
use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "macos")]
static MACOS_CURSOR_MOVEMENT: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum PixelOutputFormat {
    #[default]
    Hex,
    Rgb,
}

#[derive(Debug)]
pub struct ScriptDisplayApiError(String);

impl ScriptDisplayApiError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ScriptDisplayApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ScriptDisplayApiError {}

impl From<LuaError> for ScriptDisplayApiError {
    fn from(error: LuaError) -> Self {
        Self::new(error.to_string())
    }
}

fn check_settings_callback(context: &ScriptInputContext, function_name: &str) -> mlua::Result<()> {
    if matches!(
        context.mode(),
        ScriptExecutionMode::SettingsRender | ScriptExecutionMode::SettingsCallback
    ) {
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

fn finite_number(lua: &Lua, value: Value, name: &str) -> mlua::Result<f64> {
    let Some(number) = lua.coerce_number(value)? else {
        return Err(LuaError::RuntimeError(format!("{name} must be a number")));
    };
    if !number.is_finite() {
        return Err(LuaError::RuntimeError(format!("{name} must be a finite number")));
    }
    Ok(number)
}

fn integer(lua: &Lua, value: Value, name: &str) -> mlua::Result<i32> {
    let Some(number) = lua.coerce_integer(value)? else {
        return Err(LuaError::RuntimeError(format!("{name} must be an integer")));
    };
    i32::try_from(number)
        .map_err(|_| LuaError::RuntimeError(format!("{name} is outside the allowed range")))
}

#[derive(Clone, Debug, PartialEq)]
pub struct MoveDegreesCalibration {
    pixels_per_degree: Option<f64>,
    error: String,
}

impl MoveDegreesCalibration {
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        self.pixels_per_degree.is_some()
    }

    #[must_use]
    pub const fn pixels_per_degree(&self) -> Option<f64> {
        self.pixels_per_degree
    }

    #[must_use]
    pub fn error(&self) -> &str {
        &self.error
    }
}

const fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn parse_hex_float(text: &str) -> Option<f64> {
    let bytes = text.as_bytes();
    let mut index = 0_usize;
    let negative = match bytes.first() {
        Some(b'+') => {
            index = 1;
            false
        }
        Some(b'-') => {
            index = 1;
            true
        }
        _ => false,
    };
    if bytes.get(index..index + 2).is_none_or(|prefix| prefix != b"0x" && prefix != b"0X") {
        return None;
    }
    index += 2;

    let mut significand = 0.0_f64;
    let mut digits = 0_usize;
    while let Some(&byte) = bytes.get(index) {
        let Some(digit) = hex_digit(byte) else {
            break;
        };
        #[allow(clippy::suboptimal_flops)]
        {
            #[allow(clippy::suboptimal_flops)]
            {
                significand = significand * 16.0 + f64::from(digit);
            }
        }
        digits += 1;
        index += 1;
    }

    let mut fractional_digits = 0_usize;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        while let Some(&byte) = bytes.get(index) {
            let Some(digit) = hex_digit(byte) else {
                break;
            };
            #[allow(clippy::suboptimal_flops)]
            {
                #[allow(clippy::suboptimal_flops)]
                {
                    significand = significand * 16.0 + f64::from(digit);
                }
            }
            digits += 1;
            fractional_digits += 1;
            index += 1;
        }
    }
    if digits == 0 {
        return None;
    }

    let mut exponent = 0_i64;
    if matches!(bytes.get(index), Some(b'p' | b'P')) {
        index += 1;
        let exponent_negative = match bytes.get(index) {
            Some(b'+') => {
                index += 1;
                false
            }
            Some(b'-') => {
                index += 1;
                true
            }
            _ => false,
        };
        let exponent_start = index;
        while let Some(&byte) = bytes.get(index) {
            if !byte.is_ascii_digit() {
                break;
            }
            exponent = exponent.saturating_mul(10).saturating_add(i64::from(byte - b'0'));
            index += 1;
        }
        if index == exponent_start {
            return None;
        }
        if exponent_negative {
            exponent = exponent.saturating_neg();
        }
    }
    if index != bytes.len() {
        return None;
    }

    let fractional_bits = i64::try_from(fractional_digits).unwrap_or(i64::MAX).saturating_mul(4);
    let binary_exponent = exponent.saturating_sub(fractional_bits);
    let exponent_i32 = i32::try_from(binary_exponent)
        .unwrap_or_else(|_| if binary_exponent.is_negative() { i32::MIN } else { i32::MAX });
    let value = significand * 2.0_f64.powi(exponent_i32);
    let value = if negative { -value } else { value };
    value.is_finite().then_some(value)
}

fn parse_cpp_finite_double(text: &str) -> Option<f64> {
    let text = text.trim_matches([' ', '\t', '\r', '\n']);
    if text.is_empty() {
        return None;
    }
    text.parse::<f64>().ok().filter(|value| value.is_finite()).or_else(|| parse_hex_float(text))
}

/// Cache the settings used by C++ `moveDegrees` when the script API is registered.
#[must_use]
pub fn cache_move_degrees_calibration(
    settings: Option<&dyn SavedSettingSource>,
) -> MoveDegreesCalibration {
    let camfix = settings.and_then(|settings| settings.get_saved_setting("camfixtoggle")).and_then(
        |value| match value {
            SavedSettingValue::Bool(value) => Some(value),
            SavedSettingValue::Integer(_)
            | SavedSettingValue::Float(_)
            | SavedSettingValue::String(_) => None,
        },
    );
    if camfix.is_none() {
        return MoveDegreesCalibration {
            pixels_per_degree: None,
            error: "moveDegrees requires the saved camfixtoggle setting.".to_owned(),
        };
    }

    let sensitivity = settings
        .and_then(|settings| settings.get_saved_setting("RobloxSensValue"))
        .and_then(|value| match value {
            SavedSettingValue::String(value) => Some(value),
            SavedSettingValue::Bool(_)
            | SavedSettingValue::Integer(_)
            | SavedSettingValue::Float(_) => None,
        });
    let Some(sensitivity) = sensitivity else {
        return MoveDegreesCalibration {
            pixels_per_degree: None,
            error: "moveDegrees requires the saved RobloxSensValue setting.".to_owned(),
        };
    };
    let Some(sensitivity) = parse_cpp_finite_double(&sensitivity).filter(|value| *value > 0.0)
    else {
        return MoveDegreesCalibration {
            pixels_per_degree: None,
            error: "moveDegrees requires RobloxSensValue to be a finite number greater than 0."
                .to_owned(),
        };
    };

    let base_value = if camfix == Some(true) { 1000.0 } else { 720.0 };
    MoveDegreesCalibration {
        pixels_per_degree: Some(base_value / (360.0 * sensitivity)),
        error: String::new(),
    }
}

fn round_mouse_delta(pixels: f64, name: &str) -> mlua::Result<i32> {
    if !pixels.is_finite() {
        return Err(LuaError::RuntimeError(format!("{name} conversion overflowed")));
    }
    let rounded = pixels.round();
    if rounded < f64::from(i32::MIN) || rounded > f64::from(i32::MAX) {
        return Err(LuaError::RuntimeError(format!(
            "{name} conversion is outside the allowed range"
        )));
    }
    #[allow(clippy::cast_possible_truncation)]
    Ok(rounded as i32)
}

fn coordinate_mode(value: Option<Value>) -> mlua::Result<CoordinateMode> {
    let Some(value) = value else {
        return Ok(CoordinateMode::Pixels);
    };
    if matches!(value, Value::Nil) {
        return Ok(CoordinateMode::Pixels);
    }
    let Value::String(text) = value else {
        return Err(LuaError::RuntimeError("mode must be a string".to_owned()));
    };
    CoordinateMode::parse(&text.to_string_lossy())
        .map_err(|error| LuaError::RuntimeError(error.to_string()))
}

fn pixel_output_format(value: Option<Value>) -> mlua::Result<PixelOutputFormat> {
    let Some(value) = value else {
        return Ok(PixelOutputFormat::Hex);
    };
    if matches!(value, Value::Nil) {
        return Ok(PixelOutputFormat::Hex);
    }
    let Value::String(text) = value else {
        return Err(LuaError::RuntimeError("pixel format must be \"hex\" or \"rgb\"".to_owned()));
    };
    match text.to_string_lossy().as_ref() {
        "hex" => Ok(PixelOutputFormat::Hex),
        "rgb" => Ok(PixelOutputFormat::Rgb),
        _ => Err(LuaError::RuntimeError("pixel format must be \"hex\" or \"rgb\"".to_owned())),
    }
}

fn pixel_to_lua(lua: &Lua, color: PixelColor, format: PixelOutputFormat) -> mlua::Result<Value> {
    match format {
        PixelOutputFormat::Hex => Ok(Value::String(lua.create_string(color.legacy_hex())?)),
        PixelOutputFormat::Rgb => {
            let table = lua.create_table()?;
            table.set("r", color.r)?;
            table.set("g", color.g)?;
            table.set("b", color.b)?;
            Ok(Value::Table(table))
        }
    }
}

fn pointer_reason(
    display: Option<&Arc<dyn DisplayCapture>>,
    input: Option<&Arc<dyn InputDevice>>,
) -> String {
    if let Some(display) = display {
        let reason = display.pointer_unavailable_reason();
        if !reason.is_empty() {
            return reason;
        }
    }
    if let Some(input) = input {
        let reason = input.absolute_pointer_unavailable_reason();
        if !reason.is_empty() {
            return reason;
        }
    }
    "cursor/screen position is not available on this platform/session".to_owned()
}

fn active_bounds(
    display: Option<&Arc<dyn DisplayCapture>>,
) -> Result<smu_display::ScreenBounds, String> {
    let Some(display) = display else {
        return Err("input backend is not available".to_owned());
    };
    let Some(bounds) = display.active_monitor_bounds().filter(|bounds| bounds.is_valid()) else {
        let reason = display.pointer_unavailable_reason();
        return Err(if reason.is_empty() {
            "active monitor bounds are not available on this platform/session".to_owned()
        } else {
            reason
        });
    };
    Ok(bounds)
}

fn absolute_delta(
    input: Option<&Arc<dyn InputDevice>>,
    display: Option<&Arc<dyn DisplayCapture>>,
    dx: i32,
    dy: i32,
) -> Result<(), String> {
    let Some(input) = input else {
        return Err("input backend is not available".to_owned());
    };
    let Some(cursor) = display.and_then(|display| display.cursor_position()) else {
        return Err(pointer_reason(display, Some(input)));
    };
    let target_x = i64::from(cursor.x) + i64::from(dx);
    let target_y = i64::from(cursor.y) + i64::from(dy);
    let target_x = i32::try_from(target_x).map_err(|_| {
        "absolute movement target is outside the supported integer range".to_owned()
    })?;
    let target_y = i32::try_from(target_y).map_err(|_| {
        "absolute movement target is outside the supported integer range".to_owned()
    })?;
    input.move_mouse_absolute(target_x, target_y).map_err(|error| {
        if error.message().is_empty() {
            input.absolute_pointer_unavailable_reason()
        } else {
            error.to_string()
        }
    })
}

fn absolute_point(
    input: Option<&Arc<dyn InputDevice>>,
    display: Option<&Arc<dyn DisplayCapture>>,
    x: f64,
    y: f64,
    mode: CoordinateMode,
    use_absolute_motion: bool,
) -> Result<(), String> {
    let Some(input) = input else {
        return Err("input backend is not available".to_owned());
    };
    let bounds = active_bounds(display)?;
    let (target_x, target_y) =
        resolve_absolute_point(bounds, x, y, mode).map_err(|error| error.to_string())?;
    if use_absolute_motion || input.prefers_direct_absolute_positioning() {
        return input.move_mouse_absolute(target_x, target_y).map_err(|error| {
            if error.message().is_empty() {
                input.absolute_pointer_unavailable_reason()
            } else {
                error.to_string()
            }
        });
    }

    let Some(cursor) = display.and_then(|display| display.cursor_position()) else {
        return Err(pointer_reason(display, Some(input)));
    };
    let delta_x = i64::from(target_x) - i64::from(cursor.x);
    let delta_y = i64::from(target_y) - i64::from(cursor.y);
    let delta_x = i32::try_from(delta_x)
        .map_err(|_| "relative movement delta is outside the supported integer range".to_owned())?;
    let delta_y = i32::try_from(delta_y)
        .map_err(|_| "relative movement delta is outside the supported integer range".to_owned())?;
    input.move_mouse(delta_x, delta_y);
    Ok(())
}

fn sample_pixel(
    display: Option<&Arc<dyn DisplayCapture>>,
    x: f64,
    y: f64,
    mode: CoordinateMode,
) -> Result<PixelColor, String> {
    let Some(display) = display else {
        return Err("input backend is not available".to_owned());
    };
    let bounds = active_bounds(Some(display))?;
    let (target_x, target_y) =
        resolve_absolute_point(bounds, x, y, mode).map_err(|error| error.to_string())?;
    display.sample_pixel(target_x, target_y).map_err(|error| {
        if !error.message().is_empty() {
            return error.to_string();
        }
        let unavailable = display.unavailable_reason();
        if unavailable.is_empty() {
            "screen pixel color sampling failed".to_owned()
        } else {
            unavailable
        }
    })
}

fn install_motion_api(
    lua: &Lua,
    context: &ScriptInputContext,
    stop: &LuaStopHandle,
    input: Option<&Arc<dyn InputDevice>>,
    display: Option<&Arc<dyn DisplayCapture>>,
) -> mlua::Result<()> {
    let move_context = context.clone();
    let move_input = input.cloned();
    let move_display = display.cloned();
    lua.globals().set(
        "moveMouse",
        lua.create_function(move |lua, (dx, dy): (Value, Value)| {
            check_settings_callback(&move_context, "moveMouse")?;
            check_cleanup_start(&move_context, "moveMouse")?;
            let dx = integer(lua, dx, "dx")?;
            let dy = integer(lua, dy, "dy")?;
            if move_context.mouse_motion_mode() == ScriptMouseMotionMode::Raw {
                if let Some(input) = &move_input {
                    input.move_mouse(dx, dy);
                }
                return Ok(());
            }
            absolute_delta(move_input.as_ref(), move_display.as_ref(), dx, dy)
                .map_err(|error| LuaError::RuntimeError(format!("moveMouse failed: {error}")))
        })?,
    )?;

    let abs_context = context.clone();
    let abs_stop = stop.clone();
    let abs_input = input.cloned();
    let abs_display = display.cloned();
    lua.globals().set(
        "moveMouseAbs",
        lua.create_function(move |lua, (x, y, mode): (Value, Value, Option<Value>)| {
            check_settings_callback(&abs_context, "moveMouseAbs")?;
            check_cleanup_start(&abs_context, "moveMouseAbs")?;
            let x = finite_number(lua, x, "x")?;
            let y = finite_number(lua, y, "y")?;
            let mode = coordinate_mode(mode)?;
            if let Some(display) = abs_display.as_ref() {
                display
                    .request_absolute_pointer_access_for_script(&|| {
                        abs_stop.reason() != ScriptStopReason::None
                    })
                    .map_err(|error| {
                        LuaError::RuntimeError(format!("moveMouseAbs failed: {error}"))
                    })?;
            }
            absolute_point(
                abs_input.as_ref(),
                abs_display.as_ref(),
                x,
                y,
                mode,
                abs_context.mouse_motion_mode() == ScriptMouseMotionMode::Absolute,
            )
            .map_err(|error| LuaError::RuntimeError(format!("moveMouseAbs failed: {error}")))
        })?,
    )?;

    let set_context = context.clone();
    lua.globals().set(
        "setMouseMotionMode",
        lua.create_function(move |_, value: Value| {
            check_settings_callback(&set_context, "setMouseMotionMode")?;
            let Value::String(text) = value else {
                return Err(LuaError::RuntimeError(
                    "mouse motion mode must be one of: raw, absolute".to_owned(),
                ));
            };
            let mode = match text.to_string_lossy().as_ref() {
                "raw" => ScriptMouseMotionMode::Raw,
                "absolute" | "abs" => ScriptMouseMotionMode::Absolute,
                _ => {
                    return Err(LuaError::RuntimeError(
                        "mouse motion mode must be one of: raw, absolute".to_owned(),
                    ));
                }
            };
            set_context.set_mouse_motion_mode(mode);
            Ok(mode.as_str())
        })?,
    )?;

    let get_context = context.clone();
    lua.globals().set(
        "getMouseMotionMode",
        lua.create_function(move |_, ()| Ok(get_context.mouse_motion_mode().as_str()))?,
    )
}

fn install_pixel_api(
    lua: &Lua,
    context: &ScriptInputContext,
    stop: &LuaStopHandle,
    display: Option<&Arc<dyn DisplayCapture>>,
) -> mlua::Result<()> {
    let color_context = context.clone();
    let color_stop = stop.clone();
    let color_display = display.cloned();
    lua.globals().set(
        "getPixelColor",
        lua.create_function(
            move |lua, (x, y, mode, format): (Value, Value, Option<Value>, Option<Value>)| {
                check_settings_callback(&color_context, "getPixelColor")?;
                let x = finite_number(lua, x, "x")?;
                let y = finite_number(lua, y, "y")?;
                let mode = coordinate_mode(mode)?;
                let format = pixel_output_format(format)?;
                if let Some(display) = color_display.as_ref() {
                    display
                        .request_pixel_access_for_script(&|| {
                            color_stop.reason() != ScriptStopReason::None
                        })
                        .map_err(|error| {
                            LuaError::RuntimeError(format!("getPixelColor failed: {error}"))
                        })?;
                }
                let color = sample_pixel(color_display.as_ref(), x, y, mode).map_err(|error| {
                    LuaError::RuntimeError(format!("getPixelColor failed: {error}"))
                })?;
                pixel_to_lua(lua, color, format)
            },
        )?,
    )?;

    let rect_context = context.clone();
    let rect_stop = stop.clone();
    let rect_display = display.cloned();
    lua.globals().set(
        "getPixelRect",
        lua.create_function(
            move |lua,
                  (x1, y1, x2, y2, mode, format): (
                Value,
                Value,
                Value,
                Value,
                Option<Value>,
                Option<Value>,
            )| {
                check_settings_callback(&rect_context, "getPixelRect")?;
                let x1 = finite_number(lua, x1, "x1")?;
                let y1 = finite_number(lua, y1, "y1")?;
                let x2 = finite_number(lua, x2, "x2")?;
                let y2 = finite_number(lua, y2, "y2")?;
                let mode = coordinate_mode(mode)?;
                let format = pixel_output_format(format)?;
                if let Some(display) = rect_display.as_ref() {
                    display
                        .request_pixel_access_for_script(&|| {
                            rect_stop.reason() != ScriptStopReason::None
                        })
                        .map_err(|error| {
                            LuaError::RuntimeError(format!("getPixelRect failed: {error}"))
                        })?;
                }
                let Some(display) = rect_display.as_ref() else {
                    return Err(LuaError::RuntimeError(
                        "getPixelRect failed: input backend is not available".to_owned(),
                    ));
                };
                let bounds = active_bounds(Some(display)).map_err(|error| {
                    LuaError::RuntimeError(format!("getPixelRect failed: {error}"))
                })?;
                let rect =
                    resolve_pixel_rect(bounds, (x1, y1), (x2, y2), mode).map_err(|error| {
                        LuaError::RuntimeError(format!("getPixelRect failed: {error}"))
                    })?;
                let pixels = sample_pixel_rect(display.as_ref(), rect).map_err(|error| {
                    let message = display_error_message(display.as_ref(), &error);
                    LuaError::RuntimeError(format!("getPixelRect failed: {message}"))
                })?;
                let outer = lua.create_table()?;
                for (row_index, row) in pixels.into_iter().enumerate() {
                    let inner = lua.create_table()?;
                    for (column_index, color) in row.into_iter().enumerate() {
                        inner.raw_set(column_index + 1, pixel_to_lua(lua, color, format)?)?;
                    }
                    outer.raw_set(row_index + 1, inner)?;
                }
                Ok(outer)
            },
        )?,
    )
}

fn display_error_message(display: &dyn DisplayCapture, error: &DisplayError) -> String {
    if !error.message().is_empty() {
        return error.to_string();
    }
    let unavailable = display.unavailable_reason();
    if unavailable.is_empty() {
        "screen pixel color sampling failed".to_owned()
    } else {
        unavailable
    }
}

fn install_macos_cursor_api(lua: &Lua) -> mlua::Result<()> {
    lua.globals().set(
        "setMacOSCursorMovement",
        lua.create_function(|_, value: Value| {
            let enabled = !matches!(value, Value::Nil | Value::Boolean(false));
            #[cfg(target_os = "macos")]
            {
                MACOS_CURSOR_MOVEMENT.store(enabled, Ordering::Release);
                Ok(true)
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = enabled;
                Ok(false)
            }
        })?,
    )?;
    lua.globals().set(
        "getMacOSCursorMovement",
        lua.create_function(|_, ()| {
            #[cfg(target_os = "macos")]
            {
                Ok(MACOS_CURSOR_MOVEMENT.load(Ordering::Acquire))
            }
            #[cfg(not(target_os = "macos"))]
            {
                Ok(false)
            }
        })?,
    )
}

/// Register `moveDegrees` with the saved sensitivity/camfix snapshot captured at registration.
///
/// # Errors
///
/// Returns a Lua registration error if the function cannot be installed.
pub fn install_move_degrees_api(
    vm: &LuaScriptVm,
    context: ScriptInputContext,
    input: Option<Arc<dyn InputDevice>>,
    display: Option<Arc<dyn DisplayCapture>>,
    settings: Option<&dyn SavedSettingSource>,
) -> Result<(), ScriptDisplayApiError> {
    let calibration = cache_move_degrees_calibration(settings);
    let resources = (context, input, display, calibration);
    let execution_context = resources.0.clone();
    let input = resources.1.clone();
    let display = resources.2.clone();
    let calibration = resources.3.clone();

    vm.lua.globals().set(
        "moveDegrees",
        vm.lua.create_function(move |lua, (dx, dy): (Value, Value)| {
            check_settings_callback(&execution_context, "moveDegrees")?;
            check_cleanup_start(&execution_context, "moveDegrees")?;
            let horizontal_degrees = finite_number(lua, dx, "dx")?;
            let vertical_degrees = finite_number(lua, dy, "dy")?;
            let Some(pixels_per_degree) = calibration.pixels_per_degree else {
                return Err(LuaError::RuntimeError(calibration.error.clone()));
            };
            let horizontal_pixels =
                round_mouse_delta(horizontal_degrees * pixels_per_degree, "dx")?;
            let vertical_pixels = round_mouse_delta(-vertical_degrees * pixels_per_degree, "dy")?;

            if execution_context.mouse_motion_mode() == ScriptMouseMotionMode::Raw {
                if let Some(input) = input.as_ref() {
                    input.move_mouse(horizontal_pixels, vertical_pixels);
                }
                return Ok(());
            }
            absolute_delta(input.as_ref(), display.as_ref(), horizontal_pixels, vertical_pixels)
                .map_err(|error| LuaError::RuntimeError(format!("moveDegrees failed: {error}")))
        })?,
    )?;
    Ok(())
}

/// Register pointer movement and screen-pixel APIs against separate input/display services.
///
/// # Errors
///
/// Returns a Lua registration error if a function/table cannot be installed.
pub fn install_display_api(
    vm: &LuaScriptVm,
    context: ScriptInputContext,
    input: Option<Arc<dyn InputDevice>>,
    display: Option<Arc<dyn DisplayCapture>>,
) -> Result<(), ScriptDisplayApiError> {
    let resources = (context, input, display);
    let context = &resources.0;
    let input = resources.1.as_ref();
    let display = resources.2.as_ref();
    let stop = vm.stop_handle();
    install_motion_api(&vm.lua, context, &stop, input, display)?;
    install_pixel_api(&vm.lua, context, &stop, display)?;
    install_macos_cursor_api(&vm.lua)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::MAX_SCRIPT_RUNTIME;
    use serde::Deserialize;
    use smu_display::{CursorPosition, ScreenBounds};
    use smu_input::{InputError, InputInjector, KeyState};
    use smu_profile::{SavedSettingSource, SavedSettingValue};
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[derive(Debug, Deserialize)]
    struct MoveDegreesFixture {
        cases: Vec<MoveDegreesCase>,
    }

    #[derive(Debug, Deserialize)]
    struct MoveDegreesCase {
        name: String,
        input: serde_json::Value,
        observed: MoveDegreesObserved,
    }

    #[derive(Debug, Deserialize)]
    struct MoveDegreesObserved {
        valid: bool,
        pixels_per_degree: f64,
        error: String,
        dx_pixels: Option<i32>,
        dy_pixels: Option<i32>,
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum PointerEvent {
        Relative(i32, i32),
        Absolute(i32, i32),
    }

    #[derive(Debug, Default)]
    struct FakeInput {
        events: Mutex<Vec<PointerEvent>>,
        direct: AtomicBool,
    }

    impl FakeInput {
        fn events(&self) -> Vec<PointerEvent> {
            self.events.lock().expect("event lock").clone()
        }
    }

    impl KeyState for FakeInput {
        fn is_pressed(&self, _key: smu_core::KeyCode) -> bool {
            false
        }
    }

    impl InputInjector for FakeInput {
        fn hold_key(&self, _key: smu_core::KeyCode, _extended: bool) {}
        fn release_key(&self, _key: smu_core::KeyCode, _extended: bool) {}

        fn move_mouse(&self, dx: i32, dy: i32) {
            self.events.lock().expect("event lock").push(PointerEvent::Relative(dx, dy));
        }

        fn move_mouse_absolute(&self, x: i32, y: i32) -> Result<(), InputError> {
            self.events.lock().expect("event lock").push(PointerEvent::Absolute(x, y));
            Ok(())
        }

        fn prefers_direct_absolute_positioning(&self) -> bool {
            self.direct.load(Ordering::Acquire)
        }

        fn mouse_wheel(&self, _delta: i32) {}
    }

    #[derive(Debug)]
    struct FakeDisplay {
        bounds: Option<ScreenBounds>,
        cursor: Option<CursorPosition>,
        pixel_requests: AtomicUsize,
        absolute_requests: AtomicUsize,
    }

    impl DisplayCapture for FakeDisplay {
        fn active_monitor_bounds(&self) -> Option<ScreenBounds> {
            self.bounds
        }

        fn cursor_position(&self) -> Option<CursorPosition> {
            self.cursor
        }

        fn request_pixel_access_for_script(
            &self,
            _cancelled: &(dyn Fn() -> bool + Send + Sync),
        ) -> Result<(), DisplayError> {
            self.pixel_requests.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }

        fn request_absolute_pointer_access_for_script(
            &self,
            _cancelled: &(dyn Fn() -> bool + Send + Sync),
        ) -> Result<(), DisplayError> {
            self.absolute_requests.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }

        fn sample_pixel(&self, x: i32, y: i32) -> Result<PixelColor, DisplayError> {
            Ok(PixelColor {
                r: u8::try_from(x.rem_euclid(256)).expect("normalized test x should fit"),
                g: u8::try_from(y.rem_euclid(256)).expect("normalized test y should fit"),
                b: 0xA5,
            })
        }
    }

    #[derive(Debug, Default)]
    struct FakeSettings(Mutex<HashMap<String, SavedSettingValue>>);

    impl FakeSettings {
        fn with_calibration(camfix: bool, sensitivity: &str) -> Self {
            Self(Mutex::new(HashMap::from([
                ("camfixtoggle".to_owned(), SavedSettingValue::Bool(camfix)),
                ("RobloxSensValue".to_owned(), SavedSettingValue::String(sensitivity.to_owned())),
            ])))
        }

        fn set_sensitivity(&self, sensitivity: &str) {
            self.0.lock().expect("settings lock").insert(
                "RobloxSensValue".to_owned(),
                SavedSettingValue::String(sensitivity.to_owned()),
            );
        }
    }

    impl SavedSettingSource for FakeSettings {
        fn get_saved_setting(&self, name: &str) -> Option<SavedSettingValue> {
            self.0.lock().expect("settings lock").get(name).cloned()
        }
    }

    fn vm_with_display() -> (LuaScriptVm, ScriptInputContext, Arc<FakeInput>, Arc<FakeDisplay>) {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let context = ScriptInputContext::default();
        let input = Arc::new(FakeInput::default());
        let display = Arc::new(FakeDisplay {
            bounds: Some(ScreenBounds { x: -1920, y: 0, width: 1920, height: 1080 }),
            cursor: Some(CursorPosition { x: -1000, y: 500 }),
            pixel_requests: AtomicUsize::new(0),
            absolute_requests: AtomicUsize::new(0),
        });
        install_display_api(&vm, context.clone(), Some(input.clone()), Some(display.clone()))
            .expect("display API should install");
        (vm, context, input, display)
    }

    #[test]
    fn raw_and_absolute_delta_modes_use_different_input_paths() {
        let (vm, _, input, _) = vm_with_display();
        vm.execute_source("moveMouse(10, -20)", "raw.lua").expect("raw move should run");
        assert_eq!(input.events(), [PointerEvent::Relative(10, -20)]);

        vm.execute_source("setMouseMotionMode('abs'); moveMouse(10, -20)", "absolute.lua")
            .expect("absolute delta should run");
        assert_eq!(
            input.events(),
            [PointerEvent::Relative(10, -20), PointerEvent::Absolute(-990, 480),]
        );
    }

    #[test]
    fn script_pixel_and_absolute_mouse_calls_request_platform_permission_first() {
        let (vm, _, input, display) = vm_with_display();
        input.direct.store(true, Ordering::Release);
        vm.execute_source(
            "getPixelColor(0, 0); getPixelRect(0, 0, 1, 1); moveMouseAbs(0, 0)",
            "permission-hooks.lua",
        )
        .expect("permission-aware display calls should run");
        assert_eq!(display.pixel_requests.load(Ordering::Acquire), 2);
        assert_eq!(display.absolute_requests.load(Ordering::Acquire), 1);
    }

    #[test]
    fn move_mouse_abs_uses_monitor_coordinate_modes() {
        let (vm, _, input, _) = vm_with_display();
        vm.execute_source(
            "setMouseMotionMode('absolute'); moveMouseAbs(100, 100, 'percent')",
            "abs-point.lua",
        )
        .expect("absolute point should run");
        assert_eq!(input.events(), [PointerEvent::Absolute(-1, 1079)]);
    }

    #[test]
    fn direct_backend_preference_overrides_raw_absolute_point_path() {
        let (vm, _, input, _) = vm_with_display();
        input.direct.store(true, Ordering::Release);
        vm.execute_source("moveMouseAbs(0, 0)", "direct.lua")
            .expect("direct absolute backend should run");
        assert_eq!(input.events(), [PointerEvent::Absolute(-1920, 0)]);
    }

    #[test]
    fn pixel_color_supports_hex_and_rgb_formats() {
        let (vm, _, _, _) = vm_with_display();
        assert_eq!(
            vm.eval::<String>("return getPixelColor(10, 20, 'pixels', 'hex')", MAX_SCRIPT_RUNTIME,)
                .expect("hex pixel should return"),
            "#8A14A5"
        );
        let (r, g, b) = vm
            .eval::<(u8, u8, u8)>(
                "local c = getPixelColor(10, 20, 'pixels', 'rgb'); return c.r, c.g, c.b",
                MAX_SCRIPT_RUNTIME,
            )
            .expect("RGB pixel should return");
        assert_eq!((r, g, b), (138, 20, 165));
    }

    #[test]
    fn pixel_rect_is_row_major_and_one_indexed() {
        let (vm, _, _, _) = vm_with_display();
        let values = vm
            .eval::<(String, String, String, String)>(
                concat!(
                    "local p = getPixelRect(0, 0, 1, 1, 'pixels'); ",
                    "return p[1][1], p[1][2], p[2][1], p[2][2]"
                ),
                MAX_SCRIPT_RUNTIME,
            )
            .expect("pixel rectangle should return");
        assert_eq!(
            values,
            (
                "#8000A5".to_owned(),
                "#8100A5".to_owned(),
                "#8001A5".to_owned(),
                "#8101A5".to_owned(),
            )
        );
    }

    #[test]
    fn pixel_and_motion_errors_keep_cpp_function_prefixes() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let context = ScriptInputContext::default();
        install_display_api(&vm, context, None, None).expect("API should install without services");
        let pixel = vm
            .execute_source("getPixelColor(0, 0)", "missing-pixel.lua")
            .expect_err("missing display should fail");
        assert!(pixel.message.contains("getPixelColor failed: input backend is not available"));
        let motion = vm
            .execute_source("setMouseMotionMode('absolute'); moveMouse(1, 1)", "missing-input.lua")
            .expect_err("absolute move without input should fail");
        assert!(motion.message.contains("moveMouse failed: input backend is not available"));
    }

    #[test]
    fn settings_and_cleanup_restrictions_match_cpp() {
        let (vm, context, input, _) = vm_with_display();
        context.set_mode(ScriptExecutionMode::SettingsCallback);
        assert!(
            vm.execute_source("getPixelColor(0, 0)", "settings.lua")
                .expect_err("settings callback should reject pixel read")
                .message
                .contains("getPixelColor is not available inside script settings")
        );
        context.set_mode(ScriptExecutionMode::Cleanup);
        assert!(
            vm.execute_source("moveMouse(1, 2)", "cleanup.lua")
                .expect_err("cleanup should reject movement")
                .message
                .contains("moveMouse is not available inside onCleanup")
        );
        assert!(input.events().is_empty());
    }

    #[test]
    fn motion_mode_parser_preserves_abs_alias() {
        let (vm, _, _, _) = vm_with_display();
        assert_eq!(
            vm.eval::<String>(
                "setMouseMotionMode('abs'); return getMouseMotionMode()",
                MAX_SCRIPT_RUNTIME,
            )
            .expect("abs alias should be accepted"),
            "absolute"
        );
        assert!(
            vm.execute_source("setMouseMotionMode('pointer')", "bad-mode.lua")
                .expect_err("unknown mode should fail")
                .message
                .contains("mouse motion mode must be one of: raw, absolute")
        );
    }

    #[test]
    fn move_degrees_cases_match_cpp_oracle_fixture() {
        let fixture: MoveDegreesFixture = serde_json::from_str(include_str!(
            "../../../../compat/scripting/move_degrees_cases.json"
        ))
        .expect("C++ moveDegrees fixture should parse");

        for case in fixture.cases {
            let object =
                case.input.as_object().expect("moveDegrees fixture input should be an object");
            let mut values = HashMap::new();
            if let Some(value) = object.get("camfix") {
                let saved = match value {
                    serde_json::Value::Bool(value) => SavedSettingValue::Bool(*value),
                    serde_json::Value::Number(number) => {
                        SavedSettingValue::Float(number.as_f64().unwrap_or_default())
                    }
                    serde_json::Value::String(value) => SavedSettingValue::String(value.clone()),
                    _ => SavedSettingValue::String(String::new()),
                };
                values.insert("camfixtoggle".to_owned(), saved);
            }
            if let Some(value) = object.get("sensitivity") {
                let saved = match value {
                    serde_json::Value::String(value) => SavedSettingValue::String(value.clone()),
                    serde_json::Value::Number(number) => {
                        SavedSettingValue::Float(number.as_f64().unwrap_or_default())
                    }
                    serde_json::Value::Bool(value) => SavedSettingValue::Bool(*value),
                    _ => SavedSettingValue::String(String::new()),
                };
                values.insert("RobloxSensValue".to_owned(), saved);
            }
            let settings = FakeSettings(Mutex::new(values));
            let calibration = cache_move_degrees_calibration(Some(&settings));
            assert_eq!(
                calibration.pixels_per_degree().unwrap_or_default().to_bits(),
                case.observed.pixels_per_degree.to_bits(),
                "case {} calibration",
                case.name
            );

            if !calibration.is_valid() {
                assert!(!case.observed.valid, "case {} should fail", case.name);
                assert_eq!(calibration.error(), case.observed.error, "case {} error", case.name);
                continue;
            }

            let pixels_per_degree = calibration
                .pixels_per_degree()
                .expect("valid calibration should contain pixels per degree");
            let dx = object.get("dx").and_then(serde_json::Value::as_f64).unwrap_or_default();
            let dy = object.get("dy").and_then(serde_json::Value::as_f64).unwrap_or_default();
            let horizontal = round_mouse_delta(dx * pixels_per_degree, "dx");
            let vertical = horizontal
                .as_ref()
                .map_or_else(|_| Ok(0), |_| round_mouse_delta(-dy * pixels_per_degree, "dy"));
            match (horizontal, vertical) {
                (Ok(horizontal), Ok(vertical)) => {
                    assert!(case.observed.valid, "case {} should succeed", case.name);
                    assert_eq!(case.observed.dx_pixels, Some(horizontal), "case {} dx", case.name);
                    assert_eq!(case.observed.dy_pixels, Some(vertical), "case {} dy", case.name);
                    assert!(case.observed.error.is_empty(), "case {} error", case.name);
                }
                (Err(error), _) | (_, Err(error)) => {
                    assert!(!case.observed.valid, "case {} should fail", case.name);
                    let error = match error {
                        LuaError::RuntimeError(message) => message,
                        other => other.to_string(),
                    };
                    assert_eq!(error, case.observed.error, "case {} error", case.name);
                }
            }
        }
    }

    #[test]
    fn move_degrees_uses_cached_cpp_calibration_and_y_inversion() {
        let (vm, context, input, display) = vm_with_display();
        let settings = FakeSettings::with_calibration(false, "0.5");
        install_move_degrees_api(&vm, context, Some(input.clone()), Some(display), Some(&settings))
            .expect("moveDegrees should install");
        vm.execute_source("moveDegrees(10, 5)", "degrees.lua").expect("moveDegrees should execute");
        assert_eq!(input.events(), [PointerEvent::Relative(40, -20)]);
    }

    #[test]
    fn move_degrees_camfix_changes_pixels_per_degree() {
        let calibration =
            cache_move_degrees_calibration(Some(&FakeSettings::with_calibration(true, "0.5")));
        assert!(calibration.is_valid());
        assert_eq!(calibration.pixels_per_degree(), Some(1000.0 / 180.0));
        assert!(calibration.error().is_empty());
    }

    #[test]
    fn move_degrees_accepts_cpp_hex_float_sensitivity() {
        let calibration = cache_move_degrees_calibration(Some(&FakeSettings::with_calibration(
            false,
            "  +0x1p-1  ",
        )));
        assert_eq!(calibration.pixels_per_degree(), Some(4.0));
    }

    #[test]
    fn move_degrees_missing_and_wrong_type_settings_keep_cpp_errors() {
        let missing = cache_move_degrees_calibration(None);
        assert_eq!(missing.error(), "moveDegrees requires the saved camfixtoggle setting.");

        let wrong = FakeSettings(Mutex::new(HashMap::from([
            ("camfixtoggle".to_owned(), SavedSettingValue::Integer(1)),
            ("RobloxSensValue".to_owned(), SavedSettingValue::String("0.5".to_owned())),
        ])));
        assert_eq!(
            cache_move_degrees_calibration(Some(&wrong)).error(),
            "moveDegrees requires the saved camfixtoggle setting."
        );

        let invalid = FakeSettings::with_calibration(false, "nan");
        assert_eq!(
            cache_move_degrees_calibration(Some(&invalid)).error(),
            "moveDegrees requires RobloxSensValue to be a finite number greater than 0."
        );
    }

    #[test]
    fn move_degrees_calibration_is_snapshotted_at_registration() {
        let (vm, context, input, display) = vm_with_display();
        let settings = FakeSettings::with_calibration(false, "0.5");
        install_move_degrees_api(&vm, context, Some(input.clone()), Some(display), Some(&settings))
            .expect("moveDegrees should install");
        settings.set_sensitivity("1.0");
        vm.execute_source("moveDegrees(1, 0)", "cached.lua")
            .expect("cached calibration should execute");
        assert_eq!(input.events(), [PointerEvent::Relative(4, 0)]);
    }

    #[test]
    fn move_degrees_absolute_mode_uses_cursor_delta_path() {
        let (vm, context, input, display) = vm_with_display();
        let settings = FakeSettings::with_calibration(false, "0.5");
        install_move_degrees_api(
            &vm,
            context.clone(),
            Some(input.clone()),
            Some(display),
            Some(&settings),
        )
        .expect("moveDegrees should install");
        context.set_mouse_motion_mode(ScriptMouseMotionMode::Absolute);
        vm.execute_source("moveDegrees(10, 5)", "absolute-degrees.lua")
            .expect("absolute moveDegrees should execute");
        assert_eq!(input.events(), [PointerEvent::Absolute(-960, 480)]);
    }

    #[test]
    fn move_degrees_rounding_errors_match_cpp_messages() {
        let (vm, context, input, display) = vm_with_display();
        let settings = FakeSettings::with_calibration(false, "0.5");
        install_move_degrees_api(&vm, context, Some(input), Some(display), Some(&settings))
            .expect("moveDegrees should install");
        assert!(
            vm.execute_source("moveDegrees(1e308, 0)", "overflow.lua")
                .expect_err("overflowing conversion should fail")
                .message
                .contains("dx conversion overflowed")
        );
        assert!(
            vm.execute_source("moveDegrees(1e10, 0)", "range.lua")
                .expect_err("out-of-range conversion should fail")
                .message
                .contains("dx conversion is outside the allowed range")
        );
    }

    #[test]
    fn settings_render_mode_blocks_display_actions_like_cpp_helper() {
        let (vm, context, input, _) = vm_with_display();
        context.set_mode(ScriptExecutionMode::SettingsRender);
        let error = vm
            .execute_source("moveMouse(1, 2)", "settings-render.lua")
            .expect_err("settings render should reject movement");
        assert!(error.message.contains("moveMouse is not available inside script settings"));
        assert!(input.events().is_empty());
    }

    #[test]
    fn macos_cursor_toggle_is_platform_gated() {
        let (vm, _, _, _) = vm_with_display();
        let enabled = vm
            .eval::<bool>("return setMacOSCursorMovement(true)", MAX_SCRIPT_RUNTIME)
            .expect("macOS toggle API should return");
        if cfg!(target_os = "macos") {
            assert!(enabled);
            assert!(
                vm.eval::<bool>("return getMacOSCursorMovement()", MAX_SCRIPT_RUNTIME)
                    .expect("macOS toggle read should return")
            );
        } else {
            assert!(!enabled);
            assert!(
                !vm.eval::<bool>("return getMacOSCursorMovement()", MAX_SCRIPT_RUNTIME)
                    .expect("non-macOS toggle read should return false")
            );
        }
    }
}

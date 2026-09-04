//! Pointer/display Lua API adapters.

use crate::input_api::{ScriptExecutionMode, ScriptInputContext, ScriptMouseMotionMode};
use crate::lua_runtime::LuaScriptVm;
use mlua::{Error as LuaError, Lua, Value};
use smu_display::{
    CoordinateMode, DisplayCapture, DisplayError, PixelColor, resolve_absolute_point,
    resolve_pixel_rect, sample_pixel_rect,
};
use smu_input::InputDevice;
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
    if context.mode() == ScriptExecutionMode::SettingsCallback {
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
    display: Option<&Arc<dyn DisplayCapture>>,
) -> mlua::Result<()> {
    let color_context = context.clone();
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
                let color = sample_pixel(color_display.as_ref(), x, y, mode).map_err(|error| {
                    LuaError::RuntimeError(format!("getPixelColor failed: {error}"))
                })?;
                pixel_to_lua(lua, color, format)
            },
        )?,
    )?;

    let rect_context = context.clone();
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
    install_motion_api(&vm.lua, context, input, display)?;
    install_pixel_api(&vm.lua, context, display)?;
    install_macos_cursor_api(&vm.lua)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::MAX_SCRIPT_RUNTIME;
    use smu_display::{CursorPosition, ScreenBounds};
    use smu_input::{InputError, InputInjector, KeyState};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

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
    }

    impl DisplayCapture for FakeDisplay {
        fn active_monitor_bounds(&self) -> Option<ScreenBounds> {
            self.bounds
        }

        fn cursor_position(&self) -> Option<CursorPosition> {
            self.cursor
        }

        fn sample_pixel(&self, x: i32, y: i32) -> Result<PixelColor, DisplayError> {
            Ok(PixelColor {
                r: u8::try_from(x.rem_euclid(256)).expect("normalized test x should fit"),
                g: u8::try_from(y.rem_euclid(256)).expect("normalized test y should fit"),
                b: 0xA5,
            })
        }
    }

    fn vm_with_display() -> (LuaScriptVm, ScriptInputContext, Arc<FakeInput>, Arc<FakeDisplay>) {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let context = ScriptInputContext::default();
        let input = Arc::new(FakeInput::default());
        let display = Arc::new(FakeDisplay {
            bounds: Some(ScreenBounds { x: -1920, y: 0, width: 1920, height: 1080 }),
            cursor: Some(CursorPosition { x: -1000, y: 500 }),
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

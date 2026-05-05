# Lua Macro Scripting

Custom macros can be written as Lua scripts and imported into Spencer Macro Client. The scripting system supports `.smus`, `.hss`, and `.lua` files. Scripts run inside the macro runtime and can automate keyboard input, mouse movement, text entry, timing, freeze behavior, and lag-switch controls.

Imported scripts can also define their own custom ImGui settings by implementing `onSettings()`. The app runs that callback once in a non-rendering initialization pass when the script loads, then again in the selected-script panel to render the controls.
`onSettings()` should only call `ui.*` helpers. Input, mouse, process, sleep, and lag-switch APIs are blocked while settings are being rendered.
`onSettings()` has a 5-second hard timeout.
`onSettings()` is optional. If it is not defined, only the built-in script controls appear.

## Safety

Only import scripts from sources you trust. Lua scripts can simulate input and control process-related macro behavior.

Each script runs with a per-script Lua memory cap. The default is 64 MiB. Scripts can request a different cap with `-- @memoryLimitMB:`, clamped between 16 MiB and 256 MiB.
`onExecute()` and script load calls are timed, `onSettings()` is timed separately, and user-created coroutines are supported through wrapped `coroutine.resume()` and `coroutine.wrap()`.
The sandbox opens the base, table, string, math, utf8, and coroutine libraries. It does not open `os`, `io`, `package`, or `debug`, and removes `dofile`, `loadfile`, `load`, and `collectgarbage`.

## Script Structure

Scripts are plain-text Lua files. Every script must define an `onExecute()` function, which is called when the macro is executed.

```lua
-- @name: My Custom Macro
-- @desc: A simple example script
-- @author: YourName
-- @version: 1.0
-- @keybind: F5

function onExecute()
    log("Script executed!")
    pressKey("Space")
    sleep(100)
end
```

## Metadata

Add metadata at the top of the script using comment tags.

| Tag | Description |
| --- | --- |
| `-- @name:` | Display name shown in the macro list |
| `-- @desc:` or `-- @description:` | Description shown in the details panel |
| `-- @author:` | Script author |
| `-- @version:` | Script version |
| `-- @keybind:` | Default keybind, such as `F5` or `LCtrl+K` |
| `-- @memoryLimitMB:` | Optional Lua memory cap in MiB, clamped from 16 to 256 |

## Importing Scripts

You can import scripts with the in-app import button or place script files in the `scripts/` folder next to the executable. Scripts in that folder are loaded on startup.

## Lua API Reference

### Utility

| Function | Description |
| --- | --- |
| `log(message)` | Write a message to the log console |
| `sleep(ms)` | Pause the script for the specified number of milliseconds, clamped to 300000 ms per call |
| `nowMicros()` | Return the current monotonic time in microseconds |
| `getSMUVersion()` | Return the current application version |
| `getPlatform()` | Return `windows`, `linux`, or `unknown` |
| `getSavedValue(name)` | Return the current in-memory value of a setting persisted in the save file. Returns `nil` if the name is not exposed |

### Script Settings

The `ui` table is available inside scripts that define `onSettings()`. These functions update a persistent per-script `settings` table and render the controls in the script details panel.

Most controls accept optional size arguments at the end of the parameter list. Widths and heights are specified in pixels; controls that do not use a dimension simply ignore it.

| Function | Description |
| --- | --- |
| `ui.text(text, width)` | Render static wrapped text |
| `ui.separator(spacing)` | Render a separator line, then add optional vertical spacing |
| `ui.checkbox(id, label, defaultValue, width)` | Render a checkbox and persist a boolean setting |
| `ui.sliderInt(id, label, defaultValue, minValue, maxValue, width)` | Render an integer slider and persist the value |
| `ui.sliderFloat(id, label, defaultValue, minValue, maxValue, width)` | Render a float slider and persist the value |
| `ui.textbox(id, label, defaultValue, width, height)` | Render a text box and persist the value |
| `ui.dynamicTextbox(id, label, defaultValue, width, height)` | Render an editable multi-line text box backed by a script-updated value (not persisted) |
| `ui.setDynamicText(id, text)` | Update a dynamic text box value (clamped to 4096 characters, not persisted) |
| `ui.keybind(id, label, defaultValue, width)` | Render the standard SMU keybind picker (with hex display) and persist the combo as a hotkey value |

The current values are mirrored into a global `settings` table, so `onExecute()` can read them directly.
Script settings are stored in the save file under imported script data, separate from the main app settings.
Dynamic text boxes are session-only and are not written to the save file.
UI IDs must be non-empty, must not contain embedded NUL bytes, and are limited to 128 bytes. A single `onSettings()` call may create up to 512 UI controls. Stored script UI strings are clamped to 4096 bytes.

### Input

| Function | Description |
| --- | --- |
| `pressKey(key, delay)` | Press and release a key. `delay` defaults to 50 ms |
| `holdKey(key)` | Hold a key down |
| `releaseKey(key)` | Release a held key |
| `isKeyPressed(key)` | Return whether a key is currently pressed |
| `isHotkeyPressed(hotkey)` | Return whether a hotkey combo is currently pressed (use values from `ui.keybind`) |
| `typeText(text, delay)` | Type text with an optional per-character delay. `delay` defaults to 30 ms |
| `moveMouse(dx, dy)` | Move the mouse relative to its current position |
| `mouseWheel(delta)` | Scroll the mouse wheel |

### Process Control

| Function | Description |
| --- | --- |
| `freeze(enable)` | Freeze or unfreeze the target process |
| `robloxFreeze(enable)` | Alias for `freeze(enable)` |
| `roblox_freeze(enable)` | Alias for `freeze(enable)` |
| `lagSwitch(enable)` | Toggle lag-switch behavior |
| `lagswitch(enable)` | Alias for `lagSwitch(enable)` |

## Reading Saved Settings

Use `getSavedValue(name)` to read settings that are currently loaded in memory and persisted in the save file. The name must match the saved key exactly.

Examples:

```lua
local camfixEnabled = getSavedValue("camfixtoggle")
local camfixMode = getSavedValue("wallhopcamfix")
local jumpKey = getSavedValue("vk_f5")
local wallhopPixels = getSavedValue("WallhopPixels")
```

If a key is not part of the persisted setting registry, `getSavedValue()` returns `nil`.
Save-file settings are read-only from scripts. Scripts cannot modify the main app settings.

## Example Script

```lua
-- @name: Camfix Checker
-- @desc: Reads a save-file-backed setting and reports its current value
-- @version: 1.0

function onSettings()
    ui.text("Camera Fix - Logs camfix status and hotkey activity", 360)
    ui.separator(6)
    camfixEnabled = ui.checkbox("camfix_enabled", "Enable camfix", true, 260)
    camfixMode = ui.sliderInt("camfix_mode", "Camfix mode", 1, 0, 3, 240)
    camfixKey = ui.keybind("camfix_key", "Override keybind", "F5", 260)
    camfixLabel = ui.textbox("camfix_label", "Label", "Camfix", 260)
    ui.dynamicTextbox("camfix_status", "Status", "Idle", 500, 200)
end

function onExecute()
    local camfix = getSavedValue("camfixtoggle")
    if settings.camfix_enabled then
        local logLine = "camfixtoggle = " .. tostring(camfix)
        log(logLine)
        logBuffer = (logBuffer or "") .. logLine .. "\n"
        logBuffer = string.sub(logBuffer, -4000)
        ui.setDynamicText("camfix_status", logBuffer)

        if isHotkeyPressed(settings.camfix_key) then
            local pressedLine = "Hotkey pressed: " .. tostring(settings.camfix_key)
            log(pressedLine)
            logBuffer = (logBuffer or "") .. pressedLine .. "\n"
            logBuffer = string.sub(logBuffer, -4000)
            ui.setDynamicText("camfix_status", logBuffer)
        end
    end
end
```

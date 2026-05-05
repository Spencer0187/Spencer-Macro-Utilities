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

### Allowed `getSavedValue()` Variables

`getSavedValue(name)` can read only the saved settings registered by the app. It returns a Lua boolean, number, string, or `nil` if the name is not in this allowlist.

Scripts also receive the global `settings` table for per-script UI state created with `ui.*` helpers. The names below are for app/profile settings read through `getSavedValue()`.

#### Boolean values

| Name | Type | Description |
| --- | --- | --- |
| `macrotoggled` | boolean | Master macro enable toggle. |
| `shiftswitch` | boolean | Shift-related macro toggle. |
| `wallhopswitch` | boolean | Whether wallhop uses the left-flick variant. |
| `wallhopcamfix` | boolean | Camera-fix mode used by wallhop calculations. |
| `unequiptoggle` | boolean | Whether the unequip macro is enabled. |
| `isspeedswitch` | boolean | Whether the speed macro is enabled. |
| `isfreezeswitch` | boolean | Whether the freeze macro is enabled. |
| `iswallwalkswitch` | boolean | Whether the wallwalk macro is enabled. |
| `isspamswitch` | boolean | Whether spam-key behavior is enabled. |
| `isitemclipswitch` | boolean | Whether item-clip behavior is enabled. |
| `autotoggle` | boolean | Whether the automatic macro mode is enabled. |
| `toggle_jump` | boolean | Whether wallhop jumps during execution. |
| `toggle_flick` | boolean | Whether wallhop flicks back during execution. |
| `camfixtoggle` | boolean | Whether camera-fix behavior is enabled. |
| `wallwalktoggleside` | boolean | Selected wallwalk side toggle. |
| `antiafktoggle` | boolean | Whether anti-AFK behavior is enabled. |
| `fasthhj` | boolean | Whether the fast HHJ mode is enabled. |
| `globalzoomin` | boolean | Whether global zoom-in behavior is enabled. |
| `globalzoominreverse` | boolean | Whether global zoom-in direction is reversed. |
| `wallesslhjswitch` | boolean | Whether wall-less LHJ behavior is enabled. |
| `chatoverride` | boolean | Whether chat override behavior is enabled. |
| `bounceautohold` | boolean | Whether bounce auto-hold is enabled. |
| `bouncerealignsideways` | boolean | Whether bounce realigns sideways. |
| `bouncesidetoggle` | boolean | Selected bounce side toggle. |
| `laughmoveswitch` | boolean | Whether laugh-move behavior is enabled. |
| `freezeoutsideroblox` | boolean | Whether freeze is allowed outside Roblox. |
| `takeallprocessids` | boolean | Whether process control targets all matching processes instead of the main process. |
| `ontoptoggle` | boolean | Whether the main window is set always-on-top. |
| `bunnyhopsmart` | boolean | Whether smart bunny-hop behavior is enabled. |
| `presskeyinroblox` | boolean | Whether press-key is restricted to Roblox focus. |
| `unequipinroblox` | boolean | Whether unequip is restricted to Roblox focus. |
| `doublepressafkkey` | boolean | Whether the AFK key is double-pressed. |
| `useoldpaste` | boolean | Whether legacy paste behavior is used. |
| `floorbouncehhj` | boolean | Whether floor-bounce HHJ behavior is enabled. |
| `HHJFreezeDelayApply` | boolean | Whether HHJ freeze-delay override is applied. |
| `islagswitchswitch` | boolean | Whether lag-switch behavior is enabled. |
| `prevent_disconnect` | boolean | Whether disconnect prevention is enabled for lag-switch behavior. |
| `lagswitchoutbound` | boolean | Whether outbound packets are affected by the lag switch. |
| `lagswitchinbound` | boolean | Whether inbound packets are affected by the lag switch. |
| `lagswitchtargetroblox` | boolean | Whether the lag switch targets Roblox traffic only. |
| `lagswitchlaginbound` | boolean | Whether inbound traffic is delayed instead of blocked. |
| `lagswitchlagoutbound` | boolean | Whether outbound traffic is delayed instead of blocked. |
| `lagswitchlag` | boolean | Whether lag-switch delay mode is enabled. |
| `lagswitchusetcp` | boolean | Whether lag-switch filtering includes TCP traffic. |
| `lagswitch_autounblock` | boolean | Whether lag switch automatically unblocks after a duration. |
| `show_lag_overlay` | boolean | Whether the lag-switch overlay is shown. |
| `overlay_hide_inactive` | boolean | Whether the lag overlay hides while inactive. |
| `overlay_use_bg` | boolean | Whether the lag overlay draws a background. |

#### Numeric values

| Name | Type | Description |
| --- | --- | --- |
| `selected_section` | number | Currently selected main UI section index. |
| `vk_f5` | number | Virtual-key/hotkey value for f5. |
| `vk_f6` | number | Virtual-key/hotkey value for f6. |
| `vk_f8` | number | Virtual-key/hotkey value for f8. |
| `vk_mbutton` | number | Virtual-key/hotkey value for middle mouse button. |
| `vk_xbutton1` | number | Virtual-key/hotkey value for mouse X button 1. |
| `vk_xbutton2` | number | Virtual-key/hotkey value for mouse X button 2. |
| `vk_wallhopjumpkey` | number | Virtual-key/hotkey value for wallhopjump key. |
| `vk_leftbracket` | number | Virtual-key/hotkey value for leftbracket. |
| `vk_spamkey` | number | Virtual-key/hotkey value for spam key. |
| `vk_zkey` | number | Virtual-key/hotkey value for z key. |
| `vk_dkey` | number | Virtual-key/hotkey value for d key. |
| `vk_xkey` | number | Virtual-key/hotkey value for x key. |
| `vk_clipkey` | number | Virtual-key/hotkey value for clip key. |
| `vk_laughkey` | number | Virtual-key/hotkey value for laugh key. |
| `vk_bouncekey` | number | Virtual-key/hotkey value for bounce key. |
| `vk_bunnyhopkey` | number | Virtual-key/hotkey value for bunnyhop key. |
| `vk_shiftkey` | number | Virtual-key/hotkey value for shift key. |
| `vk_enterkey` | number | Virtual-key/hotkey value for enter key. |
| `vk_chatkey` | number | Virtual-key/hotkey value for chat key. |
| `vk_afkkey` | number | Virtual-key/hotkey value for afk key. |
| `vk_floorbouncekey` | number | Virtual-key/hotkey value for floorbounce key. |
| `vk_lagswitchkey` | number | Virtual-key/hotkey value for lagswitch key. |
| `vk_autohhjkey1` | number | Virtual-key/hotkey value for autohhj key1. |
| `vk_autohhjkey2` | number | Virtual-key/hotkey value for autohhj key2. |
| `selected_dropdown` | number | Selected dropdown option index. |
| `vk_wallkey` | number | Virtual-key/hotkey value for wall key. |
| `PreviousWallWalkSide` | number | Previously selected wallwalk side. |
| `selected_wallhop_instance` | number | Selected wallhop instance index. |
| `speed_slot` | number | Saved speed macro gear-slot number. |
| `desync_slot` | number | Saved desync macro gear-slot number. |
| `clip_slot` | number | Saved item-clip gear-slot number. |
| `spam_delay` | number | Spam-key delay in milliseconds. |
| `real_delay` | number | Resolved spam-key delay after parsing UI text. |
| `wallhop_dx` | number | Wallhop horizontal mouse movement delta. |
| `wallhop_dy` | number | Wallhop vertical mouse movement delta. |
| `wallhop_vertical` | number | Wallhop vertical movement amount. |
| `PreviousWallWalkValue` | number | Previously applied wallwalk value. |
| `maxfreezetime` | number | Maximum freeze duration in milliseconds. |
| `maxfreezeoverride` | number | Freeze duration override value. |
| `RobloxWallWalkValueDelay` | number | Delay value used by Roblox wallwalk behavior. |
| `speed_strengthx` | number | Speed macro X-axis strength. |
| `speedoffsetx` | number | Speed macro X-axis offset. |
| `speed_strengthy` | number | Speed macro Y-axis strength. |
| `speedoffsety` | number | Speed macro Y-axis offset. |
| `clip_delay` | number | Item-clip delay in milliseconds. |
| `AutoHHJKey1Time` | number | Auto-HHJ timing for key 1. |
| `AutoHHJKey2Time` | number | Auto-HHJ timing for key 2. |
| `RobloxPixelValue` | number | Parsed Roblox pixel-value setting. |
| `PreviousSensValue` | number | Previously applied Roblox sensitivity value. |
| `windowOpacityPercent` | number | Main-window opacity percentage. |
| `AntiAFKTime` | number | Anti-AFK interval value. |
| `display_scale` | number | UI display scaling factor. |
| `WindowPosX` | number | Saved main-window X position. |
| `WindowPosY` | number | Saved main-window Y position. |
| `lagswitch_max_duration` | number | Maximum lag-switch active duration in milliseconds. |
| `lagswitch_unblock_ms` | number | Lag-switch auto-unblock duration in milliseconds. |
| `lagswitchlagdelay` | number | Lag-switch delay amount in milliseconds. |
| `overlay_x` | number | Lag-overlay X position. |
| `overlay_y` | number | Lag-overlay Y position. |
| `overlay_size` | number | Lag-overlay size. |
| `overlay_bg_r` | number | Lag-overlay background red channel. |
| `overlay_bg_g` | number | Lag-overlay background green channel. |
| `overlay_bg_b` | number | Lag-overlay background blue channel. |
| `screen_width` | number | Current or last recorded screen width. |
| `screen_height` | number | Current or last recorded screen height. |

#### String values

| Name | Type | Description |
| --- | --- | --- |
| `settingsBuffer` | string | Target process name/filter text used by process-related macros. |
| `ItemDesyncSlot` | string | Text value for the desync gear slot. |
| `ItemSpeedSlot` | string | Text value for the speed gear slot. |
| `ItemClipSlot` | string | Text value for the item-clip gear slot. |
| `ItemClipDelay` | string | Text value for the item-clip delay. |
| `BunnyHopDelayChar` | string | Text value for bunny-hop delay. |
| `RobloxSensValue` | string | Text value for Roblox camera sensitivity. |
| `RobloxWallWalkValueChar` | string | Text value for wallwalk amount. |
| `RobloxWallWalkValueDelayChar` | string | Text value for wallwalk delay. |
| `WallhopPixels` | string | Text value for wallhop mouse-movement pixels. |
| `WallhopVerticalChar` | string | Text value for wallhop vertical movement. |
| `SpamDelay` | string | Text value for spam-key delay. |
| `RobloxPixelValueChar` | string | Text value for Roblox pixel value. |
| `CustomTextChar` | string | Text used by the custom text/paste macro. |
| `RobloxFPSChar` | string | Text value for Roblox FPS. |
| `AntiAFKTimeChar` | string | Text value for the anti-AFK interval. |
| `WallhopDelayChar` | string | Text value for wallhop duration. |
| `WallhopBonusDelayChar` | string | Text value for wallhop bonus delay before jumping. |
| `PressKeyDelayChar` | string | Text value for press-key delay. |
| `PressKeyBonusDelayChar` | string | Text value for press-key bonus delay. |
| `PasteDelayChar` | string | Text value for paste delay. |
| `HHJLengthChar` | string | Text value for HHJ length. |
| `HHJFreezeDelayOverrideChar` | string | Text value for HHJ freeze-delay override. |
| `HHJDelay1Char` | string | Text value for HHJ delay 1. |
| `HHJDelay2Char` | string | Text value for HHJ delay 2. |
| `HHJDelay3Char` | string | Text value for HHJ delay 3. |
| `AutoHHJKey1TimeChar` | string | Text value for Auto-HHJ key 1 timing. |
| `AutoHHJKey2TimeChar` | string | Text value for Auto-HHJ key 2 timing. |
| `FloorBounceDelay1Char` | string | Text value for floor-bounce delay 1. |
| `FloorBounceDelay2Char` | string | Text value for floor-bounce delay 2. |
| `FloorBounceDelay3Char` | string | Text value for floor-bounce delay 3. |
| `text` | string | Saved custom text macro content. |

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

# Lua Macro Scripting

Custom macros can be written as Lua scripts and imported into Spencer Macro Client. The scripting system supports `.smus`, `.hss`, `.lua`, and `.txt` files. Scripts run inside the macro runtime and can automate keyboard input, mouse movement, text entry, timing, freeze behavior, and lag-switch controls.

Imported scripts can also define their own custom ImGui settings by implementing `onSettings()`. The app runs that callback once in a non-rendering initialization pass when the script loads, then again in the selected-script panel to render the controls.
`onSettings()` should only call `ui.*` helpers. Input, mouse, process, sleep, and lag-switch APIs are blocked while settings are being rendered.
`onSettings()` has a 5-second hard timeout.
`onSettings()` is optional. If it is not defined, only the built-in script controls appear.
While a script is running, custom settings stay visible using the last cached `onSettings()` layout. Interactive controls become read-only, while `ui.dynamicTextbox()` remains live and copyable.

## Safety

Only import scripts from sources you trust. Lua scripts can simulate input and control process-related macro behavior.

Each script runs with a per-script Lua memory cap. The default is 64 MiB. Scripts can request a different cap with `-- @memoryLimitMB:`, clamped between 16 MiB and 256 MiB.
`onExecute()` and script load calls are timed based on active execution time. `sleep()` and input delays do not count against that timeout, so long-lived scripts can wait without being terminated. Timeout and cancellation errors cannot be suppressed with `pcall()` or `xpcall()`.
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

Add metadata at the top of the script using comment tags. Metadata fields are truncated if they exceed reasonable display limits.

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
| `log(message)` | Write a message to the log console (messages are truncated at 4096 bytes) |
| `sleep(ms)` | Pause the script for the specified number of milliseconds. Long sleeps are allowed and do not count against the execution timeout |
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
| `ui.dynamicTextbox(id, label, defaultValue, width, height)` | Render a read-only multi-line text box backed by a script-updated value |
| `ui.setDynamicText(id, text)` | Update a dynamic text box value (clamped to 4096 characters) |
| `ui.keybind(id, label, defaultValue, width)` | Render the standard SMU keybind picker (with hex display) and persist the combo as a hotkey value |

The current values are mirrored into a global `settings` table, so `onExecute()` can read them directly.
Script settings are stored in the save file under imported script data, separate from the main app settings.
Dynamic text boxes are saved with imported script UI state. After restarting SMU, the last dynamic text value may still be visible as a "ghost" until the script writes a new value. Scripts still start from a fresh Lua state, so previous dynamic text is not automatically appended unless the script preserves its own history.
UI IDs must be non-empty, must not contain embedded NUL bytes, and are limited to 128 bytes. A single `onSettings()` call may create up to 512 UI controls, and each script is capped at 4096 total unique UI IDs. Stored script UI strings are clamped to 4096 bytes.

### Input

#### Key and Hotkey Arguments

The input functions use two related but different argument formats:

| Argument kind | Used by | Meaning |
| --- | --- | --- |
| Single key | `pressKey()`, `holdKey()`, `releaseKey()`, `isKeyPressed()` | One physical keyboard key or mouse button. |
| Hotkey | `isHotkeyPressed()`, `ui.keybind()`, `@keybind:` | A key combination, usually one or more modifiers plus a main key. |

A single key is written as one key name:

```lua
pressKey("Space")
holdKey("LCtrl")
releaseKey("LCtrl")

if isKeyPressed("E") then
    log("E is pressed")
end
```

A hotkey may be written as a combination using `+`:

```lua
if isHotkeyPressed("LCtrl+K") then
    log("LCtrl+K is pressed")
end
```

Do **not** pass hotkey combinations to `pressKey()`, `holdKey()`, `releaseKey()`, or `isKeyPressed()`:

```lua
-- Wrong:
pressKey("LCtrl+K")

-- Correct:
holdKey("LCtrl")
pressKey("K")
releaseKey("LCtrl")
```

For typing normal text, prefer `typeText()`:

```lua
typeText("hello")
```

Do not use repeated `pressKey()` calls for ordinary text entry unless you specifically need key-level behavior. `typeText()` input is capped at 4096 characters per call.

Key names are case-insensitive. Spaces, underscores, and dashes are ignored, so these are equivalent:

```lua
pressKey("PageDown")
pressKey("page down")
pressKey("page_down")
pressKey("page-down")
```

Numeric SMU key codes are also accepted by the input APIs, but named keys are preferred because they are easier to read and less likely to depend on implementation details.

##### Supported key names

The Lua API supports the following named keys.

###### Letters

| Key names |
| --- |
| `A` through `Z` |

###### Number row

| Key names |
| --- |
| `0` through `9` |

###### Function keys

| Key names |
| --- |
| `F1` through `F24` |

###### Numpad keys

| Key names |
| --- |
| `Numpad0` through `Numpad9` |

###### Mouse buttons

| Key name | Aliases |
| --- | --- |
| `LMB` | `MouseLeft`, `LeftMouse` |
| `RMB` | `MouseRight`, `RightMouse` |
| `MMB` | `MouseMiddle`, `MiddleMouse` |
| `Mouse4` | `XButton1` |
| `Mouse5` | `XButton2` |

###### Modifier keys

| Key name | Aliases |
| --- | --- |
| `Shift` | |
| `LShift` | |
| `RShift` | |
| `Ctrl` | `Control` |
| `LCtrl` | |
| `RCtrl` | |
| `Alt` | |
| `LAlt` | |
| `RAlt` | |
| `Win` | `Super`, `Meta` |

###### Navigation and editing keys

| Key name | Aliases |
| --- | --- |
| `Space` | |
| `Enter` | `Return` |
| `Escape` | `Esc` |
| `Tab` | |
| `Backspace` | |
| `Delete` | `Del` |
| `Insert` | `Ins` |
| `Home` | |
| `End` | |
| `PageUp` | `PgUp` |
| `PageDown` | `PgDn` |
| `Up` | |
| `Down` | |
| `Left` | |
| `Right` | |

###### Punctuation keys

| Key name | Aliases |
| --- | --- |
| `Slash` | `/` |
| `Backslash` | `\` |
| `Equals` | `Equal`, `=` |
| `Minus` | `-` |
| `Comma` | `,` |
| `Period` | `Dot`, `.` |
| `Semicolon` | `;` |
| `Quote` | `Apostrophe`, `'` |
| `LeftBracket` | `BracketLeft`, `[` |
| `RightBracket` | `BracketRight`, `]` |
| `Grave` | `Backtick`, `` ` `` |

###### Lock keys

| Key name |
| --- |
| `CapsLock` |
| `NumLock` |
| `ScrollLock` |

##### Hotkey strings

Hotkey strings combine modifiers and one main key with `+`:

```lua
isHotkeyPressed("Ctrl+F")
isHotkeyPressed("Alt+Shift+X")
isHotkeyPressed("LCtrl+K")
```

Use `isHotkeyPressed()` when checking whether a combination is currently pressed.

Use `ui.keybind()` when creating a configurable hotkey in a script UI:

```lua
local hotkey = ui.keybind("Example hotkey", "LCtrl+K")

if isHotkeyPressed(hotkey) then
    log("Configured hotkey pressed")
end
```

Use `@keybind:` to define the script's default activation hotkey (the same as the "Keybind" field shown in the app's script list):

```lua
-- @keybind: LCtrl+K
```

To expose a *configurable* keybind inside your script, create it in `onSettings()` with `ui.keybind()` and read the result from the global `settings` table:

```lua
function onSettings()
    ui.keybind("actionHotkey", "Action hotkey", "LCtrl+K")
end

function onExecute()
    local actionHotkey = settings.actionHotkey
    if actionHotkey and isHotkeyPressed(actionHotkey) then
        log("Action hotkey pressed")
    end
end
```

| Function | Description |
| --- | --- |
| `pressKey(key, delay)` | Press and release a key. `delay` defaults to 50 ms |
| `holdKey(key)` | Hold a key down |
| `releaseKey(key)` | Release a held key |
| `isKeyPressed(key)` | Return whether a key is currently pressed |
| `isHotkeyPressed(hotkey)` | Return whether a hotkey combo is currently pressed (use values from `ui.keybind`) |
| `typeText(text, delay)` | Type text with an optional per-character delay. `delay` defaults to 30 ms |
| `moveMouse(dx, dy)` | Move the mouse relative to its current position. On Windows, this relative movement is multiplied by the saved `display_scale` percentage before being sent |
| `moveMouseAbs(x, y, mode)` | Move the mouse to an absolute position on the monitor containing the cursor by calculating the needed relative delta from the current cursor position. `mode` is optional and defaults to `"pixels"`; valid modes are `"pixels"`, `"percent"`, `"scaled720p"`, `"scaled1080p"`, `"scaled1440p"`, and `"scaled2160p"`. Unlike `moveMouse`, this function uses a raw relative move internally and does not apply `display_scale` |
| `getPixelColor(x, y, mode)` | Return the pixel color at a position on the monitor containing the cursor as a `"#RRGGBB"` string. Uses the same coordinate modes as `moveMouseAbs`. Not available while rendering `onSettings()` |
| `moveDegrees(dx, dy)` | Move the mouse using degree units derived from saved Roblox sensitivity and Cam-Fix settings. `dx` and `dy` may be integers or floats. Positive `dy` moves upward |
| `mouseWheel(delta)` | Scroll the mouse wheel |


`moveMouseAbs(x, y, mode)` and `getPixelColor(x, y, mode)` target the monitor containing the current cursor, not the full virtual desktop and not the saved `screen_width` / `screen_height` settings. Those saved settings are the SMU application window size. In `"pixels"` mode, `(0, 0)` is the top-left of the active monitor. In `"percent"` mode, `x` and `y` must be between `0` and `100`. In scaled modes, the coordinate pair is treated as if it was authored for the named base resolution and then scaled to the active monitor size.

Examples:

```lua
moveMouseAbs(960, 540)
moveMouseAbs(50, 50, "percent")
moveMouseAbs(960, 540, "scaled1080p")
moveMouseAbs(1280, 720, "scaled1440p")

local color = getPixelColor(50, 50, "percent")
if color == "#FF0000" then
    log("center pixel is red")
end
```

Windows: `getPixelColor()` reuses a cached monitor frame when possible and refreshes that cache approximately once per monitor refresh interval. It is suitable for moderate polling loops and repeated samples from the same frame, but it is still not a bulk image-scanning API; repeated scans across many pixels should use a future cached screenshot/buffer API instead of calling `getPixelColor()` thousands of times per frame.

Linux: `getPixelColor()` still uses the X11/XWayland screen-read path. Native Wayland sessions without usable X11 access remain unsupported for arbitrary global screen reads.

Linux note: absolute-coordinate APIs currently need X11/XWayland cursor-position and screen-read access. `moveMouseAbs()` still injects movement through the existing relative `uinput` path. Native Wayland sessions without usable X11 access report descriptive `moveMouseAbs failed: ...` or `getPixelColor failed: ...` script errors in the selected script status panel, explaining that global cursor/screen position or arbitrary screen reads are unavailable.

`moveDegrees(dx, dy)` caches its conversion settings once when the script instance starts. It uses `RobloxSensValue` and `camfixtoggle` to convert degrees into pixels with the same formula used by the built-in wallhop/rotation UI.

Limitations:

- `moveDegrees()` is inherently lossy because the final mouse movement is rounded to whole pixels before it is sent to the input backend.
- Higher Roblox sensitivity produces fewer pixels per degree, so the same requested angle has less precision and may land farther from the exact target after rounding.
- Lower Roblox sensitivity produces more pixels per degree, so fractional degree inputs can be represented more accurately.
- Example: with Cam-Fix off, sensitivity `0.01` gives about `200` pixels per degree, while sensitivity `4.0` gives about `0.5` pixels per degree.
- Because of that pixel quantization, `moveDegrees(149.32, 0)` may track very closely at low sensitivity but can resolve to a nearby angle instead of exactly `149.32` at higher sensitivities.

### Process Control

| Function | Description |
| --- | --- |
| `freeze(enable)` | Freeze or unfreeze the target process |
| `robloxFreeze(enable)` | Alias for `freeze(enable)` |
| `roblox_freeze(enable)` | Alias for `freeze(enable)` |
| `lagSwitch(enable, options)` | Toggle lag-switch behavior, optionally applying a temporary config table first |
| `lagswitch(enable)` | Alias for `lagSwitch(enable)` |
| `getLagSwitchConfig()` | Return the effective lag-switch config table currently used by scripts/backend |
| `setLagSwitchConfig(options)` | Apply a temporary script-owned lag-switch config override |
| `getLagSwitchStatus()` | Return lag-switch availability, active state, target mode, and unsupported reason |
| `clearLagSwitchConfig()` | Clear this script's config override without modifying saved app/profile settings |

Lag-switch config overrides are temporary. They apply only while the script run owns them, do not write to the main profile/save settings, and are cleared when the script completes, errors, is cancelled, is reloaded, or is removed.
If multiple scripts control lag-switch settings at the same time, the most recent script-owned override wins. A script can only clear its own current override.
Lag-switch APIs are not available inside `onSettings()`.
Lag-switch is currently Windows only (Windivert), we are actively working on a Linux port.

The config table supports these keys:

| Key | Type | Description |
| --- | --- | --- |
| `hardBlockInbound` | boolean | Hard-block inbound/download packets |
| `hardBlockOutbound` | boolean | Hard-block outbound/upload packets |
| `fakeLag` | boolean | Delay matching packets instead of only hard-blocking |
| `fakeLagInbound` | boolean | Delay inbound/download packets when fake lag is enabled |
| `fakeLagOutbound` | boolean | Delay outbound/upload packets when fake lag is enabled |
| `fakeLagDelayMs` | integer | Fake-lag delay in milliseconds |
| `targetMode` | string | `"roblox"`, `"all"`, or `"custom"` |
| `useUdp` | boolean | Include UDP traffic |
| `useTcp` | boolean | Include TCP traffic |
| `preventDisconnect` | boolean | Use the Roblox disconnect-prevention behavior while hard-blocking |
| `autoUnblock` | boolean | Keep the setting in the effective config for compatibility with built-in behavior |
| `maxDurationSeconds` | number | Auto-unblock duration value in seconds |
| `unblockDurationMs` | integer | Auto-unblock release duration in milliseconds |
| `remoteIps` | table | Custom-mode IPv4 address list, exact-match only |
| `remotePorts` | table | Custom-mode remote TCP/UDP port list |
| `includeRobloxDynamicIps` | boolean | In custom mode, also include the Roblox static range and discovered Roblox server IPs |

Custom targeting is intentionally limited to validated IP and port filters. The Lua API does not expose raw WinDivert handles, packet payload bytes, packet receive/send functions, or arbitrary packet injection.

Examples:

```lua
function onExecute()
    lagSwitch(true, {
        fakeLag = true,
        fakeLagDelayMs = 250,
        hardBlockInbound = false,
        hardBlockOutbound = false,
        targetMode = "roblox",
        useUdp = true,
        useTcp = false,
    })
    sleep(2000)
    lagSwitch(false)
end
```

```lua
function onExecute()
    setLagSwitchConfig({
        targetMode = "custom",
        remoteIps = { "203.0.113.10" },
        remotePorts = { 7777, 9000 },
        useUdp = true,
        useTcp = false,
        fakeLag = true,
        fakeLagDelayMs = 150,
    })

    local status = getLagSwitchStatus()
    if status.available then
        lagSwitch(true)
        sleep(1000)
        lagSwitch(false)
    else
        log("Lag switch unavailable: " .. status.unsupportedReason)
    end
end
```

## Reading Saved Settings

Use `getSavedValue(name)` to read settings that are currently loaded in memory and persisted in the save file. The name must match the saved key exactly.

### Allowed `getSavedValue()` Variables

`getSavedValue(name)` can read only the saved settings registered by the app. It returns a Lua boolean, number, string, or `nil` if the name is not in this allowlist.

Scripts also receive the global `settings` table for per-script UI state created with `ui.*` helpers. The names below are for app/profile settings read through `getSavedValue()`.

#### Boolean values

| Name | Type | Description |
| --- | --- | --- |
| `macrotoggled` | boolean | Master macro enable toggle. Anti-AFK is controlled separately. |
| `shiftswitch` | boolean | Unused legacy saved flag; currently persisted for compatibility but not read by the runtime. |
| `wallhopswitch` | boolean | First Wallhop/Rotation instance: use left-flick direction instead of right-flick direction. |
| `wallhopcamfix` | boolean | Legacy first Wallhop/Rotation cam-fix flag; currently saved/synced for compatibility, while global `camfixtoggle` drives the active calculations. |
| `unequiptoggle` | boolean | Item Unequip COM Offset: keep the selected item equipped after the emote/message instead of unequipping it. |
| `isspeedswitch` | boolean | Speedglitch mode flag: `false` means toggle mode, `true` means hold-key mode. |
| `isfreezeswitch` | boolean | Freeze mode flag: `false` means hold-to-freeze, `true` means press-to-toggle. |
| `iswallwalkswitch` | boolean | Wall-Walk mode flag: `false` means toggle mode, `true` means hold-key mode. |
| `isspamswitch` | boolean | First Spam a Key instance: `false` means toggle mode, `true` means hold-key mode. |
| `isitemclipswitch` | boolean | Item Clip mode flag: `false` means toggle mode, `true` means hold-key mode. |
| `autotoggle` | boolean | Wall Helicopter High Jump: automatically hold the configured Auto-HHJ keys before the freeze sequence. |
| `toggle_jump` | boolean | First Wallhop/Rotation instance: press the configured wallhop jump key during the wallhop. |
| `toggle_flick` | boolean | First Wallhop/Rotation instance: perform the flick-back movement after the initial flick. |
| `camfixtoggle` | boolean | Global Game Uses Cam-Fix setting; changes sensitivity-derived pixel calculations for Speedglitch, Wallhop, Wall-Walk, and Ledge Bounce. |
| `wallwalktoggleside` | boolean | Wall-Walk: use the left-flick side instead of the default side. |
| `antiafktoggle` | boolean | Enable the Anti-AFK timer/key press routine. |
| `fasthhj` | boolean | Wall Helicopter High Jump: decrease the default freeze duration in speedrunner mode. |
| `globalzoomin` | boolean | Use mouse-wheel zoom instead of the configured shiftlock key for HHJ-style shiftlock steps. |
| `globalzoominreverse` | boolean | Reverse the mouse-wheel direction used when `globalzoomin` is enabled. |
| `wallesslhjswitch` | boolean | Walless LHJ: use the left-sided setup key instead of the default right-sided setup key. |
| `chatoverride` | boolean | Force the chat-open key to `/` instead of relying on the custom `vk_chatkey` setting. |
| `bounceautohold` | boolean | Ledge Bounce: automatically hold the movement key after the bounce setup. |
| `bouncerealignsideways` | boolean | Ledge Bounce: use the horizontal realignment branch after the bounce. |
| `bouncesidetoggle` | boolean | Ledge Bounce: use the left-sided bounce path instead of the default side. |
| `laughmoveswitch` | boolean | Laugh Clip: disable the automatic `S` key hold during the clip sequence. |
| `freezeoutsideroblox` | boolean | Legacy compatibility mirror for Freeze foreground restriction. `true` means Freeze is allowed outside Roblox; internally this is the inverse of section 0's Disable outside Roblox flag. |
| `takeallprocessids` | boolean | Freeze/process control: target every matching process ID instead of only the newest/main matching process. |
| `ontoptoggle` | boolean | Keep the main SMU window always on top. |
| `bunnyhopsmart` | boolean | Smart Bunnyhop: temporarily suppress bunnyhop while chat is open until Enter or left-click closes it. |
| `presskeyinroblox` | boolean | First Press a Button instance: restrict the macro to Roblox foreground focus when enabled. |
| `unequipinroblox` | boolean | Item Unequip COM Offset: restrict the macro to Roblox foreground focus when enabled. |
| `doublepressafkkey` | boolean | Anti-AFK: press the configured Anti-AFK key twice per Anti-AFK run. |
| `useoldpaste` | boolean | Use the legacy Unicode/chat typing path for pasted chat text. |
| `floorbouncehhj` | boolean | Floor Bounce: run the optional HHJ-style shiftlock/helicoptering sequence after the floor bounce. |
| `HHJFreezeDelayApply` | boolean | Wall Helicopter High Jump: apply `HHJFreezeDelayOverride` instead of the default freeze-delay timing. |
| `islagswitchswitch` | boolean | Lag Switch mode flag: `false` means hold-key mode, `true` means press-to-toggle. |
| `prevent_disconnect` | boolean | Lag Switch: use the disconnect-prevention behavior while blocking/delaying Roblox traffic. |
| `lagswitchoutbound` | boolean | Lag Switch: hard-block outbound/upload packets. |
| `lagswitchinbound` | boolean | Lag Switch: hard-block inbound/download packets. |
| `lagswitchtargetroblox` | boolean | Lag Switch: restrict filtering to Roblox traffic instead of all matching network traffic. |
| `lagswitchlaginbound` | boolean | Fake Lag: delay inbound/download packets when fake-lag mode is enabled. |
| `lagswitchlagoutbound` | boolean | Fake Lag: delay outbound/upload packets when fake-lag mode is enabled. |
| `lagswitchlag` | boolean | Lag Switch: enable fake-lag packet delay mode instead of only hard-blocking. |
| `lagswitchusetcp` | boolean | Lag Switch: include TCP traffic in addition to UDP traffic. |
| `lagswitch_autounblock` | boolean | Lag Switch: automatically stop lagging after `lagswitch_max_duration` seconds. |
| `show_lag_overlay` | boolean | Show the Windows lag-switch status overlay. |
| `overlay_hide_inactive` | boolean | Hide the lag-switch overlay when the lag switch is not actively blocking/delaying traffic. |
| `overlay_use_bg` | boolean | Draw a background behind the lag-switch overlay text. |

#### Numeric values

| Name | Type | Description |
| --- | --- | --- |
| `vk_mbutton` | number | Freeze macro trigger hotkey. Legacy name reflects the default middle-mouse binding. |
| `vk_f5` | number | Item Desync macro trigger hotkey. Legacy name reflects the default F5 binding. |
| `vk_xbutton1` | number | Wall Helicopter High Jump trigger hotkey. Legacy name reflects the default mouse XButton1 binding. |
| `vk_xkey` | number | Speedglitch trigger hotkey. Legacy name reflects the default X binding. |
| `vk_f8` | number | Item Unequip COM Offset trigger hotkey. Legacy name reflects the default F8 binding. |
| `vk_zkey` | number | First Press a Button instance trigger hotkey. Legacy name reflects the default Z binding. |
| `vk_dkey` | number | First Press a Button instance output key: the key the macro presses after the trigger. Legacy name reflects the default D binding. |
| `vk_xbutton2` | number | First Wallhop/Rotation instance trigger hotkey. Legacy name reflects the default mouse XButton2 binding. |
| `vk_wallhopjumpkey` | number | First Wallhop/Rotation instance jump key used during the wallhop sequence. |
| `vk_f6` | number | Walless LHJ trigger hotkey. Legacy name reflects the default F6 binding. |
| `vk_clipkey` | number | Item Clip trigger hotkey. |
| `vk_laughkey` | number | Laugh Clip trigger hotkey. |
| `vk_wallkey` | number | Wall-Walk trigger hotkey. |
| `vk_leftbracket` | number | First Spam a Key instance trigger hotkey. Legacy name reflects the default `[` binding. |
| `vk_spamkey` | number | First Spam a Key instance output key: the key repeatedly pressed while spam is active. |
| `vk_bouncekey` | number | Ledge Bounce trigger hotkey. |
| `vk_bunnyhopkey` | number | Smart Bunnyhop trigger/output hotkey. The macro detects this physical key and also injects the same key for hopping. |
| `vk_floorbouncekey` | number | Floor Bounce trigger hotkey. |
| `vk_lagswitchkey` | number | Lag Switch trigger hotkey. |
| `vk_shiftkey` | number | Custom shiftlock key used by HHJ-style macros when `globalzoomin` is disabled. |
| `vk_chatkey` | number | Chat-open key used by chat/emote macros and by Smart Bunnyhop's chat suppression logic. |
| `vk_enterkey` | number | Key used to submit chat messages/emotes after paste/type operations. |
| `vk_afkkey` | number | Anti-AFK key that the Anti-AFK routine presses. |
| `vk_autohhjkey1` | number | Wall Helicopter High Jump Auto-HHJ first key to hold before the freeze sequence. |
| `vk_autohhjkey2` | number | Wall Helicopter High Jump Auto-HHJ second key to hold before the freeze sequence. |
| `selected_section` | number | Currently selected macro section index in the UI. Section IDs are 0 Freeze, 1 Item Desync, 2 Wall Helicopter High Jump, 3 Speedglitch, 4 Item Unequip COM Offset, 5 Press a Button, 6 Wallhop/Rotation, 7 Walless LHJ, 8 Item Clip, 9 Laugh Clip, 10 Wall-Walk, 11 Spam a Key, 12 Ledge Bounce, 13 Smart Bunnyhop, 14 Floor Bounce, 15 Lag Switch. |
| `selected_dropdown` | number | Selected Item Unequip COM Offset emote dropdown index: 0 `/e dance2`, 1 `/e laugh`, 2 `/e cheer`. |
| `PreviousWallWalkSide` | number | Unused legacy wall-walk side cache; currently persisted for compatibility but not read by the runtime. |
| `selected_wallhop_instance` | number | Currently selected Wallhop/Rotation instance index in the UI. |
| `speed_slot` | number | Item Unequip COM Offset gear slot used after the emote/message sequence. |
| `desync_slot` | number | Item Desync gear slot repeatedly equipped/released while the trigger is held. |
| `clip_slot` | number | Item Clip gear slot repeatedly equipped/released while item clip is active. |
| `spam_delay` | number | First Spam a Key instance user-facing spam delay in milliseconds. |
| `real_delay` | number | First Spam a Key instance half-cycle delay actually used between output-key press/release operations. |
| `wallhop_dx` | number | First Wallhop/Rotation instance initial horizontal mouse flick amount. |
| `wallhop_dy` | number | First Wallhop/Rotation instance horizontal flick-back amount. Despite the name, this is not vertical movement. |
| `wallhop_vertical` | number | First Wallhop/Rotation instance vertical mouse movement paired with the flick. |
| `PreviousWallWalkValue` | number | Cached Roblox sensitivity value last used to recalculate Wall-Walk pixel movement. |
| `maxfreezetime` | number | Freeze auto-unfreeze timeout in seconds. |
| `maxfreezeoverride` | number | Freeze refreeze delay in milliseconds after an automatic unfreeze. |
| `RobloxWallWalkValueDelay` | number | Wall-Walk delay between the two flicks, stored as microseconds. |
| `speed_strengthx` | number | Speedglitch/HHJ first horizontal mouse movement amount for the timed 180-degree turn. |
| `speedoffsetx` | number | Unused legacy Speedglitch X offset; currently persisted for compatibility but not read by the runtime. |
| `speed_strengthy` | number | Speedglitch/HHJ second horizontal mouse movement amount for the timed 180-degree turn. |
| `speedoffsety` | number | Unused legacy Speedglitch Y offset; currently persisted for compatibility but not read by the runtime. |
| `clip_delay` | number | Item Clip total equip/release cycle delay in milliseconds; the runtime uses half for hold and half for release spacing. |
| `AutoHHJKey1Time` | number | Wall Helicopter High Jump Auto-HHJ first key hold time in milliseconds. |
| `AutoHHJKey2Time` | number | Wall Helicopter High Jump Auto-HHJ second key hold time in milliseconds. |
| `RobloxPixelValue` | number | Calculated Speedglitch/HHJ 180-degree turn pixel value derived from Roblox sensitivity and `camfixtoggle`. |
| `PreviousSensValue` | number | Cached Roblox sensitivity value last used to recalculate sensitivity-derived pixel values. |
| `windowOpacityPercent` | number | Main window opacity percentage. |
| `AntiAFKTime` | number | Anti-AFK interval in minutes. |
| `display_scale` | number | Windows mouse movement scale percentage. It affects `moveMouse()` and other scaled relative movement paths, but `moveMouseAbs()` bypasses it so absolute targeting can use raw relative deltas. `getPixelColor()` only reads screen pixels and is not affected by `display_scale`. |
| `WindowPosX` | number | Saved main window X position. |
| `WindowPosY` | number | Saved main window Y position. |
| `lagswitch_max_duration` | number | Lag Switch auto-unlag timeout in seconds. |
| `lagswitch_unblock_ms` | number | Lag Switch auto-unblock/unlag duration in milliseconds. |
| `lagswitchlagdelay` | number | Fake Lag packet delay amount in milliseconds. |
| `overlay_x` | number | Lag-switch overlay X position. |
| `overlay_y` | number | Lag-switch overlay Y position. |
| `overlay_size` | number | Lag-switch overlay text size. |
| `overlay_bg_r` | number | Lag-switch overlay background red channel, normalized 0.0 to 1.0. |
| `overlay_bg_g` | number | Lag-switch overlay background green channel, normalized 0.0 to 1.0. |
| `overlay_bg_b` | number | Lag-switch overlay background blue channel, normalized 0.0 to 1.0. |
| `screen_width` | number | Saved/calculated application window width value. |
| `screen_height` | number | Saved/calculated application window height value. |
| `active_monitor_width` | number | Current monitor width in pixels for the monitor containing the cursor (transient; not saved to disk). |
| `active_monitor_height` | number | Current monitor height in pixels for the monitor containing the cursor (transient; not saved to disk). |

#### String values

| Name | Type | Description |
| --- | --- | --- |
| `settingsBuffer` | string | Target Roblox executable/process-name text, or PID list text on Linux/Wine paths. |
| `ItemDesyncSlot` | string | Text backing the Item Desync gear-slot input. |
| `ItemSpeedSlot` | string | Text backing the Item Unequip COM Offset gear-slot input. |
| `ItemClipSlot` | string | Text backing the Item Clip gear-slot input. |
| `ItemClipDelay` | string | Text backing the Item Clip delay input. |
| `BunnyHopDelayChar` | string | Text backing the Smart Bunnyhop delay input in milliseconds. |
| `RobloxSensValue` | string | Text backing the Roblox sensitivity input used for sensitivity-derived pixel calculations. |
| `RobloxWallWalkValueChar` | string | Text backing the Wall-Walk pixel-value input. |
| `RobloxWallWalkValueDelayChar` | string | Text backing the Wall-Walk delay-between-flicks input. |
| `WallhopPixels` | string | First Wallhop/Rotation instance text backing the flick pixel amount. |
| `WallhopVerticalChar` | string | First Wallhop/Rotation instance text backing vertical pixel movement. |
| `SpamDelay` | string | First Spam a Key instance text backing the spam delay input. |
| `RobloxPixelValueChar` | string | Text backing the Speedglitch/HHJ 180-degree turn pixel-value input. |
| `CustomTextChar` | string | Custom Item Unequip COM Offset chat message. If non-empty, it disables gear equipping and only pastes/sends this text. |
| `RobloxFPSChar` | string | Text backing the Roblox FPS input used for frame-delay calculations. |
| `AntiAFKTimeChar` | string | Text backing the Anti-AFK interval input. |
| `WallhopDelayChar` | string | First Wallhop/Rotation instance text backing wallhop length in milliseconds. |
| `WallhopBonusDelayChar` | string | First Wallhop/Rotation instance text backing bonus delay before jumping, in milliseconds. |
| `PressKeyDelayChar` | string | First Press a Button instance text backing output-key hold length in milliseconds. |
| `PressKeyBonusDelayChar` | string | First Press a Button instance text backing delay before pressing the output key, in milliseconds. |
| `PasteDelayChar` | string | Text backing the delay between chat typing/paste key events in milliseconds. |
| `HHJLengthChar` | string | Text backing HHJ flick/helicopter duration in milliseconds. |
| `HHJFreezeDelayOverrideChar` | string | Text backing the optional HHJ freeze-delay override in milliseconds. |
| `HHJDelay1Char` | string | Text backing HHJ delay after unfreezing and before shiftlock/zoom is held, in milliseconds. |
| `HHJDelay2Char` | string | Text backing HHJ delay before helicoptering/spinning starts, in milliseconds. |
| `HHJDelay3Char` | string | Text backing HHJ delay while shiftlock/zoom remains held after spinning starts, in milliseconds. |
| `AutoHHJKey1TimeChar` | string | Text backing Auto-HHJ first key hold time in milliseconds. |
| `AutoHHJKey2TimeChar` | string | Text backing Auto-HHJ second key hold time in milliseconds. |
| `FloorBounceDelay1Char` | string | Text backing Floor Bounce HHJ delay after unfreezing and before shiftlocking, in milliseconds. |
| `FloorBounceDelay2Char` | string | Text backing Floor Bounce HHJ delay before helicoptering, in milliseconds. |
| `FloorBounceDelay3Char` | string | Text backing Floor Bounce HHJ helicoptering duration in milliseconds. |
| `text` | string | Selected Item Unequip COM Offset emote text from the dropdown, such as `/e dance2`, `/e laugh`, or `/e cheer`. |

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
-- @name: Hotkey Telemetry Demo
-- @desc: Shows custom settings, save-file reads, and live dynamic text updates
-- @version: 1.0
-- @keybind: F6

local liveLog = ""

local function pushStatus(line)
    log(line)
    if liveLog == "" then
        liveLog = line
    else
        liveLog = liveLog .. "\n" .. line
    end
    ui.setDynamicText("telemetry_output", liveLog)
end

function onSettings()
    ui.text("This demo shows custom settings, live dynamic text, and save-file reads.", 420)
    ui.separator(8)
    ui.checkbox("telemetry_enabled", "Enable telemetry run", true, 260)
    ui.sliderInt("telemetry_samples", "Sample count", 12, 1, 60, 260)
    ui.sliderFloat("telemetry_delay_ms", "Delay between samples (ms)", 125, 10, 1000, 260)
    ui.keybind("telemetry_hotkey", "Hotkey to watch", "F6", 260)
    ui.textbox("telemetry_label", "Label", "Telemetry", 260)
    ui.dynamicTextbox("telemetry_output", "Live Output", "No samples yet.", 560, 220)
end

function onExecute()
    if not settings.telemetry_enabled then
        pushStatus("Telemetry run skipped because the custom setting is disabled.")
        sleep(800)
        return
    end

    liveLog = ""

    local label = settings.telemetry_label or "Telemetry"
    local savedCamfix = getSavedValue("camfixtoggle")
    local samples = settings.telemetry_samples or 12
    local delayMs = math.max(0, math.floor((settings.telemetry_delay_ms or 125) + 0.5))
    local watchedHotkey = settings.telemetry_hotkey or "F6"

    pushStatus(string.format("%s started on %s (SMU %s)", label, getPlatform(), getSMUVersion()))
    pushStatus("Saved camfixtoggle = " .. tostring(savedCamfix))

    for i = 1, samples do
        local t0 = nowMicros()
        local pressed = isHotkeyPressed(watchedHotkey)
        local elapsedMs = (nowMicros() - t0) / 1000.0
        pushStatus(string.format("[%02d/%02d] %s pressed=%s poll=%.3fms",
            i, samples, tostring(watchedHotkey), tostring(pressed), elapsedMs))
        sleep(delayMs)
    end

    pushStatus(label .. " finished.")
end
```

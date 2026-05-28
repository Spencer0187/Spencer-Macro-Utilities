# Lua Macro Scripting

Lua scripts let you build custom SMU macros with their own settings panels, live status output, managed hotkeys, input callbacks, mouse/pixel automation, process control, and lag-switch behavior. Imported scripts can automate keyboard input, mouse movement, text entry, timing, pixel reads, freeze behavior, and lag-switch controls. Scripts may also define their own settings UI with `onSettings()`.

Supported file extensions: `.smus`, `.hss`, `.lua`, `.txt`

## Capability Matrix

| Feature | Windows | Linux X11/XWayland | Native Wayland | macOS |
| --- | --- | --- | --- | --- |
| Keyboard input | yes | yes | yes | yes with Accessibility |
| Global key state reads | yes | yes | yes | yes with Accessibility |
| Relative mouse | yes | yes | yes | yes with Accessibility |
| Absolute mouse | yes | yes via X11 | no / limited | yes with Accessibility |
| Pixel reads | yes | yes via X11 | planned | yes with Screen Recording |
| Freeze | yes | yes | yes | yes when process signals are permitted |
| Lag switch | yes | planned | planned | no |

On Linux today, absolute mouse coordinates and pixel reads rely on X11/XWayland access. Native Wayland usually blocks global cursor-position and arbitrary screen-read APIs, and the Linux lag-switch backend is not implemented yet. On macOS, SMU asks for Accessibility before synthetic input and global key state reads, and Screen Recording before screen pixel reads.

## Quick Start

Every script must define `onExecute()`. Two other callbacks are optional:

- `onSettings()` builds a per-script settings panel with the `ui.*` helpers.
- `onCleanup(reason)` runs when the script exits, is cancelled, times out, or errors.

The host also provides a global `settings` table for values created by `onSettings()`.

### Minimal Script

```lua
-- @name: My First Script
-- @desc: Press Space five times
-- @author: YourName
-- @version: 1.0
-- @keybind: F6

function onExecute()
    for i = 1, 5 do
        pressKey("Space")
        sleep(100)
    end
end
```

Import scripts from the app, or place them in a `scripts/` folder next to the executable so they load on startup.

### Add Script Settings

```lua
-- @name: Configurable Space Press
-- @keybind: F6

function onSettings()
    ui.sliderInt("count", "Press count", 5, 1, 20, 260)
    ui.sliderInt("delay_ms", "Delay (ms)", 100, 0, 1000, 260)
    ui.dynamicTextbox("status", "Status", "Idle", 420, 100)
end

function onExecute()
    ui.setDynamicText("status", "Running")

    local count = settings.count or 5
    local delayMs = settings.delay_ms or 100

    for i = 1, count do
        pressKey("Space")
        sleep(delayMs)
    end
end

function onCleanup(reason)
    ui.setDynamicText("status", "Stopped: " .. tostring(reason))
end
```

### Script Metadata

Add metadata at the top of the file with Lua comments:

| Tag | Description |
| --- | --- |
| `-- @name:` | Display name shown in the script list |
| `-- @desc:` or `-- @description:` | Description shown in the details panel |
| `-- @author:` | Script author |
| `-- @version:` | Script version |
| `-- @keybind:` | Default activation hotkey such as `F5` or `LCtrl+K` |
| `-- @memoryLimitMB:` | Optional Lua memory cap in MiB, clamped from `16` to `256` |

If `@name:` is omitted, SMU uses the file name. Metadata is truncated to reasonable display limits.

### Lifecycle Notes

- `onSettings()` is optional. If absent, only the built-in script controls appear.
- SMU calls `onSettings()` once in a non-rendering initialization pass when the script loads, then again when rendering the selected-script panel.
- While a script is running, custom settings remain visible using the cached `onSettings()` layout. Interactive controls become read-only, while `ui.dynamicTextbox()` remains live and copyable.
- Press the script hotkey again or use Force Stop to cancel a running script.

## Recipes

### Toggle-Hold a Managed Combo

Use the managed `input` API when you want SMU to keep combo keys balanced and release them automatically on stop:

```lua
function onSettings()
    ui.hotkey("trigger", "Trigger", "F7", 260)
    ui.keyCombo("target", "Target combo", "Ctrl+W", 260)
end

function onExecute()
    input.onPressed(settings.trigger, function()
        input.toggleHeld(settings.target)
    end)
    input.listenUntilCancelled(2)
end
```

### Mirror One Hotkey to Another

`input.mirror()` is shorthand for listening to a source hotkey and applying that state to a managed target combo:

```lua
function onSettings()
    ui.hotkey("source", "Source", "RMB", 260)
    ui.keyCombo("target", "Target", "G", 260)
end

function onExecute()
    input.mirror(settings.source, settings.target, { strict = true })
    input.listenUntilCancelled(2)
end
```

For a fuller example, see `scripts/examples/lock_mirror_rebinder.smus`.

### Sample a Pixel

Use absolute coordinates for desktop-style screen targeting:

```lua
function onExecute()
    setMouseMotionMode("absolute")

    local color = getPixelColor(50, 50, "percent", "rgb")
    if color.r == 255 and color.g == 0 and color.b == 0 then
        log("The center area is red")
    end
end
```

On Linux, `getPixelColor()` currently requires X11/XWayland access.

### Poll Efficiently in a Tight Loop

Use `checkpoint()` or `sleepMicros()` instead of a Lua busy-wait:

```lua
function onExecute()
    while not isCancelled() do
        if shouldYield(1000) then
            checkpoint()
        end

        local color = getPixelColor(50, 50, "percent")
        if color == "#FF0000" then
            pressKey("Space")
        end

        sleepMicros(500)
    end
end
```

### Temporarily Control the Lag Switch

```lua
function onExecute()
    local status = getLagSwitchStatus()
    if not status.available then
        log("Lag switch unavailable: " .. tostring(status.unsupportedReason))
        return
    end

    lagSwitch(true, {
        fakeLag = true,
        fakeLagDelayMs = 150,
        hardBlockInbound = false,
        hardBlockOutbound = false,
        targetMode = "roblox",
        useUdp = true,
        useTcp = false,
    })

    sleep(1000)
    lagSwitch(false)
end
```

Lag-switch overrides from scripts are temporary. They apply only while that script instance owns them and do not write back to the main app/profile settings.

### Read Saved App/Profile State

This is for advanced compatibility, not for normal per-script configuration:

```lua
function onExecute()
    local camfix = getSavedValue("camfixtoggle")
    local displayScale = getSavedValue("display_scale")
    log("camfixtoggle = " .. tostring(camfix))
    log("display_scale = " .. tostring(displayScale))
end
```

For new scripts, prefer script-local `settings.*` values created with `ui.*`. Use `getSavedValue()` only when you intentionally need compatibility with SMU's persisted app/profile state.

## API Reference

### Script Callbacks

| Callback | Required | Description |
| --- | --- | --- |
| `onExecute()` | yes | Main entry point when the script is started |
| `onSettings()` | no | Build the custom script settings UI |
| `onCleanup(reason)` | no | Cleanup hook after managed input is released and callbacks are cleared |

`onCleanup(reason)` receives one of:

- `"completed"`
- `"cancelled"`
- `"timeout"`
- `"error"`

### Utility

| Function | Description |
| --- | --- |
| `log(message)` | Write a message to the log console. Messages are truncated at 4096 bytes |
| `sleep(ms)` | Sleep for the specified number of milliseconds |
| `sleepMicros(us)` | Sleep for the specified number of microseconds, clamped to a 24-hour maximum. Uses a cancellable native high-precision wait path |
| `nowMicros()` | Return the current monotonic time in microseconds |
| `getSMUVersion()` | Return the current application version |
| `getPlatform()` | Return `windows`, `linux`, `macos`, or `unknown` |
| `getScriptHotkey()` | Return this script's current activation hotkey as a combined hotkey value, or `nil` if it is unbound |
| `getSavedValue(name)` | Return the current in-memory value of a saved app/profile setting, or `nil` if that key is not exposed |

For sub-millisecond pacing, prefer `sleepMicros()` over a Lua `nowMicros()` busy-wait. Busy-wait loops consume execution budget and can still trip the watchdog.

### Execution Control

| Function | Description |
| --- | --- |
| `isCancelled()` | Return whether the script has been stopped by the host app, timeout, or hotkey cancel request |
| `sleepUntilCancelled(ms)` | Wait up to `ms` milliseconds and return `true` if the script was stopped before the wait finished |
| `throwIfCancelled()` | Throw the same stop error used by interruptible runtime calls |
| `checkpoint()` | Cooperatively yield to the host, check for cancellation/timeout, and reset the active-execution watchdog window |
| `shouldYield(thresholdMicros)` | Return `true` when the current uninterrupted Lua slice has run for at least `thresholdMicros`. Defaults to `1000` microseconds |

Example:

```lua
while not isCancelled() do
    if sleepUntilCancelled(250) then
        break
    end
    throwIfCancelled()
    log("still running")
end
```

### Script Settings UI

The `ui` table is available to scripts that define `onSettings()`. These helpers define persistent per-script UI state and render controls in the selected-script panel.

| Function | Description |
| --- | --- |
| `ui.text(text, width)` | Render static wrapped text |
| `ui.separator(spacing)` | Render a separator line, then add optional vertical spacing |
| `ui.checkbox(id, label, defaultValue, width)` | Render a checkbox and persist a boolean setting |
| `ui.sliderInt(id, label, defaultValue, minValue, maxValue, width)` | Render an integer slider and persist the value |
| `ui.sliderFloat(id, label, defaultValue, minValue, maxValue, width)` | Render a float slider and persist the value |
| `ui.textbox(id, label, defaultValue, width, height)` | Render a text box and persist the value |
| `ui.dynamicTextbox(id, label, defaultValue, width, height)` | Render a read-only multi-line text box backed by a script-updated value |
| `ui.setDynamicText(id, text)` | Update a dynamic text-box value. Clamped to 4096 characters |
| `ui.keybind(id, label, defaultValue, width)` | Render the standard SMU keybind picker and persist the combo as a hotkey value. Kept for compatibility |
| `ui.hotkey(id, label, defaultValue, width)` | Semantic alias for input trigger hotkeys such as `F7` or `RMB` |
| `ui.keyCombo(id, label, defaultValue, width)` | Semantic alias for output combos that scripts may hold or release, such as `Ctrl+W` |
| `ui.button(id, label, width, height)` | Render a button and return `true` only on the frame it is pressed |
| `ui.button(id, label, callback, width, height)` | Render a button and call `callback(id)` when pressed. `callback` may be a Lua function or a dot-separated function path such as `"actions.reset"` |

Notes:

- Current UI values are mirrored into the global `settings` table so `onExecute()` can read them directly.
- Assignments to supported `settings` values from `onSettings()` are synced back to the script UI state after the callback finishes.
- Buttons are not persisted.
- `ui.setDynamicText()` may be called from `onExecute()` and `onCleanup(reason)`.
- Dynamic textbox values are saved with imported script UI state. After restarting SMU, the last dynamic text may still be visible until the script writes a new value.
- UI IDs must be non-empty, contain no embedded NUL bytes, and are limited to 128 bytes.
- A single `onSettings()` call may create up to 512 UI controls, and each script is capped at 4096 total unique UI IDs.
- Stored script UI strings are clamped to 4096 bytes.

Example:

```lua
actions = {}

function actions.clearStatus(id)
    ui.setDynamicText("status", "Cleared by " .. id)
end

function actions.resetSpeed()
    settings.speed = 50
end

function onSettings()
    ui.dynamicTextbox("status", "Status", "Ready", 360, 90)
    ui.sliderInt("speed", "Speed", 50, 0, 100, 260)
    ui.button("reset-speed", "Reset Speed", "actions.resetSpeed", 140, 0)

    if ui.button("mark-ready", "Mark Ready", 140, 0) then
        ui.setDynamicText("status", "Ready")
    end

    ui.button("clear-status", "Clear Status", "actions.clearStatus", 140, 0)
end
```

### Input

#### Single Keys vs Hotkeys

The input API accepts two related but different formats:

| Argument kind | Used by | Meaning |
| --- | --- | --- |
| Single key | `pressKey()`, `holdKey()`, `releaseKey()`, `isKeyPressed()` | One physical keyboard key or mouse button |
| Hotkey | `isHotkeyPressed()`, `input.*`, `ui.hotkey()`, `ui.keyCombo()`, `ui.keybind()`, `@keybind:` | A key combination, usually modifiers plus a main key |

Examples:

```lua
pressKey("Space")
holdKey("LCtrl")
releaseKey("LCtrl")

if isKeyPressed("E") then
    log("E is pressed")
end

if isHotkeyPressed("LCtrl+K") then
    log("LCtrl+K is pressed")
end
```

Do not pass combos to single-key functions:

```lua
-- Wrong:
pressKey("LCtrl+K")

-- Correct:
holdKey("LCtrl")
pressKey("K")
releaseKey("LCtrl")
```

Use `typeText()` for ordinary text entry:

```lua
typeText("hello")
```

Key names are case-insensitive. Spaces, underscores, and dashes are ignored, so these are equivalent:

```lua
pressKey("PageDown")
pressKey("page down")
pressKey("page_down")
pressKey("page-down")
```

Numeric SMU key codes are also accepted, but named keys are preferred.

#### Supported Key Names

##### Letters

| Key names |
| --- |
| `A` through `Z` |

##### Number Row

| Key names |
| --- |
| `0` through `9` |

##### Function Keys

| Key names |
| --- |
| `F1` through `F24` |

##### Numpad Keys

| Key names |
| --- |
| `Numpad0` through `Numpad9` |

##### Mouse Buttons

| Key name | Aliases |
| --- | --- |
| `LMB` | `MouseLeft`, `LeftMouse` |
| `RMB` | `MouseRight`, `RightMouse` |
| `MMB` | `MouseMiddle`, `MiddleMouse` |
| `Mouse4` | `XButton1` |
| `Mouse5` | `XButton2` |
| `MouseWheelUp` | `WheelUp` |
| `MouseWheelDown` | `WheelDown` |

##### Modifier Keys

| Key name | Aliases |
| --- | --- |
| `Shift` | |
| `LShift` | `LeftShift` |
| `RShift` | `RightShift` |
| `Ctrl` | `Control` |
| `LCtrl` | `LeftCtrl` |
| `RCtrl` | `RightCtrl` |
| `Alt` | |
| `LAlt` | `LeftAlt` |
| `RAlt` | `RightAlt` |
| `Win` | `Super`, `Meta` |
| `LWin` | `LeftWin` |
| `RWin` | `RightWin` |

##### Navigation and Editing Keys

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

##### Punctuation Keys

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

##### Lock Keys

| Key name |
| --- |
| `CapsLock` |
| `NumLock` |
| `ScrollLock` |

#### Hotkey Strings

Hotkey strings combine modifiers and one main key with `+`:

```lua
isHotkeyPressed("Ctrl+F")
isHotkeyPressed("Alt+Shift+X")
isHotkeyPressed("LCtrl+K")
```

Side-specific modifier names normalize to the generic modifier bit in the stored hotkey value, so `LCtrl+K`, `RCtrl+K`, `LeftCtrl+K`, and `Ctrl+K` all match generic Ctrl plus `K`.

By default, script hotkeys use loose matching. A loose `S` match is true while `Ctrl+S` is held, and a loose `Ctrl+S` match is true while `Alt+Ctrl+S` is held. Strict matching requires the exact generic modifier set.

```lua
if isHotkeyPressed("S", "strict") then
    log("Only S is held")
end

input.setHotkeyMode("strict")

if input.isPressed("Ctrl+S") then
    log("Exactly Ctrl+S is held")
end

if input.isPressed("Ctrl+S", { strict = false }) then
    log("Ctrl+S is held, extra modifiers allowed")
end
```

Use `ui.hotkey()` for user input triggers and `ui.keyCombo()` for output combos a script sends or holds.

#### Input Functions

| Function | Description |
| --- | --- |
| `pressKey(key, delay)` | Press and release a key or mouse-related action. `delay` defaults to `50` ms |
| `clickMouse(key, delay)` | Alias for `pressKey()` that only accepts mouse buttons and mouse wheel actions |
| `holdKey(key)` | Hold a single key down |
| `releaseKey(key)` | Release a held single key |
| `isKeyPressed(key)` | Return whether a single key is currently pressed |
| `isHotkeyPressed(hotkey, options)` | Return whether a hotkey combo is currently pressed. `options` may be `true`, `false`, `"strict"`, `"loose"`, or `{ strict = true/false }` |
| `typeText(text, delay)` | Type text with an optional per-character delay. `delay` defaults to `30` ms |
| `mouseWheel(delta)` | Scroll the mouse wheel |

`typeText()` input is capped at 4096 characters per call.

#### Managed Hotkey API

| Function | Description |
| --- | --- |
| `input.hold(hotkey)` | Managed-hold a hotkey combo |
| `input.release(hotkey)` | Release a managed-held hotkey combo |
| `input.setHeld(hotkey, down)` | Set a managed hotkey held state and return the resulting held state |
| `input.toggleHeld(hotkey)` | Toggle a managed hotkey held state and return the resulting held state |
| `input.releaseAllManaged()` | Release all managed hotkey holds owned by this script |
| `input.isPressed(hotkey, options)` | Alias for hotkey state checks |
| `input.setHotkeyMode(mode)` | Set the script's default hotkey match mode to `"loose"` or `"strict"` |
| `input.getHotkeyMode()` | Return the current default hotkey match mode |
| `input.onPressed(hotkey, callback, options)` | Register a callback for the up-to-down edge |
| `input.onReleased(hotkey, callback, options)` | Register a callback for the down-to-up edge |
| `input.onChanged(hotkey, callback, options)` | Register a callback for either edge. Callback receives `(isDown, hotkey)` |
| `input.listenUntilCancelled(scanMs)` | Run registered callbacks until the script stops |
| `input.mirror(sourceHotkey, targetHotkey, options)` | Mirror a source hotkey state to a managed target hold |

Notes:

- Managed holds are reference-counted per physical key.
- Managed holds are released automatically when the script completes, errors, times out, or is cancelled.
- Mouse wheel values such as `MouseWheelUp` and `MouseWheelDown` are rejected for managed holds.
- `input.listenUntilCancelled(scanMs)` defaults to `2` ms and clamps `scanMs` from `1` to `50`.
- Input callbacks are script-scoped. They only run while the script is actively executing inside `input.listenUntilCancelled()`.
- The first scan initializes callback state without firing callbacks.
- If a callback errors, listening stops and the script exits as an error.
- Mirroring onto the script's own activation hotkey is blocked unless `allowScriptHotkeyTarget = true` is explicitly passed.

#### Mouse and Pixel Functions

| Function | Description |
| --- | --- |
| `moveMouse(dx, dy)` | Move the mouse relative to its current position. In `"raw"` mode on Windows, relative movement is multiplied by the saved `display_scale` percentage before being sent |
| `moveMouseAbs(x, y, mode)` | Move the mouse to an absolute position on the monitor containing the cursor |
| `setMouseMotionMode(mode)` | Set Lua mouse motion mode to `"raw"` or `"absolute"` |
| `getMouseMotionMode()` | Return the current Lua mouse motion mode |
| `setMacOSCursorMovement(enabled)` | macOS only: set the global Quartz relative movement mode. Returns `true` on macOS and `false` elsewhere |
| `getMacOSCursorMovement()` | macOS only: return whether global Quartz relative movement moves the visible cursor. Returns `false` elsewhere |
| `getPixelColor(x, y, mode, format)` | Return the pixel color at a position. `format` defaults to `"hex"` and may be `"rgb"` |
| `getPixelRect(x1, y1, x2, y2, mode, format)` | Return a row-major 2D table of pixels covering the rectangle between two points |
| `moveDegrees(dx, dy)` | Move the mouse using degree units derived from saved Roblox sensitivity and Cam-Fix settings. Positive `dy` moves upward |

Coordinate modes for `moveMouseAbs()`, `getPixelColor()`, and `getPixelRect()`:

- `"pixels"`
- `"percent"`
- `"scaled720p"`
- `"scaled1080p"`
- `"scaled1440p"`
- `"scaled2160p"`

Examples:

```lua
moveMouseAbs(960, 540)
moveMouseAbs(50, 50, "percent")
moveMouseAbs(960, 540, "scaled1080p")

local color = getPixelColor(50, 50, "percent")
if color == "#FF0000" then
    log("center pixel is red")
end

local rgb = getPixelColor(50, 50, "percent", "rgb")
if rgb.r == 255 and rgb.g == 0 and rgb.b == 0 then
    log("center pixel is red, in RGB")
end

local block = getPixelRect(10, 10, 12, 12, "pixels", "rgb")
log("sampled " .. tostring(#block) .. " rows of pixels")
```

Behavior notes:

- Lua scripts start in `"raw"` mouse motion mode by default.
- In `"absolute"` mode, `moveMouse()` uses platform absolute-pointer APIs instead of a raw relative path.
- On macOS, `setMacOSCursorMovement(false)` keeps raw relative movement game-safe by sending Quartz delta fields without moving the visible cursor; `true` makes raw relative movement move the visible cursor for desktop automation.
- `moveMouseAbs()`, `getPixelColor()`, and `getPixelRect()` target the monitor containing the current cursor, not the full virtual desktop.
- In `"pixels"` mode, `(0, 0)` is the top-left of the active monitor.
- In `"percent"` mode, `x` and `y` must be between `0` and `100`.
- In scaled modes, the coordinate pair is authored against the named base resolution and then scaled to the active monitor size.
- `moveDegrees()` caches its conversion settings once when the script instance starts.
- `moveDegrees()` is lossy because the final movement is rounded to whole pixels before it is sent.

### Process and Network Control

| Function | Description |
| --- | --- |
| `freeze(enable)` | Freeze or unfreeze the target process |
| `robloxFreeze(enable)` | Alias for `freeze(enable)` |
| `roblox_freeze(enable)` | Alias for `freeze(enable)` |
| `lagSwitch(enable, options)` | Toggle lag-switch behavior, optionally applying a temporary config table first |
| `lagswitch(enable)` | Alias for `lagSwitch(enable)` |
| `getLagSwitchConfig()` | Return the effective lag-switch config currently used by scripts/backend |
| `setLagSwitchConfig(options)` | Apply a temporary script-owned lag-switch config override |
| `getLagSwitchStatus()` | Return lag-switch availability, active state, target mode, and unsupported reason |
| `clearLagSwitchConfig()` | Clear this script's config override without changing saved app/profile settings |

Lag-switch config keys:

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
| `preventDisconnect` | boolean | Use Roblox disconnect-prevention behavior while hard-blocking |
| `autoUnblock` | boolean | Keep the setting in the effective config for compatibility with built-in behavior |
| `maxDurationSeconds` | number | Auto-unblock duration in seconds |
| `unblockDurationMs` | integer | Auto-unblock release duration in milliseconds |
| `remoteIps` | table | Custom-mode IPv4 address list, exact-match only |
| `remotePorts` | table | Custom-mode remote TCP/UDP port list |
| `includeRobloxDynamicIps` | boolean | In custom mode, also include the Roblox static range and discovered Roblox server IPs |

Notes:

- Lag-switch config overrides are temporary.
- If multiple scripts control lag-switch settings at once, the most recent script-owned override wins.
- A script can only clear its own current override.
- Custom targeting is intentionally limited to validated IP and port filters. Lua does not expose raw WinDivert handles, packet bytes, packet receive/send functions, or arbitrary packet injection.

## Security / Sandbox

Only import scripts you trust. Lua scripts can simulate input and control process- and network-related macro behavior.

### Memory and Execution Limits

- Each script runs with a per-script Lua memory cap.
- The default cap is `64` MiB.
- Scripts can request a different cap with `-- @memoryLimitMB:`, clamped from `16` to `256`.
- `onExecute()` and script-load calls are protected by an active-execution watchdog.
- A script that continuously runs Lua code without yielding, sleeping, or checkpointing will time out.
- `onSettings()` has its own 5-second hard timeout.
- Timeout and cancellation errors cannot be suppressed with `pcall()` or `xpcall()`.

Cooperative scripts can run for a long time if they periodically call `checkpoint()`, `sleep()`, `sleepMicros()`, `sleepUntilCancelled()`, or host APIs that naturally wait.

### Available Lua Standard Libraries

The sandbox opens:

- `base`
- `table`
- `string`
- `math`
- `utf8`
- `coroutine`

Coroutines are fully supported, including `coroutine.resume()`, `coroutine.wrap()`, and yielding through `sleep()` or `sleepMicros()` from a coroutine.

The sandbox does not open:

- `os`
- `io`
- `package`
- `debug`

It also removes:

- `dofile`
- `loadfile`
- `load`
- `collectgarbage`

### Cancellation and Cleanup

- A running script can be stopped by pressing the same activation hotkey again or by using Force Stop in the app.
- Scripts that poll `isCancelled()`, `sleepUntilCancelled()`, or `throwIfCancelled()` can exit cooperatively.
- Managed held hotkeys are released before `onCleanup(reason)` runs.
- Input callbacks are cleared before `onCleanup(reason)` runs.

Cleanup may:

- log
- update dynamic text
- release low-level keys
- release managed input
- disable freeze
- disable lag switch
- clear lag-switch config

Cleanup may not start new actions such as:

- `holdKey()`
- `pressKey()`
- `typeText()`
- `moveMouse()`
- `moveMouseAbs()`
- `moveDegrees()`
- `mouseWheel()`
- `freeze(true)`
- `lagSwitch(true)`
- `input.hold()`
- `input.setHeld(hotkey, true)`
- `input.toggleHeld()`
- input callback registration
- `input.listenUntilCancelled()`

### `onSettings()` Restrictions

`onSettings()` is for layout and persistent script UI state. Input, mouse, process, timing, checkpoint, and lag-switch APIs are blocked while `onSettings()` is running. Use `ui.*` there, and keep runtime work in `onExecute()`.

## Platform Compatibility

`getPlatform()` returns `windows`, `linux`, `macos`, or `unknown`.

### Keyboard and Relative Mouse Input

Keyboard injection and relative mouse motion are available on Windows and Linux. On Linux, relative input continues to work in native Wayland sessions because it uses the existing low-level input path rather than global desktop coordinate APIs. On macOS, synthetic keyboard and mouse calls, global `isKeyPressed()`, and hotkey reads require Accessibility permission. macOS raw relative movement defaults to Quartz delta-only input for locked-camera games; use `setMacOSCursorMovement(true)` or the app checkbox when desktop cursor movement is required.

### Absolute Mouse Coordinates

Use `setMouseMotionMode("absolute")` when you want desktop-style pointer targeting.

- Windows uses normalized absolute `SendInput` coordinates.
- Linux/X11 uses X11 absolute pointer warping.
- Native Wayland sessions without usable X11 access cannot provide this global absolute-pointer path.
- macOS uses CoreGraphics cursor coordinates and requires Accessibility permission for global pointer movement.

When absolute targeting is unavailable on Linux, script errors explain why, for example that the current session lacks usable X11/XWayland cursor-position access.

### Pixel Reads

`getPixelColor()` and `getPixelRect()` read from the active monitor containing the cursor.

- Windows reuses a cached monitor frame when possible and refreshes that cache roughly once per monitor refresh interval. Repeated polling is efficient and suitable for high-frequency color checks and moderate real-time scanning.
- Linux currently uses an X11/XWayland screen-read path.
- Native Wayland sessions without usable X11 access cannot currently provide arbitrary global screen reads.
- macOS uses Screen Recording permission and samples the same active-monitor coordinate space used by `moveMouseAbs()`.

### Freeze and Foreground Detection

Freeze is available on Windows and Linux. On Linux, native Wayland sessions cannot reliably inspect other apps' active windows, so foreground-detection-dependent behavior may fall back to an always-active mode instead of an exact active-window restriction. On macOS, foreground detection uses the frontmost app and freeze uses process stop/continue signals; the signal operation still depends on normal OS process permissions.

### Lag Switch

Lag switch is currently implemented on Windows. The Lua API still exists on Linux and macOS, but `getLagSwitchStatus()` reports that the native lag-switch backend is unavailable there.

## Legacy / Saved App Settings Reference

> Advanced compatibility section. `getSavedValue()` exposes exact app/profile state names that SMU currently persists in memory and to disk. Many of these names are legacy or internal compatibility names such as `vk_f5`, `wallhopcamfix`, and `RobloxPixelValueChar`.
>
> For new scripts, do not treat this list as the clean public API. Prefer script-local `settings.*` values and dedicated Lua functions first.

`getSavedValue(name)` reads only the saved settings registered by the app. The name must match exactly. It returns a Lua boolean, number, string, or `nil` if the key is not exposed. Save-file settings are read-only from scripts.

Examples:

```lua
local camfixEnabled = getSavedValue("camfixtoggle")
local camfixMode = getSavedValue("wallhopcamfix")
local jumpKey = getSavedValue("vk_f5")
local wallhopPixels = getSavedValue("WallhopPixels")
```

If a key is not part of the persisted setting registry, `getSavedValue()` returns `nil`.

### Boolean Values

| Name | Type | Description |
| --- | --- | --- |
| `macrotoggled` | boolean | Master macro enable toggle. Anti-AFK is controlled separately |
| `shiftswitch` | boolean | Unused legacy saved flag; persisted for compatibility but not read by the runtime |
| `wallhopswitch` | boolean | First Wallhop/Rotation instance: use left-flick direction instead of right-flick direction |
| `wallhopcamfix` | boolean | Legacy first Wallhop/Rotation cam-fix flag. Saved for compatibility; active calculations use global `camfixtoggle` |
| `unequiptoggle` | boolean | Item Unequip COM Offset: keep the selected item equipped after the emote/message instead of unequipping it |
| `isspeedswitch` | boolean | Speedglitch mode flag: `false` means toggle mode, `true` means hold-key mode |
| `isfreezeswitch` | boolean | Freeze mode flag: `false` means hold-to-freeze, `true` means press-to-toggle |
| `iswallwalkswitch` | boolean | Wall-Walk mode flag: `false` means toggle mode, `true` means hold-key mode |
| `isspamswitch` | boolean | First Spam a Key instance: `false` means toggle mode, `true` means hold-key mode |
| `isitemclipswitch` | boolean | Item Clip mode flag: `false` means toggle mode, `true` means hold-key mode |
| `autotoggle` | boolean | Wall Helicopter High Jump: automatically hold the configured Auto-HHJ keys before the freeze sequence |
| `toggle_jump` | boolean | First Wallhop/Rotation instance: press the configured wallhop jump key during the wallhop |
| `toggle_flick` | boolean | First Wallhop/Rotation instance: perform the flick-back movement after the initial flick |
| `camfixtoggle` | boolean | Global Game Uses Cam-Fix setting; changes sensitivity-derived pixel calculations for Speedglitch, Wallhop, Wall-Walk, and Ledge Bounce |
| `macos_cursor_movement` | boolean | macOS-only Quartz relative mouse setting: `false` sends game-safe delta input, `true` moves the visible cursor |
| `wallwalktoggleside` | boolean | Wall-Walk: use the left-flick side instead of the default side |
| `antiafktoggle` | boolean | Enable the Anti-AFK timer/key press routine |
| `fasthhj` | boolean | Wall Helicopter High Jump: decrease the default freeze duration in speedrunner mode |
| `globalzoomin` | boolean | Use mouse-wheel zoom instead of the configured shiftlock key for HHJ-style macros |
| `globalzoominreverse` | boolean | Reverse the mouse-wheel direction used when `globalzoomin` is enabled |
| `wallesslhjswitch` | boolean | Walless LHJ: use the left-sided setup key instead of the default right-sided setup key |
| `chatoverride` | boolean | Force the chat-open key to `/` instead of relying on the custom `vk_chatkey` setting |
| `bounceautohold` | boolean | Ledge Bounce: automatically hold the movement key after the bounce setup |
| `bouncerealignsideways` | boolean | Ledge Bounce: use the horizontal realignment branch after the bounce |
| `bouncesidetoggle` | boolean | Ledge Bounce: use the left-sided bounce path instead of the default side |
| `laughmoveswitch` | boolean | Laugh Clip: disable the automatic `S` key hold during the clip sequence |
| `freezeoutsideroblox` | boolean | Legacy compatibility mirror for Freeze foreground restriction. `true` means Freeze is allowed outside Roblox |
| `takeallprocessids` | boolean | Freeze/process control: target every matching process ID instead of only the newest/main matching process |
| `ontoptoggle` | boolean | Keep the main SMU window always on top |
| `bunnyhopsmart` | boolean | Smart Bunnyhop: temporarily suppress bunnyhop while chat is open until Enter or left-click closes it |
| `presskeyinroblox` | boolean | First Press a Button instance: restrict the macro to Roblox foreground focus when enabled |
| `unequipinroblox` | boolean | Item Unequip COM Offset: restrict the macro to Roblox foreground focus when enabled |
| `doublepressafkkey` | boolean | Anti-AFK: press the configured Anti-AFK key twice per run |
| `useoldpaste` | boolean | Use the legacy Unicode/chat typing path for pasted chat text |
| `floorbouncehhj` | boolean | Floor Bounce: run the optional HHJ-style shiftlock/helicoptering sequence after the floor bounce |
| `HHJFreezeDelayApply` | boolean | Wall Helicopter High Jump: apply `HHJFreezeDelayOverride` instead of the default freeze-delay timing |
| `islagswitchswitch` | boolean | Lag Switch mode flag: `false` means hold-key mode, `true` means press-to-toggle |
| `prevent_disconnect` | boolean | Lag Switch: use the disconnect-prevention behavior while blocking/delaying Roblox traffic |
| `lagswitchoutbound` | boolean | Lag Switch: hard-block outbound/upload packets |
| `lagswitchinbound` | boolean | Lag Switch: hard-block inbound/download packets |
| `lagswitchtargetroblox` | boolean | Lag Switch: restrict filtering to Roblox traffic instead of all matching traffic |
| `lagswitchlaginbound` | boolean | Fake Lag: delay inbound/download packets when fake-lag mode is enabled |
| `lagswitchlagoutbound` | boolean | Fake Lag: delay outbound/upload packets when fake-lag mode is enabled |
| `lagswitchlag` | boolean | Lag Switch: enable fake-lag packet delay mode instead of only hard-blocking |
| `lagswitchusetcp` | boolean | Lag Switch: include TCP traffic in addition to UDP traffic |
| `lagswitch_autounblock` | boolean | Lag Switch: automatically stop lagging after `lagswitch_max_duration` seconds |
| `show_lag_overlay` | boolean | Show the Windows lag-switch status overlay |
| `overlay_hide_inactive` | boolean | Hide the lag-switch overlay when the lag switch is not actively blocking or delaying traffic |
| `overlay_use_bg` | boolean | Draw a background behind the lag-switch overlay text |

### Numeric Values

| Name | Type | Description |
| --- | --- | --- |
| `vk_mbutton` | number | Freeze macro trigger hotkey. Legacy name reflects the default middle-mouse binding |
| `vk_f5` | number | Item Desync macro trigger hotkey. Legacy name reflects the default F5 binding |
| `vk_xbutton1` | number | Wall Helicopter High Jump trigger hotkey. Legacy name reflects the default mouse XButton1 binding |
| `vk_xkey` | number | Speedglitch trigger hotkey. Legacy name reflects the default X binding |
| `vk_f8` | number | Item Unequip COM Offset trigger hotkey. Legacy name reflects the default F8 binding |
| `vk_zkey` | number | First Press a Button instance trigger hotkey. Legacy name reflects the default Z binding |
| `vk_dkey` | number | First Press a Button instance output key: the key the macro presses after the trigger |
| `vk_xbutton2` | number | First Wallhop/Rotation instance trigger hotkey. Legacy name reflects the default mouse XButton2 binding |
| `vk_wallhopjumpkey` | number | First Wallhop/Rotation instance jump key used during the wallhop sequence |
| `vk_f6` | number | Walless LHJ trigger hotkey. Legacy name reflects the default F6 binding |
| `vk_clipkey` | number | Item Clip trigger hotkey |
| `vk_laughkey` | number | Laugh Clip trigger hotkey |
| `vk_wallkey` | number | Wall-Walk trigger hotkey |
| `vk_leftbracket` | number | First Spam a Key instance trigger hotkey. Legacy name reflects the default `[` binding |
| `vk_spamkey` | number | First Spam a Key instance output key: the key repeatedly pressed while spam is active |
| `vk_bouncekey` | number | Ledge Bounce trigger hotkey |
| `vk_bunnyhopkey` | number | Smart Bunnyhop trigger/output hotkey |
| `vk_floorbouncekey` | number | Floor Bounce trigger hotkey |
| `vk_lagswitchkey` | number | Lag Switch trigger hotkey |
| `vk_shiftkey` | number | Custom shiftlock key used by HHJ-style macros when `globalzoomin` is disabled |
| `vk_chatkey` | number | Chat-open key used by chat/emote macros and by Smart Bunnyhop chat suppression logic |
| `vk_enterkey` | number | Key used to submit chat messages/emotes after paste/type operations |
| `vk_afkkey` | number | Anti-AFK key that the Anti-AFK routine presses |
| `vk_autohhjkey1` | number | Wall Helicopter High Jump Auto-HHJ first key to hold before the freeze sequence |
| `vk_autohhjkey2` | number | Wall Helicopter High Jump Auto-HHJ second key to hold before the freeze sequence |
| `selected_section` | number | Currently selected macro section index in the UI |
| `selected_dropdown` | number | Selected Item Unequip COM Offset emote dropdown index |
| `PreviousWallWalkSide` | number | Unused legacy wall-walk side cache; persisted for compatibility but not read by the runtime |
| `selected_wallhop_instance` | number | Currently selected Wallhop/Rotation instance index in the UI |
| `speed_slot` | number | Item Unequip COM Offset gear slot used after the emote/message sequence |
| `desync_slot` | number | Item Desync gear slot repeatedly equipped/released while the trigger is held |
| `clip_slot` | number | Item Clip gear slot repeatedly equipped/released while item clip is active |
| `spam_delay` | number | First Spam a Key instance user-facing spam delay in milliseconds |
| `real_delay` | number | First Spam a Key instance half-cycle delay actually used between output-key press/release operations |
| `wallhop_dx` | number | First Wallhop/Rotation instance initial horizontal mouse flick amount |
| `wallhop_dy` | number | First Wallhop/Rotation instance horizontal flick-back amount |
| `wallhop_vertical` | number | First Wallhop/Rotation instance vertical mouse movement paired with the flick |
| `PreviousWallWalkValue` | number | Cached Roblox sensitivity value last used to recalculate Wall-Walk pixel movement |
| `maxfreezetime` | number | Freeze auto-unfreeze timeout in seconds |
| `maxfreezeoverride` | number | Freeze refreeze delay in milliseconds after an automatic unfreeze |
| `RobloxWallWalkValueDelay` | number | Wall-Walk delay between the two flicks, stored as microseconds |
| `speed_strengthx` | number | Speedglitch/HHJ first horizontal mouse movement amount for the timed 180-degree turn |
| `speedoffsetx` | number | Unused legacy Speedglitch X offset; persisted for compatibility but not read by the runtime |
| `speed_strengthy` | number | Speedglitch/HHJ second horizontal mouse movement amount for the timed 180-degree turn |
| `speedoffsety` | number | Unused legacy Speedglitch Y offset; persisted for compatibility but not read by the runtime |
| `clip_delay` | number | Item Clip total equip/release cycle delay in milliseconds |
| `AutoHHJKey1Time` | number | Wall Helicopter High Jump Auto-HHJ first key hold time in milliseconds |
| `AutoHHJKey2Time` | number | Wall Helicopter High Jump Auto-HHJ second key hold time in milliseconds |
| `RobloxPixelValue` | number | Calculated Speedglitch/HHJ 180-degree turn pixel value derived from Roblox sensitivity and `camfixtoggle` |
| `PreviousSensValue` | number | Cached Roblox sensitivity value last used to recalculate sensitivity-derived pixel values |
| `windowOpacityPercent` | number | Main window opacity percentage |
| `AntiAFKTime` | number | Anti-AFK interval in minutes |
| `display_scale` | number | Windows mouse movement scale percentage used by raw relative movement paths |
| `WindowPosX` | number | Saved main window X position |
| `WindowPosY` | number | Saved main window Y position |
| `lagswitch_max_duration` | number | Lag Switch auto-unlag timeout in seconds |
| `lagswitch_unblock_ms` | number | Lag Switch auto-unblock/unlag duration in milliseconds |
| `lagswitchlagdelay` | number | Fake Lag packet delay amount in milliseconds |
| `overlay_x` | number | Lag-switch overlay X position |
| `overlay_y` | number | Lag-switch overlay Y position |
| `overlay_size` | number | Lag-switch overlay text size |
| `overlay_bg_r` | number | Lag-switch overlay background red channel, normalized `0.0` to `1.0` |
| `overlay_bg_g` | number | Lag-switch overlay background green channel, normalized `0.0` to `1.0` |
| `overlay_bg_b` | number | Lag-switch overlay background blue channel, normalized `0.0` to `1.0` |
| `screen_width` | number | Saved or calculated application window width value |
| `screen_height` | number | Saved or calculated application window height value |
| `active_monitor_width` | number | Current monitor width in pixels for the monitor containing the cursor. Transient; not saved to disk |
| `active_monitor_height` | number | Current monitor height in pixels for the monitor containing the cursor. Transient; not saved to disk |
| `active_monitor_hz` | number | Current monitor refresh rate in Hz for the monitor containing the cursor. Transient; not saved to disk |

### String Values

| Name | Type | Description |
| --- | --- | --- |
| `settingsBuffer` | string | Target Roblox executable/process-name text, or PID list text on Linux/Wine paths |
| `ItemDesyncSlot` | string | Text backing the Item Desync gear-slot input |
| `ItemSpeedSlot` | string | Text backing the Item Unequip COM Offset gear-slot input |
| `ItemClipSlot` | string | Text backing the Item Clip gear-slot input |
| `ItemClipDelay` | string | Text backing the Item Clip delay input |
| `BunnyHopDelayChar` | string | Text backing the Smart Bunnyhop delay input in milliseconds |
| `RobloxSensValue` | string | Text backing the Roblox sensitivity input used for sensitivity-derived pixel calculations |
| `RobloxWallWalkValueChar` | string | Text backing the Wall-Walk pixel-value input |
| `RobloxWallWalkValueDelayChar` | string | Text backing the Wall-Walk delay-between-flicks input |
| `WallhopPixels` | string | First Wallhop/Rotation instance text backing the flick pixel amount |
| `WallhopVerticalChar` | string | First Wallhop/Rotation instance text backing vertical pixel movement |
| `SpamDelay` | string | First Spam a Key instance text backing the spam delay input |
| `RobloxPixelValueChar` | string | Text backing the Speedglitch/HHJ 180-degree turn pixel-value input |
| `CustomTextChar` | string | Custom Item Unequip COM Offset chat message. If non-empty, it disables gear equipping and only pastes or sends this text |
| `RobloxFPSChar` | string | Text backing the Roblox FPS input used for frame-delay calculations |
| `AntiAFKTimeChar` | string | Text backing the Anti-AFK interval input |
| `WallhopDelayChar` | string | First Wallhop/Rotation instance text backing wallhop length in milliseconds |
| `WallhopBonusDelayChar` | string | First Wallhop/Rotation instance text backing bonus delay before jumping, in milliseconds |
| `PressKeyDelayChar` | string | First Press a Button instance text backing output-key hold length in milliseconds |
| `PressKeyBonusDelayChar` | string | First Press a Button instance text backing delay before pressing the output key, in milliseconds |
| `PasteDelayChar` | string | Text backing the delay between chat typing or paste key events in milliseconds |
| `HHJLengthChar` | string | Text backing HHJ flick/helicopter duration in milliseconds |
| `HHJFreezeDelayOverrideChar` | string | Text backing the optional HHJ freeze-delay override in milliseconds |
| `HHJDelay1Char` | string | Text backing HHJ delay after unfreezing and before shiftlock/zoom is held, in milliseconds |
| `HHJDelay2Char` | string | Text backing HHJ delay before helicoptering/spinning starts, in milliseconds |
| `HHJDelay3Char` | string | Text backing HHJ delay while shiftlock/zoom remains held after spinning starts, in milliseconds |
| `AutoHHJKey1TimeChar` | string | Text backing Auto-HHJ first key hold time in milliseconds |
| `AutoHHJKey2TimeChar` | string | Text backing Auto-HHJ second key hold time in milliseconds |
| `FloorBounceDelay1Char` | string | Text backing Floor Bounce HHJ delay after unfreezing and before shiftlocking, in milliseconds |
| `FloorBounceDelay2Char` | string | Text backing Floor Bounce HHJ delay before helicoptering, in milliseconds |
| `FloorBounceDelay3Char` | string | Text backing Floor Bounce HHJ helicoptering duration in milliseconds |
| `text` | string | Selected Item Unequip COM Offset emote text from the dropdown |

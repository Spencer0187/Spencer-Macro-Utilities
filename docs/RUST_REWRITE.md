# Rust rewrite plan

The Rust rewrite is developed on `rewrite/rust`. The existing C++ application remains in this
branch until parity is proven and is treated as the behavioral oracle, not as an implementation
template.

## Non-negotiable compatibility targets

The replacement must preserve observable SMU behavior unless a deliberate compatibility change is
separately approved:

- Dear ImGui layout, styling, interaction, and script-generated controls.
- Existing `SMCSettings.json` and legacy settings behavior, including recovery/migration paths.
- Hotkey encoding and ordering.
- Built-in macro action order, timing, cancellation, and cleanup.
- Lua API semantics, resource limits, cancellation, and managed-input cleanup.
- Roblox process/log behavior.
- Platform capabilities on Windows, Linux X11/Wayland, and macOS.
- Updater and packaging behavior.

The rewrite may improve internals aggressively. Compatibility tests decide whether a change is safe.

## Migration order

1. Capture behavior from C++ as fixtures, traces, and UI screenshots.
2. Establish strict Rust workspace/tooling and compatibility utilities.
3. Prove SDL3 + Dear ImGui + OpenGL3 rendering in Rust with a representative UI slice.
4. Port deterministic foundations: key/hotkey types and profile/config compatibility.
5. Introduce process, Roblox, input, display/capture, and network services with fake backends.
6. Build the macro runtime against fakes and timing contracts before using native backends.
7. Port built-in macros one vertical slice at a time, including their UI and parity tests.
8. Port scripting after the underlying services are stable.
9. Replace the C++ entry point only after the compatibility matrix is green.

## Current Rust service slices

- `smu-process` owns process discovery/selection plus Linux process-control and foreground
  capabilities. Process control covers cgroup-v2 child freezing for safe app cgroups, ptrace-aware
  SIGSTOP refusal, process-tree SIGSTOP/SIGCONT fallback ordering, and the existing root
  `processhelper` socket protocol behind an injected authorization gate. Foreground detection keeps
  Wayland/unknown sessions in the C++ false path, reads `_NET_ACTIVE_WINDOW` / `_NET_WM_PID` through
  X11 when available, and reconciles namespace PIDs against target cgroups for Sober. `smu-scripting`
  wires the three legacy freeze globals through these capabilities with frozen-PID snapshots. On
  Linux, `smu-app` now also owns the C++-style two-minute authorization gate and Additional Freeze
  Permission Required modal; approval stages the bundled `processhelper`, launches it through
  graphical `pkexec`, verifies the root peer, and wakes the blocked process-control caller. The live
  Rust Freeze slice now composes this service end-to-end: the app owns the Freeze pane/sidebar state,
  one long-lived controller lane, one-second process refresh, 25 ms X11 foreground caching, main-vs-all
  PID selection, and the characterized partial-success suspend/resume state machine.
- `smu-input` owns separate physical-state and injection contracts, C++-oracle-tested loose/strict
  hotkey matching, transient-input boundaries, and the historical text-entry fallback mapping.
  Cursor/screen/pixel APIs are intentionally excluded; those belong to `smu-display`.
- `smu-input` now has a native Linux evdev/uinput backend: it creates the historical `SMU Native
  Linux Input` virtual device, passively reads physical keyboard/mouse event devices, keeps injected
  events out of physical state, preserves wheel binding sentinels and generic modifier collapse,
  supports X11 absolute pointer warps, and falls back to the C++ 12-second interactive device
  discovery path when capability probes do not identify both keyboard and mouse. The full VK/evdev
  mapping is differential-tested from a generated C++ oracle fixture. On native Wayland, the Linux
  input backend can share `smu-display`'s approved RemoteDesktop session for direct `moveMouseAbs`
  while correctly continuing to report global cursor-position reads as unavailable.
- `smu-display` owns monitor/cursor geometry, active-monitor refresh metadata, script coordinate
  modes, pixel colors/rectangle reads, and the screen-capture capability. Linux has native X11/XWayland
  cursor/Xinerama/root-window pixel support plus a native Wayland portal service using ScreenCast,
  RemoteDesktop, and PipeWire. The Wayland service keeps portal/PipeWire objects on an owned worker,
  converts BGRx/BGRA/RGBx/RGBA frames into immutable RGB snapshots, maps captured pixels back to the
  portal's logical monitor size for absolute pointer motion, and preserves the C++ activation-generation
  handoff: Lua workers wait while the app owns the approval modals. `smu-scripting` wires
  `moveMouse`, `moveMouseAbs`, `moveDegrees`, mouse motion mode, `getPixelColor`, `getPixelRect`, and
  the macOS cursor toggle through separate input/display services. `moveDegrees` caches its saved
  camfix/sensitivity calibration at API registration and is differential-tested against a C++
  oracle, including `strtod` hex-float acceptance and rounding/overflow errors.
- `smu-roblox` owns incremental Roblox player-log discovery/parser state plus a thread-safe log
  source capability. Its fixture suite is generated by the C++ `roblox-file` oracle, including state
  transitions, server metadata, numeric conversion quirks, truncation/rotation handling, and
  partial-line reads. `smu-scripting` exposes `readRobloxLog` / `robloxLog.read` plus ordered
  `robloxLog.onLine` callbacks and `listenUntilCancelled`; callback limits, reentrancy guards,
  cancellation, and a representative ECMAScript regex corpus are checked against the C++ path.
- `smu-updater` owns release/version contract validation, update-check orchestration, and
  cross-platform update-asset scoring. HTTP remains an injected transport and platform replacement
  remains separate, so untrusted metadata is validated before install-side effects are possible.
- `smu-runtime` includes the characterized Lag Switch controller step as a pure side-effect boundary.
  A generated 140-case C++ oracle corpus freezes section/foreground cleanup, hold/toggle edges,
  backend initialization failure, config-cache publication order, activation start-time resets, and
  float32 auto-unlag thresholds. `smu-app` now owns the first live built-in controller cutover on
  Linux: one long-lived 2 ms controller thread reads the native evdev state service, publishes UI
  config into `smu-network`, and drives the real helper transport. The cutover deliberately preserves
  C++'s separate `bWinDivertEnabled` compatibility flag: hotkey-triggered transport initialization can
  succeed while that UI-enabled flag stays false; manual UI/helper initialization sets it true.
- `smu-network` owns lag-switch config arbitration, the deterministic Linux helper command grammar,
  and the runtime control capability used by scripting. Linux now also has the real root-validated
  `nethelper` Unix-socket transport, applied-command dedup/error semantics, script/base ownership,
  and the one-second dynamic Roblox-target refresh worker; endpoint metadata comes from `smu-roblox`
  instead of duplicating log parsing. `smu-scripting` wires `lagSwitch` / `lagswitch`, config
  get/set/clear, and status through that service with the legacy field/range, owner-token, settings,
  and cleanup semantics. `smu-app` now owns the Linux lagswitch helper-launch boundary: it resolves
  the bundled helper with the same AppImage/current-directory precedence, stages a private 0700 copy,
  binds it to the GUI PID/start-time identity, launches it through graphical `pkexec`, verifies the
  root helper socket, and exposes the default C++ Administrator Required modal. Notification
  suppression/profile persistence for that modal remains part of the broader app-settings cutover.
- `smu-scripting` owns imported-script hotkey/metadata parsing, script-owned hotkey state, persisted
  UI/settings state, and the restricted Lua 5.4.8 VM lifecycle. The VM matches the C++ library
  surface, allocator cap, instruction-hook timeout/cancellation model, protected-call/coroutine stop
  wrappers, and foundational timing/cancellation functions. Service-backed globals now cover direct
  input/text/hotkeys, edge-triggered input callbacks/listeners and managed hotkey mirroring,
  display/pixels/movement, process freeze, lag switch, Roblox log reads/callbacks, `log`,
  `getPlatform`, `getSMUVersion`, and `getSavedValue`. The full legacy `ui` table is also modeled
  without ImGui in the scripting crate: persistent `settings` synchronization, control/ID budgets,
  capture commit/rollback, render-time edits, dynamic text, keybind aliases, and button callbacks
  are exposed through app-facing state/interaction queues. `LuaScriptInstance` now composes those
  adapters into one per-script VM, enforces text-only chunks and the `onExecute` gate, runs initial
  `onSettings`, maps completed/error/timeout/cancelled cleanup reasons, releases managed input and
  callbacks before `onCleanup`, clears lag-switch ownership afterward, and intentionally leaves a
  script freeze in place until instance shutdown just like C++. `ScriptManager` now owns imported
  record/path normalization, import-from-save quirks, reload/reset/remove, persistence, live hotkey
  and process-target handles, and named per-record execution workers. Different scripts can execute
  concurrently; the same record is protected by an internal active reservation, while public
  `running` is published only after the Lua budget/cancellation state is armed so Force Stop cannot
  be lost during worker startup. Render-mode `onSettings` uses the C++ try-lock/skip behavior. Saved
  app-setting scalars are exposed by `smu-profile`; transient active-monitor values stay in
  `smu-display`. Settings render/callback mode is treated as one restricted scripting phase,
  matching the C++ `IsSettingsCallbackActive` helper.

## Architecture rules

- UI code observes application state and sends commands; it does not call OS APIs directly.
- Platform implementations do not know about Dear ImGui or built-in macros.
- `Input` is not a synonym for display/capture. Monitor geometry and pixel capture are a separate
  capability.
- Platform selection and dependency wiring happen at the application composition root.
- No generic "run arbitrary worker" API is exposed to macro code. Threads have named ownership and
  cleanup semantics.
- Persist the existing settings format during the compatibility phase. A new schema is a later,
  separately tested change.

## Timing contract

SMU contains timing-sensitive behavior down to microseconds. Porting `sleep_for` calls mechanically
is not sufficient, but neither is replacing them with a permanent busy loop.

Timing work follows three layers:

1. **Logical trace tests** use a fake clock and assert exact requested delays/action ordering.
2. **Timing primitive benchmarks** measure real wake-up error on each supported OS.
3. **End-to-end macro tests** allow explicit platform-specific timing tolerances while still
   requiring exact action order and cleanup.

A high-precision wait may combine an efficient blocking sleep with a short final yield/spin phase.
Any spin threshold must be measured, bounded, cancellation-aware, and documented. The runtime
architecture remains open to either one deterministic scheduler or a small number of deliberate
workers until measurements prove which preserves current behavior best. After a controller stall has
been differentially characterized, it may be intentionally migrated to a named worker when doing so
removes unrelated macro interference; preserve its internal action/wait order and add a lane-topology
regression test. HHJ motion, Speedglitch, ItemClip, WallWalk, Windows Anti-AFK, and Linux Freeze
are approved migrations of this kind. Freeze's 50 ms auto-unfreeze override is now modeled on a
separate dedicated lane: its worker clock advances exactly 50 ms while the controller clock remains
at zero, preserving the internal unfreeze/wait/resuspend order without stalling Lag Switch.

## UI parity contract

The C++ UI currently includes hand-drawn `ImDrawList` geometry, including the footer/content border
extension in `app/app_ui.cpp`. Those pixels are part of the current visual contract even if a later
implementation can produce the same appearance more cleanly.

Parity tests compare fixed-state C++ and Rust captures at a matrix of window sizes and display
scales. Exact RGBA equality is the default. Masks are permitted only for explicitly documented
regions where the oracle itself is intentionally excluded; a mask must be narrow and reviewed.

Named layout constants are preferred in Rust even when their numeric values must match existing
magic constants. Pixel parity is the requirement, not preservation of the old implementation style.

## Branch/cutover policy

- `main` remains the production C++ line during the rewrite.
- Important C++ fixes can be cherry-picked into `rewrite/rust` while the oracle still exists.
- Do not delete the C++ tree early; same-checkout differential testing is intentional.
- The final cutover is a normal reviewed merge/replacement after tests prove parity.
- Local commits are expected on `rewrite/rust`; do not push without explicit approval.

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
  Rust Freeze slice composes this service end-to-end. `smu-app` now shares one characterized target
  cache implementation across live macro controllers: process matches refresh at most once per
  second, X11 foreground eligibility at most every 25 ms, foreground matching always considers every
  matching PID, and process-control users may independently select main-vs-all targets.
- `smu-input` owns separate physical-state and injection contracts, C++-oracle-tested loose/strict
  hotkey matching, transient-input boundaries, and both native-layout and historical fallback text
  entry. Linux character injection queries the current X11 keyboard/modifier maps and mirrors the C++
  six-state `XLookupString` search (`none`, Shift, Mod1, Mod5, Shift+Mod1, Shift+Mod5), including the
  first-key-per-modifier and X11-keycode-minus-8 evdev rules; synthetic-map tests freeze plain Shift
  and Mod5/ISO-Level3 behavior. If native layout resolution is unavailable, callers retain the exact
  compatibility US key map. Cursor/screen/pixel APIs are intentionally excluded; those belong to
  `smu-display`.
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
- `smu-app` now owns a safety-scoped profile-browser slice backed by `smu-profile`. The existing
  footer `Profiles ->` control opens the C++-style drop-up list and uses backup-aware, read-only
  discovery with the same SMC-before-RMC and canonical/executable/parent/current priority, but does
  not migrate, repair, create, or auto-save any file. The browser preserves default-first sorting,
  preferred-resume selection, rename validation, duplicate naming, and the 0.7 s two-click delete
  confirmation. Non-default duplicate/rename/delete operate on whole profile JSON objects through the
  already differential-tested backup-safe mutations; `(default)` duplication is disabled because C++
  captures live globals there. Save To and Load are intentionally disabled until Rust can round-trip
  every live/unported profile field through a complete `ShellState`/payload adapter. All browser CRUD
  tests use temporary settings files, and parity sessions instantiate a storage-isolated profile
  service so UI capture cannot touch user configuration.
- `smu-app` now owns the first live Linux Settings-window slice and shared keybind editor. The
  floating window is opened by the existing global Settings button and is backed by real app/service
  state for process-found status, Shift/Chat keybinds, chat override, paste delay, global zoom/reverse,
  display-scale/profile values, sidebar-description suppression, and native Wayland ScreenCast /
  RemoteDesktop status and actions. Wayland starts/stops run on one owned Settings worker so portal
  prompts and first-frame waits never block the UI thread; process status is cached for one second.
  The keybind editor preserves the C++ release-before-capture flow, modifier packing/normalization,
  wheel transient capture, modifier-only release binding, and post-bind hotkey suppression; all live
  macro controllers receive the resulting effective-macro gate, and Escape remains bindable while
  capture is active. Linux `smu-input` now forwards current-key/transient methods through `KeyState`,
  fixing trait-object wheel capture. This is intentionally a partial Settings cutover: Anti-AFK still
  lacks a live Rust app controller, updater HTTP/UI is not composed, Theme Editor remains unported,
  and profile/imported-script UI remain separate entry-point gates.
- `smu-app` now owns the live Linux Item Unequip COM Offset and Laugh Clip slices, completing live
  Linux pane/controller cutover for all 16 built-in macro sections. Both use one shared macro-text
  path: `OpenChatForMacro` first tries native-layout `/` only when chat override is enabled and falls
  back to the live configured chat hotkey when that native attempt fails; normal text tries native
  layout per byte and then the historical fallback map. Item Unequip keeps the C++ single-worker active
  guard, captures only custom/emote text before spawn, re-reads paste/Enter/dropdown/slot/keep-equipped
  settings inside the worker, and preserves the custom-text early return plus three separate dropdown
  reads. Laugh Clip keeps the same active guard and re-reads zoom mode, shift key, and S-movement state
  independently at action and release sites, so mid-worker setting drift retains the characterized
  asymmetries. This completes built-in macro execution cutover only; profile UI, remaining Settings
  controls, scripting UI, updater UI, and final C++ entry-point removal remain separate rewrite gates.
- `smu-app` now owns the live Linux Wall Helicopter High Jump slice plus its dedicated motion lane.
  The XButton1 rising-edge controller snapshots only the target PID vector; automatic-key, freeze,
  zoom/shift, HHJ length, and release settings remain live at the exact C++ worker read sites. The
  one-shot worker preserves the freeze-delay override/fast-HHJ branches and the historical quirk where
  HHJ length can activate the shared `isHHJ` state and later change to zero before the finish check,
  leaving that state active. A long-lived motion worker replaces repeated worker creation while keeping
  the section-2 + `isHHJ` gate and per-cycle FPS/strength live reads; X strength is read before the first
  move, Y strength only after the first frame wait. Linux Floor Bounce shares that same atomic rather
  than maintaining separate helicopter state.
- `smu-app` now owns the live Linux Floor Bounce slice. Its F4 rising edge snapshots the current target
  PID vector and launches overlapping one-shot workers. The fixed prefix remains direct Space-down,
  521 ms, suspend 72 ms, resume 72 ms, suspend 72 ms, resume, Space-up. Only after that 737 ms prefix
  does the worker re-read the experimental HHJ toggle; when enabled it re-reads delay 1, the shift key
  for bound key-down, delay 2, delay 3, then—after disabling shared HHJ state and a fixed 100 ms wait—
  re-reads the shift key for bound release. Negative configurable delays clamp to zero while retaining
  the wait boundary, matching the characterized C++ helper.
- `smu-app` now owns the live Linux Bunnyhop slice. Its 2 ms controller owns target-foreground
  eligibility, Intelligent Auto-Toggle chat locking, and the physical hotkey decision; the lock uses
  the C++ update order where chat-open is applied before Enter/left-click clears it, and guard exits do
  not reset it. Linux `smu-input` reads physical state only from real evdev devices and excludes SMU's
  uinput device, so the C++ keyboard-latch requirement is satisfied without a second synthetic state
  source. One long-lived owned worker replaces repeated activation-thread creation while preserving
  the active handoff: each iteration snapshots output hotkey and half-delay, performs bound key-down,
  waits the first half, always releases that snapshotted hotkey, and only then honors mid-cycle
  cancellation before the optional second half-delay. Wheel triggers remain physically ineligible and
  mouse-button/packed-modifier triggers use the characterized source classification.
- `smu-app` now owns the live Linux Ledge Bounce slice. The C-key controller keeps the C++
  rising-edge latch and foreground gate, then launches overlapping one-shot workers with no active
  guard. Sensitivity, Cam-Fix, and side are read at worker start; the worker executes the frozen
  90/40/16 ms prefix, re-reads horizontal realign at 146 ms, and re-reads auto-hold at 146 ms without
  realign or 216 ms with realign. The C-style sensitivity parser and turn calculation continue to
  come from the characterized `smu-runtime` helper, including invalid/negative/NaN flooring and
  overflow/truncation behavior.
- `smu-app` now owns the live Linux Spam a Key (`SpamKey`) slice as the third consumer of the
  shared multi-instance container. It preserves the constructor quirk where the UI initially displays
  `20` ms but the worker's `real_delay` remains `1000` ms until that text field is edited; edits use
  C++ `std::stof` numeric-prefix behavior and `int((delay + 0.5f) / 2)` conversion. Per-index runtime
  slots are stable and own one long-lived worker each. This intentionally removes C++'s dangling
  reference hazard while retaining copy-left/pop-back semantics: middle removal keeps the surviving
  slot/worker identity and only stops the popped final slot. Controller hold/toggle decisions use the
  characterized state machine; global Macro Toggle clears active worker state while preserving edge
  latches; each cycle snapshots delay and key-down, then re-reads the output key only after the first
  half-delay for release, matching the frozen live-read point.
- `smu-app` now also owns the live Linux Press a Button (`PressKey`) slice using the shared
  multi-instance container. Each instance keeps its own Z-by-default trigger, D-by-default bound
  output key, foreground gate, 16 ms output hold, and 0 ms bonus delay. Runtime keeps one rising-edge
  latch per instance and resets the entire latch vector on count changes, just like C++. Each rising
  edge snapshots trigger/output/delays before spawning an overlapping one-shot worker. The worker
  preserves the same-key quirk (release the trigger first when trigger and output match), clamps bonus
  delay to nonnegative and hold delay to at least 1 ms through the characterized runtime plan, checks
  the shared running flag only after the optional bonus delay, and otherwise completes output
  hold/release even if shutdown begins during the hold.
- `smu-app` now owns the live Linux Wallhop/Rotation slice and the first shared multi-instance UI
  container. The container preserves the C++ rules used by Wallhop, PressKey, and SpamKey: there is
  always one main instance; new instances append defaults; removing while the main slot is selected
  targets the final extra instance; removing an extra slot targets that slot; and removal requires a
  second click inside the 0.7-second confirmation window. Wallhop sidebar sub-instance selection is
  now live. Runtime state keeps one rising-edge latch per instance and resets the entire latch vector
  whenever the instance count changes, matching `wallhopWasPressed_.assign(size, false)`. Each edge
  snapshots the complete characterized worker plan before spawning an overlapping one-shot worker,
  including instance-local foreground gating and packed bound jump-key injection.
- `smu-app` now owns the live Linux Walless LHJ slice. The F6 controller snapshots the current
  process-target vector at the rising edge, then launches one-shot workers whose internal 645 ms
  sequence is preserved exactly. Side selection is read at worker start; global zoom/shift mode and
  reverse direction are re-read around 595 ms; the release mode and shift key are re-read around
  645 ms. Workers intentionally continue through the sequence if the global macro toggle changes,
  matching C++ generic-worker behavior. The controller also preserves its rising-edge latch while the
  global Macro Toggle is off. The same pause-latch rule was corrected for live Freeze: Freeze resumes
  suspended PIDs when globally disabled but does not reset the toggle edge latch.
- `smu-app` now owns the live Linux Wall-Walk slice. The F1 toggle/hold controller is separated
  from a dedicated motion worker. Each iteration snapshots FPS and the two pre-move side/strength
  reads before the first move, waits one characterized frame delay, performs the second move, then
  re-reads the microsecond tail delay only at the legacy post-move point. The Rust pane also preserves
  C++'s odd sensitivity calibration sign flip between the global-settings pass and the selected
  Wall-Walk section.
- `smu-app` now owns the live Linux Item Clip slice. The F3/slot-7/34-ms defaults feed a 2 ms
  foreground-gated controller and a dedicated worker. Every worker iteration snapshots delay and slot
  before key-down, clamps the half-delay exactly like C++, and always completes down/wait/up/wait before
  checking cancellation again. This is the characterized ItemClip controller/worker migration applied
  to the real native input service.
- `smu-app` now owns the live Linux Speedglitch slice as two long-lived lanes: a 2 ms controller
  retains foreground/hotkey/hold-vs-toggle state while a dedicated motion worker owns the frame waits.
  Each worker cycle snapshots runtime FPS exactly once, reads X immediately before the first move,
  then re-reads Y only after the first wait, preserving the characterized C++ live-read point. The
  Rust global settings state also preserves C++'s unusual startup split: production displays FPS `60`
  while the runtime value starts at `120` until the FPS text buffer is successfully synchronized; the
  Lag Switch parity hook displays `200` without changing that runtime value.
- `smu-app` now also owns the live Linux Item Desync slice. Its Rust pane preserves the F5/slot-5
  defaults and foreground gate, while a dedicated 2 ms controller lane emits the characterized
  down/up/down/up inventory-slot pulse for every active poll. The original C++ controller behavior
  remains frozen separately in the runtime fixture; the migration test proves all four pulse events
  moved to the dedicated lane while the shared controller clock remains unchanged.
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
regression test. HHJ motion, Speedglitch, ItemClip, WallWalk, Windows Anti-AFK, Linux Freeze, and
Linux Item Desync are approved migrations of this kind. The live Linux Speedglitch cutover now uses
that approved controller/worker split directly. Freeze's 50 ms auto-unfreeze override is modeled on a
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

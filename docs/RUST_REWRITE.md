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
regression test. HHJ motion, Speedglitch, ItemClip, and WallWalk are approved migrations of this kind.

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

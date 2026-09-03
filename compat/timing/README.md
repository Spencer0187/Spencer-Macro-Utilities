# Timing compatibility contract

Timing is observable SMU behavior. The Rust parity phase does not collapse the current controller
and worker topology into a single scheduler unless differential traces prove that doing so is
behaviorally identical.

## Current oracle topology

- `MacroRuntime::controllerLoop` polls most macro state on one controller thread. Sleeps performed by
  controller-path macros intentionally stall unrelated polling during that interval.
- One-shot and persistent worker macros created by `MacroRuntime::runWorker` run on independent
  native threads and may overlap each other and the controller.
- Imported scripts execute on independent runtime workers. Coroutines inside one script are resumed
  serially by that script instance.
- Native `InputBackend::pressKey` sleeps between down/up without cancellation.
- Lua `pressKey` uses `ScriptInstance::waitFor` and is cancellation-aware.
- Lua precise waits block efficiently until the final 600 microseconds and then spin the bounded
  tail. The 600us value is an oracle value, not yet a Rust tuning recommendation.

## What tests must prove

Logical tests use a fake monotonic clock and timestamped fake backends. They assert exact action
order and exact requested delays without depending on wall-clock scheduling noise.

Real-clock tests separately measure the platform timing primitive. The existing C++ benchmark sweeps
short waits and evaluates precision/CPU tradeoffs; Rust should retain an equivalent benchmark rather
than making ordinary unit tests flaky.

The first differential trace set must cover:

1. press-key down/delay/up ordering;
2. Wallhop split-delay and 100ms envelope behavior;
3. Speedglitch/ItemClip/WallWalk controller stalls;
4. overlapping one-shot workers;
5. SpamKey and Bunnyhop cancellation boundaries;
6. `FrameDelaysForFps` threshold behavior around the 0.33/0.66 fractional boundaries and 0.008
   epsilon;
7. Lua coarse wait, precise wait, cancellation, sleep-until-cancelled, and coroutine resume order;
8. Lua execution-budget reset after waits/checkpoints;
9. runtime shutdown ordering and held-input/process/network cleanup.

## Compatibility versus later fixes

Some current behavior is awkward or racy. For example, native one-shot workers can sleep through
shutdown and the current stop order can unfreeze a process before every native worker has joined.
Those are recorded as oracle behavior first. A later safety change should have its own test and
release note rather than being smuggled into the language rewrite.

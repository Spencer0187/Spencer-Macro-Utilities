# Compatibility oracle data

This directory holds artifacts used to compare the Rust rewrite against current SMU behavior.

## UI captures

`ui/cases.toml` describes the target capture matrix. A baseline should be produced from the C++
application in a deterministic state and then compared with the Rust build using `smu-compat`.

The default comparison policy is exact per-channel equality. Anti-aliasing/font differences are not
silently tolerated: if they appear, first make the renderer/font/DPI setup deterministic. Explicit
ignore rectangles are allowed only for reviewed oracle exceptions and are recorded beside the case.

`lag-switch-user-reference-872x580.png` is the user-provided reference that motivated the first UI
parity work. It is a reference image, not yet an automated golden capture.

## Timing/behavior fixtures

Future fixtures live under `compat/` rather than being embedded in implementation tests when they
represent cross-language contracts. C++ and Rust should consume the same input and emit normalized
JSON traces wherever practical.

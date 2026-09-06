# UI parity capture contract

Primary goldens are **client framebuffer** captures. Window decorations, desktop shadows, and other
OS chrome are excluded from the strict raster comparison.

Each capture stores sidecar geometry metadata:

- SDL logical window size and framebuffer pixel size;
- SDL display scale/video driver and window position;
- Dear ImGui `DisplaySize` and `DisplayFramebufferScale`;
- monitor usable bounds;
- current theme and relevant style values;
- `LSANS.TTF` hash/load source, 20px font size, oversampling configuration;
- selected module/profile/script, sidebar scroll state, process state, and deterministic dynamic text;
- key item rectangles for `GlobalSettings`, `LeftScrollSection`, `RightSection`, `BottomControls`,
  the first sidebar item, and the Profiles button.

## Raster policy

Same-platform/same-renderer captures target exact RGBA equality for solid interiors and zero logical
pixel drift at 1x. Fractional-scale one-unit strokes may use a one-physical-pixel antialias halo.
Text edge masks may be one pixel wide, but text baseline, wrapping, and bounding geometry remain
strict.

The manually reconstructed `RightSection`/footer seam in `app/app_ui.cpp` is handled specially:

- its expected coordinates are asserted from captured item geometry;
- only the one-pixel raster halo around the side borders, erased old bottom seam, and replacement
  bottom line may be ignored/tolerated;
- rounded-corner repair rectangles are **not** broadly masked.

The automated `settings-linux-minimum`, `theme-editor-linux-minimum`, and
`profile-menu-linux-minimum` cases additionally mask only the four 6x6 Dear ImGui rounded-corner
halos for their respective floating surfaces. Settings/Theme/Profile contents, the shell underneath,
and all non-corner floating-window geometry remain gated. `sidebar-no-scroll-linux-1280x1200`
keeps only the translated footer-seam exclusions and leaves the complete sidebar body gated; 1200px
is the measured 1x height where C++'s short-description fixture actually reaches
`GetScrollMaxY()==0` and switches the button-width adjustment from 18px to 7px. The paired
`process-not-found-linux-minimum` / `process-found-linux-minimum` cases keep live process discovery
disabled and deterministically force only the Roblox/Sober status state; their Rust captures differ
only inside the 10x10 status dot. `builtin-footer-linux-minimum` selects Freeze and gates the
built-in pane plus `Revert Module` footer; under bare Xvfb it also exercises C++'s foreground-
detection fallback because the required EWMH active-window properties are unavailable.
`imported-script-footer-linux-minimum` injects the same loaded, idle, unbound in-memory script on
both sides and gates its sidebar entry, selected-script controls/details/custom-settings state, and
footer reset without importing or executing a real Lua file; it needs no masks beyond the standard
minimum-size footer seam. `import-preview-linux-minimum` similarly injects an in-memory Lua source
preview and gates the centered trust modal, syntax-highlighted read-only preview child, disclaimer,
and dimmed shell underneath. `import-preview-error-linux-minimum` reuses that modal with the exact
embedded-NUL preview rejection text, covering the production error-colored preview branch without
reading a real file. Rust parity captures pin Dear ImGui `DeltaTime` to 1/60s after backend
frame setup, matching the C++ oracle's active-frame cadence so time-based modal dimming reaches the
same third-frame state. Only the four reviewed rounded modal-corner halos are additionally masked.
`warning-toast-linux-minimum` gates the real top-right warning FIFO/toast window, selectable read-only
message field, and Dismiss control. Its deterministic fixture is deliberately one line: Dear ImGui
rewrote punctuation-aware word breaking between 1.92.5 and 1.92.9, so this case isolates the toast
surface instead of treating that upstream wrapping algorithm as application behavior. Rust's live
Linux path also queues the foreground-detection fallback warning through this same FIFO. Only the
four measured 6x6 toast-window corner halos are additionally masked. `tutorial-media-linux-minimum`
selects Laugh Clip and replaces only the tutorial JPEG with the same centered 248x140 deterministic
placeholder on both apps, so the oracle gates the real media slot, surrounding tutorial text, and
module layout without depending on JPEG decoder output. Production Rust loads the same four tutorial
JPEGs as C++ through managed ImGui textures, with packaged/AppDir/source-tree asset resolution and
missing-asset warnings routed through the warning FIFO. Four companion cases gate the same centered
placeholder inside Wallhop, Item Clip, Wall Walk, and Spam Key. They caught Wallhop-specific drift:
Rust was missing C++'s IMPORTANT sensitivity note, auto-sized the 25px Jump Key reset button, and
incorrectly treated initial/profile sensitivity as a user edit, recalibrating the default 300px
flick to 600px before first render. Startup/profile state now preserves pixels like C++, while a real
sensitivity edit still triggers recalibration. These tutorial cases need no masks beyond the standard
minimum-size footer seam. Nine additional module-body cases select the remaining built-in sections
(Item Desync, HHJ, Speedglitch, Item Unequip COM Offset, Press a Button, Walless LHJ, Ledge Bounce,
Smart Bunnyhop, and Floor Bounce) without mutating runtime state. They caught HHJ/Floor Bounce
advanced controls that Rust had rendered unconditionally instead of behind C++'s closed-by-default
collapsing headers, a missing HHJ `Reference video coming soon!` line, and the hard-coded Bunnyhop
title instead of C++'s `Smart Bunnyhop` catalog title. Speedglitch has one reviewed 1x1 text
anti-aliasing delta at (738,467); the other module-body cases need only the standard footer seam.
The 1280x800 theme matrix now gates Cyberpunk idle and real sidebar hover plus Default Dark and
Sunset Orange. The hover case injects the same mouse position before Dear ImGui `NewFrame` on both
apps, so it exercises the production `ButtonHovered` path; idle-to-hover changes the same
(16,152)-(350,197) sidebar region on each implementation. Default Dark masks only three measured
zero-rounding edge runs, and Sunset Orange masks four tiny rounded footer-corner fragments.

That lets the Rust UI replace the old draw-list workaround while still proving it landed at the same
place and produced the same visible structure.

## Required states

Every scale captures the main shell/footer and both sidebar branches (`GetScrollMaxY()==0` and an
overflowing sidebar). At 1280x800/1x additionally capture Cyberpunk idle/hover, Default Dark,
Sunset Orange, profile menu, Settings, Theme Editor, import trust/syntax preview, warning toast,
tutorial media, built-in-module footer, imported-script footer, and Roblox process found/not found.

Windows lag-switch native overlay goldens are maintained separately because it is not an ImGui
surface.

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
only inside the 10x10 status dot.

That lets the Rust UI replace the old draw-list workaround while still proving it landed at the same
place and produced the same visible structure.

## Required states

Every scale captures the main shell/footer and both sidebar branches (`GetScrollMaxY()==0` and an
overflowing sidebar). At 1280x800/1x additionally capture Cyberpunk idle/hover, Default Dark,
Sunset Orange, profile menu, Settings, Theme Editor, import trust/syntax preview, warning toast,
tutorial media, built-in-module footer, imported-script footer, and Roblox process found/not found.

Windows lag-switch native overlay goldens are maintained separately because it is not an ImGui
surface.

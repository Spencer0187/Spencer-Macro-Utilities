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

The automated `settings-linux-minimum` case additionally masks only the four 6x6 Dear ImGui
floating-window corner halos. The Settings contents, updater panel, shell underneath the window,
and all non-corner window geometry remain gated.

That lets the Rust UI replace the old draw-list workaround while still proving it landed at the same
place and produced the same visible structure.

## Required states

Every scale captures the main shell/footer and both sidebar branches (`GetScrollMaxY()==0` and an
overflowing sidebar). At 1280x800/1x additionally capture Cyberpunk idle/hover, Default Dark,
Sunset Orange, profile menu, Settings, Theme Editor, import trust/syntax preview, warning toast,
tutorial media, built-in-module footer, imported-script footer, and Roblox process found/not found.

Windows lag-switch native overlay goldens are maintained separately because it is not an ImGui
surface.

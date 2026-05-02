# SDL3 UI Porting Notes

## Current Scope

- The main application window, GUI event pump, OpenGL context creation, and Dear ImGui platform backend now use SDL3 when `SMU_USE_SDL_UI=1`.
- The legacy Win32/WGL UI path is still guarded behind `SMU_USE_SDL_UI=0` as a temporary fallback.
- Global macro input is still routed through the existing platform/input backend and `wine_compatibility_layer` wrappers. SDL events are not used as the global macro input backend.
- The native Linux SDL3 path now renders the extracted legacy ImGui UI from `visual studio/macroframework.cpp` instead of the temporary Option A placeholder UI.
- Linux and SDL3 app builds use the same legacy theme/profile code paths through `app/app_theme_bridge.*` and `app/app_profile_bridge.*`.

## Preserved Windows Behavior

- Window title remains `Spencer Macro Client`.
- Saved window size and position are still applied when available; invalid saved values fall back to `1280x800`.
- Minimum window size is applied through `SDL_SetWindowMinimumSize`.
- Always-on-top is applied through `SDL_SetWindowAlwaysOnTop`, with a Win32 native-handle fallback on Windows.
- Window opacity is applied through `SDL_SetWindowOpacity`, with a Win32 layered-window fallback on Windows.
- The existing low-level keyboard hook, raw-input wheel bridge, SendInput path, admin relaunch flow, foreground Roblox window checks, and lagswitch code remain Windows-backed.
- The UI now loads the vendored `visual studio/Resource Files/LSANS.TTF` font at 20 px for SDL3 builds; if the file cannot be found at runtime, ImGui falls back to its default font and that is a visual parity gap.
- The legacy profile schema and theme schema are still owned by `profile_manager.cpp`, `theme_manager.cpp`, and `Resource Files/globals.h`.

## Remaining Parity Gaps

- Title bar color is still Windows-specific and uses the native HWND exposed by SDL. There is no cross-platform title-bar color implementation yet.
- The Windows raw-input mouse wheel path is preserved through SDL's Windows message hook so global wheel binds are not silently downgraded to focused-window SDL events. If raw input registration fails, SDL mouse wheel events act as a focused-window fallback only.
- The app icon is still applied through the Windows HWND. A cross-platform SDL surface icon can be added later if non-Windows UI builds need it.
- Linux input and Linux lagswitch behavior are intentionally unchanged in this step.
- Linux lagswitch remains disabled behind the shared `NetworkLagBackend` stub. The real lagswitch UI is visible for layout parity, but controls are disabled and show the backend unsupported reason.
- Linux foreground-window detection still depends on the platform capability result. On Wayland or unknown foreground support, per-macro "Disable outside of Roblox" checkboxes are forced into always-active fallback mode with the warning popup behavior.
- The extracted UI preserves the legacy settings, sidebar, selected-section controls, theme editor, profile menu, and bottom bar. Native macro execution threads for duplicated Press a Button, Wallhop/Rotation, and Spam a Key instances are still not launched by the Linux UI path; the UI state/profile data is present, but Linux macro execution remains backend work.

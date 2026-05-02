# Shared Macro Runtime Coverage

The shared runtime lives in `app/macro_runtime.cpp` and must stay platform-neutral. It may call `InputBackend`, `ProcessBackend`, `NetworkLagBackend`, and, if ever required, `WindowBackend`; it must not include Win32, WinDivert, Wine helper, or raw `SendInput`/thread-priority APIs.

| Macro | Shared runtime status | Notes |
| --- | --- | --- |
| Freeze | Implemented in shared runtime | Uses `ProcessBackend`; intentionally does nothing and does not notify when no target process is found. |
| Item Desync | Implemented in shared runtime | Uses `InputBackend` inventory-slot key presses. |
| Wall Helicopter High Jump | Implemented in shared runtime | Uses `ProcessBackend` freeze timing plus shared HHJ mouse movement loop. |
| Speedglitch | Implemented in shared runtime | Uses `InputBackend` mouse movement with FPS-derived frame delays. |
| Item Unequip COM Offset | Implemented in shared runtime | Uses shared text typing for the selected/custom chat command, then portable inventory-slot input. |
| Press a Button | Implemented in shared runtime | Per-instance macro using `InputBackend`. |
| Wallhop/Rotation | Implemented in shared runtime | Per-instance macro using `InputBackend`. |
| Walless LHJ | Implemented in shared runtime | Uses `InputBackend` plus `ProcessBackend` freeze/resume timing. |
| Item Clip | Implemented in shared runtime | Uses `InputBackend` inventory-slot key timing. |
| Laugh Clip | Implemented in shared runtime | Uses shared text typing for `/e laugh` plus portable input timing. |
| Wall-Walk | Implemented in shared runtime | Uses `InputBackend` mouse movement with FPS-derived delay. |
| Spam a Key | Implemented in shared runtime | Per-instance toggle/hold behavior uses a bounded joinable worker. |
| Ledge Bounce | Implemented in shared runtime | Uses `InputBackend` key/mouse timing. |
| Smart Bunnyhop | Implemented in shared runtime | Uses `InputBackend`; preserves chat lock behavior. |
| Floor Bounce | Implemented in shared runtime | Uses `InputBackend` plus `ProcessBackend` freeze/resume timing. |
| Lag Switch | Blocked by lagswitch | Routed through `NetworkLagBackend`; Linux remains an unsupported stub until a backend is integrated. |
| Anti-AFK | Not implemented | No portable implementation has been moved into `MacroRuntime` yet. |

# Spencer Macro Client
An open-source, cross-platform Windows, Linux, and macOS C++ Roblox ImGui macro with many features.

### Is this A CHEAT???
No, it's a macro; it doesn't communicate with Roblox memory in any way.

### Known Issues
- If it doesn't launch at all in Windows, go into properties and select "Unblock" on the file.

- Downgrading from a version with more features to a version with fewer features can cause the program to crash on launch. It is not recommended to downgrade. If required, delete your RMCsettings.json file.

![GitHub Releases](https://img.shields.io/github/downloads/Spencer0187/Spencer-Macro-Utilities/total.svg)

## [Link to Latest Version](https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/latest)
- Windows Installation: Run the executable "suspend" file.
- Linux Native Installation: Run the native `suspend` build. Follow the setup screens instructions.
- macOS Installation: Drag `suspend.app` to Applications, use Privacy & Security > Open Anyway when macOS warns that the app is unidentified, then follow the in-app permission setup screen.

## Join the Roblox Glitching Discord! (I can help you with support)
https://discord.gg/roblox-glitching-community-998572881892094012

# Current Features (Fully explained in-program):

1. Anti-AFK at all times (even if Roblox isn't shown) - Windows Only
2. Customizable UI buttons (Drag to Swap Locations)
3. Custom Lua Macro Scripts
4. Wall Helicopter High Jump
5. Speedglitch
6. Automatic Ledge Bouncing
7. Automatic Laugh Clipping
8. Dropless Item Desync Hitboxes
9. Freeze Macro
10. Unequip Speedglitch (Deprecated, Roblox patched)
11. Wallhop/Mouse-Move Macro (Supports multiple instances natively in the GUI)
12. Walless Lag High Jump (14 Studs)
13. Press a Key for One Frame
14. Wall-Walk
15. Item-Clip
16. Spam a Key/Button (Supports multiple instances natively in the GUI)
17. Intelligent Bhop/Bunnyhop

## Lua Macro Scripts

Custom macros can be written as Lua scripts and imported as `.smus`, `.hss`, `.lua`, or `.txt` files. See [docs/lua_macro_scripting.md](docs/lua_macro_scripting.md) for the scripting format, metadata comments, and the Lua API. Scripts can define `onSettings()` and use the `ui` table to render their own persistent ImGui controls.

# AUTOMATICALLY SAVES YOUR SETTINGS WHEN CLOSED

## The UI is customizable, drag your buttons to re-order them

<img width="1446" height="1053" alt="Macro Screenshot2" src="https://github.com/user-attachments/assets/428bc456-dfba-4fe7-8635-e7a2d3deab08" />

<img width="1446" height="1053" alt="Macro Screenshot3" src="https://github.com/user-attachments/assets/67332f63-2bb2-4b99-88ad-9169b5148adf" />

<img width="1446" height="1053" alt="Macro Screenshot1" src="https://github.com/user-attachments/assets/cd5b028c-7571-4e95-a2ec-1589fba2eb94" />

https://github.com/user-attachments/assets/a2c63feb-b947-4247-802c-34bf6cf8c2ce

## Code Signing Policy

Windows builds use free code signing provided by [SignPath.io](https://about.signpath.io/), with certificates issued by [SignPath Foundation](https://signpath.org/). macOS builds are not Apple Developer ID signed and are not notarized. They are signed with a stable self-signed project certificate so macOS privacy permissions are less likely to become stale after updates, but users still need to approve first launch through macOS Privacy & Security.

| [<img src="https://avatars.githubusercontent.com/u/34448643?s=25&v=4" width="25">](https://about.signpath.io/) | Free code signing provided by [**SignPath.io**](https://about.signpath.io/), certificate by [**SignPath Foundation**](https://signpath.org/) |
|----------------------------------------------------------------------------------------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------|

## Debugging Instructions
  (To receive generated logs)
  - Windows: Open Command Prompt in the directory of suspend.exe, run `set DEBUG=1`, and then run suspend.exe within Command Prompt.
  - Linux: Run using `DEBUG=1 ./suspend`.
  - macOS: Run the app binary with `DEBUG=1 ./suspend.app/Contents/MacOS/suspend` from the built bundle.
  - Developer-only MacroRuntime performance profiling on Windows: set `SMU_MACRORUNTIME_PERF=1` before launching. This appends `SMU MacroRuntime profile` blocks to `SMC.log` every few seconds.

## Compilation

### Windows CMake:
Open the repository folder directly in Visual Studio 2022 as a CMake project, or use VS Code with CMake Tools.

```powershell
cmake -S . -B build/windows -G "Visual Studio 17 2022" -A x64
cmake --build build/windows --config Release --target suspend
```

### Linux Native Backend:

Install dependencies on Ubuntu/Debian:
```bash
sudo apt-get update && sudo apt-get install -y build-essential cmake pkg-config golang-go libgl1-mesa-dev libx11-dev libxext-dev libxrandr-dev libxcursor-dev libxfixes-dev libxi-dev libxss-dev libxtst-dev libxinerama-dev libxkbcommon-dev
```

Go is required when building the Linux portable folder or AppImage because the Linux network lagswitch helper is compiled from `platform/linux/nethelper`. The helper module currently requests Go `1.26.2`; with Go toolchain auto-download enabled, a modern Go install will fetch that toolchain automatically during `go mod download`, `go test`, or `go build`. If your distro package is too old for toolchain auto-download, install a current Go release from https://go.dev/dl/ before running the package scripts.

Configure and build:
```bash
cmake -S . -B build/linux -DCMAKE_BUILD_TYPE=Release
cmake --build build/linux --target suspend
```

Build a portable folder:
```bash
cmake --build build/linux --target package-linux-dir
```

This creates `build/linux-package/SpencerMacroUtilities/` with `suspend`, `run.sh`, `LINUX_SETUP.md`, `scripts/install_linux_permissions.sh`, runtime assets, and bundled SDL3 when `SMU_BUNDLE_SDL3=ON`. Copy the whole folder to a compatible Linux system and launch it with `./run.sh`. The app starts unprivileged; if native input permissions are missing, it shows setup options inside the UI.

Runtime notes for the native Linux backend:
- Input injection uses `/dev/uinput`.
- Input state reading uses `/dev/input/event*`.
- The backend needs group-based access to those devices. Run `sudo ./scripts/install_linux_permissions.sh` once, then log out and back in or reboot if access is still missing.
- The app may launch the setup script with `pkexec`, but it does not relaunch the whole GUI as root and does not read your password.
- Runtime assets are loaded from `assets/` next to the executable.
- X11 foreground detection requires X11 development/runtime support and `_NET_ACTIVE_WINDOW` / `_NET_WM_PID`. Lua `moveMouseAbs()` and `getPixelColor()` on Linux also require X11/XWayland cursor-position/screen-read access; native Wayland sessions without usable X11 access report descriptive script errors instead of attempting unsupported absolute-coordinate behavior. Wayland support is currently in development.
- Wayland foreground process detection is intentionally unsupported.
- Linux network lagswitch support uses the bundled `nethelper` daemon. Build the portable folder or AppImage through the package scripts so `nethelper` is compiled and staged next to `suspend`.

### macOS Universal App Bundle:

Install the Xcode Command Line Tools first:
```bash
xcode-select --install
```

Configure and build the local architecture:
```bash
cmake --preset macos-release
cmake --build --preset macos-release --target suspend
open out/build/macos-release/suspend.app
```

Build the universal Intel + Apple Silicon package:
```bash
bash scripts/package_macos.sh
```

The universal preset sets `CMAKE_OSX_ARCHITECTURES` to `arm64;x86_64`, builds a single signed `suspend.app`, verifies the bundle executable with `lipo`, and stages both `SpencerMacroUtilities-macOS-universal.zip` and a drag-to-install dmg when `hdiutil` is present. Set `SMU_MACOS_SIGN_IDENTITY` to use a stable local code-signing certificate instead of ad-hoc signing.

Runtime notes for the native macOS backend:
- Accessibility permission enables global synthetic keyboard and mouse output.
- Screen Recording permission enables Lua screen pixel reads on the active monitor containing the cursor.
- macOS releases are not notarized. First launch requires System Settings > Privacy & Security > Open Anyway. Do not disable Gatekeeper globally.
- If permissions still show missing after an update, use the in-app `Reset macOS Permission Entries` button, re-enable Accessibility and Screen Recording, then restart SMU.
- If macOS quarantine gets stuck, run `xattr -dr com.apple.quarantine /Applications/suspend.app`.
- Permission changes are controlled by macOS. Use the in-app Restart & Check Permissions button after granting a permission.
- User settings are stored in `~/Library/Application Support/Spencer Macro Utilities/SMCSettings.json`, including when SMU is launched from a dmg.
- macOS auto-update can replace a writable installed `.app` bundle from the universal zip release asset. A read-only dmg launch can check for updates but must be installed to Applications before automatic replacement is possible.
- Foreground Roblox detection uses the frontmost macOS app. Process freeze uses macOS process stop/continue signals where the OS permits it.
- Lagswitch support is intentionally unsupported on macOS.

See [`MACOS_SETUP.md`](MACOS_SETUP.md) for build, unsigned-release, permission repair, and optional Developer ID notes.

---

### Team Roles
- **Committer and Approver**: [Project Owner (Spencer)](https://github.com/Spencer0187/) | [Discord](https://discord.com/users/618407079622737931)
- **Linux Maintainer**: [quuut](https://github.com/quuuut) | [Discord](https://discord.com/users/750631921079287839)
- **Maintainer and Packager**: [Floofy](https://github.com/FloofyIV) | [Discord](https://discord.com/users/966112150915518474)

### Privacy Policy
This application makes client-side HTTP requests solely for version checking and updates. No user data is collected or transmitted to any servers.

## Credits

- Freezing code framework based on [craftwar/suspend](https://github.com/craftwar/suspend)
- ImGui GUI library [ocornut/imgui](https://github.com/ocornut/imgui)
- WinDivert used for Lagswitching Capabilities [basil00/WinDivert](https://github.com/basil00/WinDivert)

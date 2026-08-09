# Spencer Macro Utilities

A native Windows, Linux, and macOS Roblox macro client with built-in movement tools and a full cross-platform Lua scripting API.

[![Latest release](https://img.shields.io/github/v/release/Spencer0187/Spencer-Macro-Utilities?label=release)](https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/latest)
[![Build status](https://github.com/Spencer0187/Spencer-Macro-Utilities/actions/workflows/build.yml/badge.svg)](https://github.com/Spencer0187/Spencer-Macro-Utilities/actions/workflows/build.yml)
[![Downloads](https://img.shields.io/github/downloads/Spencer0187/Spencer-Macro-Utilities/total.svg)](https://github.com/Spencer0187/Spencer-Macro-Utilities/releases)
[![License](https://img.shields.io/github/license/Spencer0187/Spencer-Macro-Utilities)](LICENSE)

SMU sends normal keyboard and mouse input. It does not read or modify Roblox memory.

## Download

Official packages are available on the [Releases page](https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/latest).

| Platform | Download | Install |
| --- | --- | --- |
| Windows 10/11 x64 | `xyz.exe` or `xyz.zip` | Extract the ZIP and run `suspend.exe`. If Windows blocks it, open Properties and select **Unblock**. |
| macOS 11+ | `xyz-macOS-universal.dmg` or `.zip` | Drag `suspend.app` to Applications, then approve the first launch in **System Settings → Privacy & Security**. |
| Generic Linux x86_64 | `xyz-Linux-x86_64.zip` or `.AppImage` | Choose the AppImage or portable tarball inside. Follow `LINUX_SETUP.md` once for input permissions. |
| Arch | read `install` | Use any AUR helper to install the `spencers-macro-git` AUR package, eg: `yay -Sy spencers-macro-git` |
| Debian | `xyz.deb` | Use the `apt` package manager to install it with this **exemplary** command as root: `apt install /path/to/xyz.deb` |
| Fedora | `xyz.rpm` | Use the `dnf` package manager to install it with this **exemplary** command as root: `dnf install /path/to/xyz.rpm` |

The macOS build is universal for Apple Silicon and Intel. Linux packages run the interface as your normal user; SMU uses the desktop authentication prompt only for the small permission/network helper when required.
The RPM is built for Fedora/RHEL-family package names. openSUSE users should use the AppImage or portable archive until that RPM has native openSUSE validation.

For help, join the [Roblox Glitching Community Discord](https://discord.gg/roblox-glitching-community-998572881892094012).

## Features

- Wallhop/rotation with multiple independent selections, horizontal and vertical movement, degree or pixel modes, and a configurable jump key
- Wall Helicopter High Jump, Speedglitch, Walless Lag High Jump, Item Clip, Item Desync, Laugh Clip, Wall-Walk, Ledge Bounce, Floor Bounce, and Smart Bunnyhop
- Process freeze, key press, key spam, customizable hotkeys, automatic profile saving, and drag-to-reorder controls
- Network lag switch with hard blocking or fake lag, direction, protocol, Roblox targeting, and automatic release controls on Windows and Linux
- A complete Lua macro API with managed input, timing, cancellation, screen pixels, process and lag controls, saved settings, and custom interface widgets
- Script import by file picker or drag and drop, read-only previews, resource limits, and cleanup when a script is stopped
- Custom themes, scalable interface layouts, copyable notifications, and per-profile macro configuration

See [the scripting guide](docs/lua_macro_scripting.md) for the script format and API.

<img width="1200" alt="Spencer Macro Utilities macro interface" src="https://github.com/user-attachments/assets/428bc456-dfba-4fe7-8635-e7a2d3deab08" />

## Platform support

| Capability | Windows | Linux | macOS |
| --- | :---: | :---: | :---: |
| Built-in macros | Yes | Yes | Yes |
| Lua scripting API | Yes | Yes | Yes |
| Global keyboard and mouse input | Yes | Yes¹ | Yes² |
| Foreground Roblox detection | Yes | X11/XWayland³ | Yes |
| Process freeze | Yes | Yes | Yes |
| Network hard-block lag switch | Yes | Yes⁴ | Not yet⁵ |
| Fake lag | Yes | Yes⁴ | No |
| Global lag-switch overlay | Yes | No | No |
| Always-on Anti-AFK | Yes | No | No |

1. Linux needs one-time access to `/dev/input` and `/dev/uinput`; the app provides the setup flow.
2. macOS needs Accessibility permission. Screen Recording permission is needed for script pixel reads.
3. Native Wayland restricts foreground-app detection, absolute pointer positioning, and screen reads. SMU explains unavailable behavior instead of silently failing.
4. Linux hard-block and fake-lag rules match selected machine-wide traffic rather than a Sober process. Roblox mode combines the static Roblox range with the UDMUX and RCC addresses discovered from Sober's player log. Fake lag refuses to replace custom traffic-control configuration on the active interface.
5. A safe macOS lag switch needs a Developer ID-signed, Apple-entitled Network Extension. We are not paying for that.

On a Wayland session, SMU prefers SDL's native Wayland backend and falls back to X11/XWayland only if native initialization fails. An explicit `SDL_VIDEO_DRIVER` (or legacy `SDL_VIDEODRIVER`) setting is always preserved. Native Wayland window opacity works both with compositors that support `wp_alpha_modifier_v1` and through an alpha-buffer fallback on compositors that do not.

## Updates

SMU checks the official GitHub repository over HTTPS and only accepts release assets from that repository.

| Installation | Update behavior |
| --- | --- |
| Windows official ZIP | Downloads the Windows ZIP and installs only a valid trusted signed executable. |
| Linux AppImage in a writable folder | Downloads the all-in-one Linux ZIP and replaces the AppImage in place. |
| Linux `.deb`, `.rpm`, portable, or Nix | Shows the new version when `curl` is available; reinstall with the same package method. |
| macOS app installed in a writable folder | Replaces the app only when the new bundle is valid and signed by the same release certificate. |
| macOS app opened from the DMG | Shows the new version; copy the app to Applications before updating. |

See each release’s **Important Platform Notes** before installing. Downgrading across settings-format changes is not supported; if an old build cannot load a newer profile, back up and remove its settings file.

## Build from source

Clone the repository, enter it, and use the cross-platform build command:

```bash
git clone https://github.com/Spencer0187/Spencer-Macro-Utilities.git
cd Spencer-Macro-Utilities
python3 scripts/build.py
```

On Linux, build an AppImage locally with:

```bash
python3 scripts/build.py --appimage
```

Or create the complete Linux release ZIP after installing nFPM:

```bash
python3 scripts/build.py --linux-release
```

Nix users can build or run the locked flake directly:

```bash
nix build
nix run
```

The same command builds an unsigned Windows executable on Windows, a portable directory on Linux, and a universal app package on macOS. Dependency lists, outputs, Nix commands, and direct CMake instructions are in [docs/building.md](docs/building.md).

Release maintainers can bump the single canonical version with:

```bash
python3 scripts/version.py --bump patch
# or: --bump minor / --bump major
```

The app, packages, updater metadata, and release workflow all read that version. See [docs/releasing.md](docs/releasing.md) for the complete signed draft-release checklist.

## Platform setup and troubleshooting

- [Linux setup and permissions](LINUX_SETUP.md)
- [macOS setup, permissions, and first launch](MACOS_SETUP.md)
- [Local and release builds](docs/building.md)

To enable logs, start SMU with `DEBUG=1` on Linux/macOS or `set DEBUG=1` in Windows Command Prompt. Windows maintainers can additionally enable MacroRuntime profiling with `SMU_MACRORUNTIME_PERF=1`.

## Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by [SignPath Foundation](https://signpath.org/).

- Committers and reviewers: [Spencer](https://github.com/Spencer0187), [Floofy](https://github.com/FloofyIV), and [quuut](https://github.com/quuuut)
- Signing approver: [Spencer](https://github.com/Spencer0187)

Every Windows release signing request requires approval through SignPath. The release workflow builds from the selected repository commit, verifies the returned Authenticode signature, and only then packages the executable.

macOS releases use a stable project self-signed certificate so permission identity survives updates, but they are not Apple Developer ID-signed or notarized. First launch therefore requires **Open Anyway**; SMU never asks users to disable Gatekeeper.

SMU has no telemetry or user accounts. It makes a once-per-session HTTPS request to GitHub's public release API to check for updates and downloads an official release asset only after the user confirms installation. See the [privacy policy](PRIVACY.md).

## Team and credits

- Project owner: [Spencer](https://github.com/Spencer0187/)
- Linux maintainer: [quuut](https://github.com/quuuut)
- Maintainer and packager: [Floofy](https://github.com/FloofyIV)
- Process freezing framework: [craftwar/suspend](https://github.com/craftwar/suspend)
- Interface: [Dear ImGui](https://github.com/ocornut/imgui)
- Windows network filtering: [WinDivert](https://github.com/basil00/WinDivert)

SMU is licensed under the terms in [LICENSE](LICENSE).
Third-party copyright and license information is in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

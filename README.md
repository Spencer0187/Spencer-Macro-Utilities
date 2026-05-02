# Spencer Macro Client
An open-source, Cross-Platform Windows + Linux C++ Roblox ImGui Macro with many features.

### Is this A CHEAT???
No, it's a macro, it doesn't communicate with Roblox memory in any way.

### Known Issues
- If it doesn't launch at all in Windows, go into properties and select "Unblock" on the file.

- Downgrading from a version with more features to a version with fewer features can cause the program to crash on launch. It is not recommended to downgrade. If required, delete your RMCsettings.json file.

![GitHub Releases](https://img.shields.io/github/downloads/Spencer0187/Spencer-Macro-Utilities/total.svg)

## [Link to Latest Version](https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/latest)
- Windows Installation: Run the executable "suspend" file.
- Linux Installation: Run the executable "suspend" file through [Wine](https://gitlab.winehq.org/wine/wine/-/wikis/Download), and accept the Admin Confirmation. Works across nearly all distros.
(Zenity must be installed on Linux to launch, run `sudo apt-get install zenity` or equivalent command)

## Join the Roblox Glitching Discord! (I can help you with support)
https://discord.gg/roblox-glitching-community-998572881892094012

# Current Features (Fully explained in-program):

1. Anti-AFK at all times (even if Roblox isn't shown)
2. Customizable UI buttons (Drag to Swap Locations)
3. Wall Helicopter High Jump
4. Speedglitch
5. Automatic Ledge Bouncing
6. Automatic Laugh Clipping
7. Dropless Item Desync Hitboxes
8. Freeze Macro
9. Unequip Speedglitch (Deprecated, Roblox patched)
10. Wallhop/Mouse-Move Macro (Supports multiple instances natively in the GUI)
11. Walless Lag High Jump (14 Studs)
12. Press a Key for One Frame
13. Wall-Walk
14. Item-Clip
15. Spam a Key/Button (Supports multiple instances natively in the GUI)
16. Intelligent Bhop/Bunnyhop

# AUTOMATICALLY SAVES YOUR SETTINGS WHEN CLOSED

## The UI is customizable, drag your buttons to re-order them

<img width="1446" height="1053" alt="Macro Screenshot2" src="https://github.com/user-attachments/assets/428bc456-dfba-4fe7-8635-e7a2d3deab08" />

<img width="1446" height="1053" alt="Macro Screenshot3" src="https://github.com/user-attachments/assets/67332f63-2bb2-4b99-88ad-9169b5148adf" />

<img width="1446" height="1053" alt="Macro Screenshot1" src="https://github.com/user-attachments/assets/cd5b028c-7571-4e95-a2ec-1589fba2eb94" />

https://github.com/user-attachments/assets/a2c63feb-b947-4247-802c-34bf6cf8c2ce

## Code Signing Policy

This project uses free code signing provided by [SignPath.io](https://about.signpath.io/), with certificates issued by [SignPath Foundation](https://signpath.org/).

| [<img src="https://avatars.githubusercontent.com/u/34448643?s=25&v=4" width="25">](https://about.signpath.io/) | Free code signing provided by [**SignPath.io**](https://about.signpath.io/), certificate by [**SignPath Foundation**](https://signpath.org/) |
|----------------------------------------------------------------------------------------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------|

## Debugging Instructions
  (To show printed messages)
  - Windows: Open Command Prompt in the directory of suspend.exe, run `set DEBUG=1`, and then run suspend.exe within Command Prompt.
  - Linux: Run using `DEBUG=1 wine suspend.exe`.  

## Compilation

### Windows Method #1:
- Run the .sln file in any Version of Visual Studio 2022 or newer and compile it.

### Windows Method #2:
- Open the project folder in Visual Studio Code and run the `Build Release` task with [MSBuild 2022 Tools for Desktop C++](https://aka.ms/vs/17/release/vs_buildtools.exe) installed.

### Windows CMake:
```powershell
cmake --preset windows-msvc-debug
cmake --build --preset windows-msvc-debug
```

Use `windows-msvc-release` instead of `windows-msvc-debug` for a release build.

### Linux Native Backend:
The Linux build now produces the native `suspend` executable. SDL3 is built from the vendored source tree in `third_party/SDL`, so you do not need a system `libsdl3-dev` package.

Install dependencies on Ubuntu/Debian:
```bash
sudo apt-get update && sudo apt-get install -y build-essential cmake pkg-config libgl1-mesa-dev libx11-dev libxext-dev libxrandr-dev libxcursor-dev libxfixes-dev libxi-dev libxss-dev libxtst-dev libxkbcommon-dev
```

Configure and build:
```bash
cmake --preset linux-release && cmake --build --preset linux-release
```

Runtime notes for the native Linux backend:
- Input injection uses `/dev/uinput`.
- Input state reading uses `/dev/input/event*`.
- The backend must run as root or with equivalent uinput/input device permissions.
- X11 foreground detection requires X11 development/runtime support and `_NET_ACTIVE_WINDOW` / `_NET_WM_PID`.
- Wayland foreground process detection is intentionally unsupported.
- Linux network lagswitch support is not part of the native backend target.

### Linux Wine Helper:
The existing Wine path still uses the separate Linux helper source in `visual studio/Resource Files/Suspend_Input_Helper_Source`. Compile that helper with `g++` when updating the Wine compatibility helper binary.

---

### Team Roles
- **Committer and Approver**: [Project Owner (Spencer)](https://github.com/Spencer0187/) | [Discord](https://discord.com/users/618407079622737931)
- **Linux Maintainer**: [quuut](https://github.com/quuuut) | [Discord](https://discord.com/users/750631921079287839)

### Privacy Policy
This application makes client-side HTTP requests solely for version checking and updates. No user data is collected or transmitted to any servers.

## Credits

- Freezing code framework based on [craftwar/suspend](https://github.com/craftwar/suspend)
- ImGui GUI library [ocornut/imgui](https://github.com/ocornut/imgui)
- WinDivert used for Lagswitching Capabilities [basil00/WinDivert](https://github.com/basil00/WinDivert)

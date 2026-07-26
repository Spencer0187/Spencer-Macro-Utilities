# Linux Native Input Setup

Spencer Macro Utilities reads global key state through `/dev/input/event*` and injects input through `/dev/uinput`. Linux protects those device files because they expose keyboard and mouse input for the whole desktop.

The GUI app should run as your normal desktop user. If SMU cannot access the Linux input devices on startup, it shows an in-app setup modal that can launch a one-time installer for the required permissions.

Do not run the whole GUI as root, do not run the AppImage with `sudo`, and do not launch `suspend` with `sudo`. Only the installer script should be elevated.

SMU never asks for your sudo password inside its own ImGui interface. Authentication is handled by `pkexec` or `sudo`.

## Runtime Tools

SMU uses `curl` for update checks, `pkexec` for graphical authentication, `iptables` for hard blocking, and `iproute2` (`tc` and `ip`) for optional fake lag. The Debian and RPM packages declare these dependencies, and the Nix package supplies them. AppImage and portable users should install `curl`, Polkit, `iptables`, `iproute2`, and `kmod` with their distribution's package manager if those commands are not already present.

The included RPM uses Fedora/RHEL-family dependency names. On openSUSE, use the AppImage or portable archive unless you have independently validated and adapted the RPM dependencies.

## Linux Lag Switch

The optional Go helper provides both hard blocking and fake lag. Hard-block
iptables rules match the selected traffic for the whole machine rather than a
specific Sober process. **Only Lag Switch Roblox** combines the static Roblox
IPv4 range `128.116.0.0/16` with the current UDMUX and RCC server addresses
read from Sober's player log; turning it off selects all traffic. Fake lag uses
SMU-owned `tc`/IFB state and refuses to replace custom traffic-control settings
on the active interface.

The helper watches the exact SMU process that launched it and removes its own
firewall chains when that process exits or crashes. It never clears the
system's built-in firewall chains. If the helper itself is forcibly killed
with `SIGKILL`, launch the lag switch again so its startup cleanup can remove
any stale SMU-owned chains.

## In-App Setup

When permissions are missing, the setup modal offers:

1. Graphical `pkexec` using your desktop polkit agent.
2. A terminal installer that runs `sudo ./scripts/install_linux_permissions.sh`.
3. A copyable manual `sudo` command if no supported terminal could be launched.

The installer writes persistent udev rules, adds your user to `smu-input`, and applies temporary ACLs with `setfacl` when it is available. If access still does not work immediately, log out and back in or reboot so your session gets the new group membership.

If your desktop does not run a graphical polkit agent, `pkexec` may fail without showing an authentication window. Hyprland, i3, sway, Openbox, and other minimal environments often need an agent installed and autostarted, such as `hyprpolkitagent`, `polkit-kde-agent`, or `polkit-gnome`.

## Manual Install

From the portable folder or repository root, the manual command is:

```bash
sudo ./scripts/install_linux_permissions.sh
```

If you extracted an AppImage manually, the installer script is bundled inside the `AppDir` and can be run from there with `sudo` the same way. In normal AppImage use, just run the AppImage as your user and use the in-app setup modal.

## Security

Members of `smu-input` can read global input events and write to `/dev/uinput`. Only add users you trust with desktop-wide input access. The installer does not make input devices world-readable or world-writable.

## Undo

Remove the udev rule and group membership:

```bash
sudo gpasswd -d "$USER" smu-input
sudo rm -f /etc/udev/rules.d/70-spencer-macro-utilities.rules
sudo udevadm control --reload-rules
sudo udevadm trigger
```

Then log out and back in or reboot. You can remove the group too if nothing else uses it:

```bash
sudo groupdel smu-input
```

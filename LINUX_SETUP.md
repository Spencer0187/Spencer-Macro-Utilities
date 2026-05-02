# Linux Native Input Setup

Spencer Macro Utilities reads global key state through `/dev/input/event*` and injects input through `/dev/uinput`. Linux protects those device files because they expose keyboard and mouse input for the whole desktop.

The app should not run the whole GUI as root. Instead, install a one-time udev rule that grants access to members of the `smu-input` group.

SMU never asks for your sudo password inside its own ImGui interface. Authentication is handled by `pkexec` or `sudo`.

## Install

From the portable folder or repository root:

```bash
sudo ./scripts/install_linux_permissions.sh
```

Inside the app, the setup modal tries the installer in this order:

1. Graphical `pkexec` using your desktop polkit agent.
2. A terminal installer that runs `sudo ./scripts/install_linux_permissions.sh`.
3. A copyable manual `sudo` command if no supported terminal could be launched.

If your desktop does not run a graphical polkit agent, `pkexec` may fail without showing an authentication window. Hyprland, i3, sway, Openbox, and other minimal environments often need an agent installed and autostarted, such as `hyprpolkitagent`, `polkit-kde-agent`, or `polkit-gnome`.

The installer writes persistent udev rules, adds your user to `smu-input`, and applies temporary ACLs with `setfacl` when it is available. If access still does not work immediately, log out and back in or reboot so your session gets the new group membership.

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

# Linux Native Input Setup

Spencer Macro Utilities reads global key state through `/dev/input/event*` and injects input through `/dev/uinput`. Linux protects those device files because they expose keyboard and mouse input for the whole desktop.

The app should not run the whole GUI as root. Instead, install a one-time udev rule that grants access to members of the `smu-input` group.

## Install

From the portable folder or repository root:

```bash
sudo ./scripts/install_linux_permissions.sh
```

The app can also launch that same script with `pkexec`. If your desktop does not have a graphical polkit authentication agent running, `pkexec` may fail without a password prompt. On Hyprland and other lightweight desktops, install and autostart an agent such as `hyprpolkitagent`, `polkit-kde-agent`, or `polkit-gnome`, or run the sudo command manually.

After installation, log out and back in or reboot so your session gets the new `smu-input` group membership.

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

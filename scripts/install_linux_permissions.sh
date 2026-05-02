#!/usr/bin/env bash
set -euo pipefail

if [[ "${EUID}" -ne 0 ]]; then
  echo "This installer must be run as root." >&2
  echo "Run: sudo ./scripts/install_linux_permissions.sh" >&2
  exit 1
fi

TARGET_USER="${SMU_TARGET_USER:-${SUDO_USER:-}}"
if [[ -z "${TARGET_USER}" || "${TARGET_USER}" == "root" ]]; then
  echo "Could not determine the target desktop user." >&2
  echo "Set SMU_TARGET_USER to your username, or run through sudo from your user account." >&2
  exit 1
fi

if ! id "${TARGET_USER}" >/dev/null 2>&1; then
  echo "Target user does not exist: ${TARGET_USER}" >&2
  exit 1
fi

if ! getent group smu-input >/dev/null 2>&1; then
  groupadd --system smu-input
fi

usermod -aG smu-input "${TARGET_USER}"

install -d -m 0755 /etc/udev/rules.d
cat >/etc/udev/rules.d/70-spencer-macro-utilities.rules <<'RULES'
KERNEL=="uinput", MODE="0660", GROUP="smu-input", OPTIONS+="static_node=uinput"
SUBSYSTEM=="input", KERNEL=="event*", MODE="0660", GROUP="smu-input"
RULES

modprobe uinput || true
udevadm control --reload-rules
udevadm trigger

echo "Installed Spencer Macro Utilities Linux input permissions."
echo "User '${TARGET_USER}' is now a member of smu-input."
echo "Log out and back in, or reboot, if the app still cannot access input devices."

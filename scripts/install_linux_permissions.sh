#!/usr/bin/env bash
set -euo pipefail

if [[ "${EUID}" -ne 0 ]]; then
  echo "This installer must be run as root." >&2
  echo "Run: sudo ./scripts/install_linux_permissions.sh" >&2
  exit 1
fi

TARGET_USER="${SMU_TARGET_USER:-}"

if [[ -z "${TARGET_USER}" && -n "${PKEXEC_UID:-}" ]]; then
  TARGET_USER="$(getent passwd "${PKEXEC_UID}" | cut -d: -f1 || true)"
fi

if [[ -z "${TARGET_USER}" && -n "${SUDO_UID:-}" ]]; then
  TARGET_USER="$(getent passwd "${SUDO_UID}" | cut -d: -f1 || true)"
fi

if [[ -z "${TARGET_USER}" && -n "${SUDO_USER:-}" ]]; then
  TARGET_USER="${SUDO_USER}"
fi

if [[ -z "${TARGET_USER}" || "${TARGET_USER}" == "root" ]]; then
  echo "Could not determine the target desktop user." >&2
  echo "Run with sudo from your user account, or set SMU_TARGET_USER." >&2
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
KERNEL=="uinput", MODE="0660", GROUP="smu-input", TAG+="uaccess", OPTIONS+="static_node=uinput"
SUBSYSTEM=="input", KERNEL=="event*", MODE="0660", GROUP="smu-input", TAG+="uaccess"
RULES

modprobe uinput || true
udevadm control --reload-rules
udevadm trigger

if command -v setfacl >/dev/null 2>&1; then
  for node in /dev/uinput /dev/input/event*; do
    if [[ -e "${node}" ]]; then
      setfacl -m "u:${TARGET_USER}:rw" "${node}" || true
    fi
  done
else
  echo "setfacl was not found; access may require logging out and back in." >&2
fi

echo "Installed Spencer Macro Utilities Linux input permissions."
echo "User '${TARGET_USER}' is now a member of smu-input."
echo "Temporary ACLs were applied when setfacl was available."
echo "If SMU still cannot access input devices, log out and back in or reboot."

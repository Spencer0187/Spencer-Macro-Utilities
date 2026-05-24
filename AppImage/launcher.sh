#!/usr/bin/env bash
set -euo pipefail
NETHELPER="/usr/bin/nethelper"
TMP="/tmp/nethelper-$USER"

if ! pgrep -f "$TMP" >/dev/null 2>&1; then
  rm -f "$TMP"
  cp "$NETHELPER" "$TMP"
  chmod +x "$TMP"
  pkexec "$TMP" &
fi

exec /usr/share/spencer-macro-utilities/suspend "$@"

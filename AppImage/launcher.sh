#!/usr/bin/env bash
set -euo pipefail
NETHELPER="/usr/bin/nethelper"
TMP="/tmp/nethelper-$USER"

if ! pgrep -x "nethelper" >/dev/null 2>&1; then
  rm -f "$TMP"
  cp "$NETHELPER" "$TMP"
  chmod +x "$TMP"
  pkexec "$TMP" &
fi

exec /usr/libexec/spencer-macro-utilities/suspend "$@"

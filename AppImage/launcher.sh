#!/usr/bin/env bash
set -euo pipefail

NETHELPER="/usr/libexec/spencer-macro-utilities/nethelper"

TMP="/tmp/nethelper-$USER"

cp "$NETHELPER" "$TMP"
chmod +x "$TMP"

# only start once (prevents duplicates)
if ! pgrep -f "$NETHELPER" >/dev/null 2>&1; then
  pkexec "$TMP"
fi

exec /usr/bin/suspend "$@"

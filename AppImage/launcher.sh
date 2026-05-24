#!/usr/bin/env bash
set -euo pipefail
NETHELPER="/usr/bin/nethelper"
TMP="/tmp/nethelper-$USER"

if [ ! -S "/tmp/nethelper.sock" ]; then
  rm -f "$TMP"
  cp "$NETHELPER" "$TMP"
  chmod +x "$TMP"
  pkexec "$TMP" &
fi

exec /usr/libexec/spencer-macro-utilities/suspend "$@"

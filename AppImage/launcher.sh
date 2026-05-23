#!/usr/bin/env bash
set -euo pipefail

NETHELPER="/usr/libexec/spencer-macro-utilities/nethelper"

if ! pgrep -f "$NETHELPER" >/dev/null 2>&1; then
  "$NETHELPER" >/dev/null 2>&1 &
fi

exec /usr/bin/suspend "$@"

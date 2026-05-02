#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD_DIR="${SMU_BUILD_DIR:-"$ROOT_DIR/build/linux-package"}"

cmake -S "$ROOT_DIR" -B "$BUILD_DIR" \
  -DCMAKE_BUILD_TYPE="${CMAKE_BUILD_TYPE:-Release}" \
  -DSMU_BUNDLE_SDL3="${SMU_BUNDLE_SDL3:-ON}" \
  -DSMU_LINK_SDL3_STATIC="${SMU_LINK_SDL3_STATIC:-OFF}" \
  "$@"

cmake --build "$BUILD_DIR" --target package-linux-dir --parallel

PACKAGE_DIR="$BUILD_DIR/SpencerMacroUtilities"
SUSPEND_BIN="$PACKAGE_DIR/suspend"

echo
echo "Portable folder: $PACKAGE_DIR"
echo
file "$SUSPEND_BIN"
echo
ldd "$SUSPEND_BIN" | tee "$BUILD_DIR/package-linux-ldd.txt"

if grep -q "not found" "$BUILD_DIR/package-linux-ldd.txt"; then
  echo "ERROR: ldd reports missing shared libraries." >&2
  exit 1
fi

if grep -q "libSDL3" "$BUILD_DIR/package-linux-ldd.txt" && ! compgen -G "$PACKAGE_DIR/lib/libSDL3.so*" >/dev/null; then
  echo "ERROR: suspend depends on SDL3, but no bundled libSDL3.so* was found in $PACKAGE_DIR/lib." >&2
  exit 1
fi

test -f "$PACKAGE_DIR/assets/LSANS.TTF"
test -f "$PACKAGE_DIR/assets/smu_icon.bmp"
test -x "$PACKAGE_DIR/run.sh"

echo
echo "Package diagnostics passed. Copy the whole SpencerMacroUtilities folder to a compatible Linux system and run ./run.sh."
echo "Recommended distribution formats: portable tarball now; AppImage or distro packages later."

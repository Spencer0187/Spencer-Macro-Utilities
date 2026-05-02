#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD_DIR="${SMU_BUILD_DIR:-"$ROOT_DIR/build/linux-appimage"}"
APPDIR="$BUILD_DIR/AppDir"
APPIMAGETOOL="${APPIMAGETOOL:-appimagetool}"
OUTPUT_NAME="${SMU_APPIMAGE_NAME:-Spencer-Macro-Utilities-x86_64.AppImage}"
APP_ICON_SOURCE="$ROOT_DIR/AppImage/SMU.png"

cmake -S "$ROOT_DIR" -B "$BUILD_DIR" \
  -DCMAKE_BUILD_TYPE="${CMAKE_BUILD_TYPE:-Release}" \
  -DSMU_BUNDLE_SDL3=ON \
  -DSMU_LINK_SDL3_STATIC="${SMU_LINK_SDL3_STATIC:-OFF}" \
  "$@"

cmake --build "$BUILD_DIR" --target suspend --parallel

rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/lib"
mkdir -p "$APPDIR/usr/share/spencer-macro-utilities/scripts"
mkdir -p "$APPDIR/usr/share/doc/spencer-macro-utilities"
mkdir -p "$APPDIR/scripts"

cp "$BUILD_DIR/suspend" "$APPDIR/usr/bin/suspend"
cp "$ROOT_DIR/AppImage/AppRun" "$APPDIR/AppRun"
chmod 755 "$APPDIR/AppRun" "$APPDIR/usr/bin/suspend"

cp -R "$BUILD_DIR/assets" "$APPDIR/usr/bin/assets"

if compgen -G "$BUILD_DIR/lib/libSDL3.so*" >/dev/null; then
  cp -a "$BUILD_DIR"/lib/libSDL3.so* "$APPDIR/usr/lib/"
fi

cp "$ROOT_DIR/scripts/install_linux_permissions.sh" "$APPDIR/scripts/install_linux_permissions.sh"
cp "$ROOT_DIR/scripts/install_linux_permissions.sh" \
  "$APPDIR/usr/share/spencer-macro-utilities/scripts/install_linux_permissions.sh"
chmod 755 \
  "$APPDIR/scripts/install_linux_permissions.sh" \
  "$APPDIR/usr/share/spencer-macro-utilities/scripts/install_linux_permissions.sh"

cp "$ROOT_DIR/LINUX_SETUP.md" "$APPDIR/LINUX_SETUP.md"
cp "$ROOT_DIR/LINUX_SETUP.md" "$APPDIR/usr/share/doc/spencer-macro-utilities/LINUX_SETUP.md"
cp "$ROOT_DIR/LINUX_SETUP.md" "$APPDIR/usr/share/spencer-macro-utilities/LINUX_SETUP.md"

cat > "$APPDIR/spencer-macro-utilities.desktop" <<'EOF_DESKTOP'
[Desktop Entry]
Type=Application
Name=Spencer Macro Utilities
Exec=suspend
Icon=spencer-macro-utilities
Categories=Utility;
Terminal=false
EOF_DESKTOP

cp "$APP_ICON_SOURCE" "$APPDIR/spencer-macro-utilities.png"
cp "$APP_ICON_SOURCE" "$APPDIR/.DirIcon"

test -x "$APPDIR/AppRun"
test -x "$APPDIR/usr/bin/suspend"
test -d "$APPDIR/usr/bin/assets"
test -x "$APPDIR/scripts/install_linux_permissions.sh"
test -f "$APPDIR/LINUX_SETUP.md"
test -f "$APPDIR/spencer-macro-utilities.desktop"
test -f "$APPDIR/spencer-macro-utilities.png"

if ! command -v "$APPIMAGETOOL" >/dev/null 2>&1; then
  echo "ERROR: appimagetool was not found in PATH." >&2
  echo "Install appimagetool or set APPIMAGETOOL=/path/to/appimagetool." >&2
  exit 1
fi

"$APPIMAGETOOL" "$APPDIR" "$BUILD_DIR/$OUTPUT_NAME"
chmod 755 "$BUILD_DIR/$OUTPUT_NAME"

echo
echo "Built AppImage: $BUILD_DIR/$OUTPUT_NAME"

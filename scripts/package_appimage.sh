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

echo "Building nethelper (Go)..."
NETHELPER_BIN="$BUILD_DIR/nethelper"

(
  cd "$ROOT_DIR/platform/linux/nethelper"
  go mod download
  GOOS=linux GOARCH=amd64 CGO_ENABLED=0 go build \
    -o "$NETHELPER_BIN" \
    .
)

chmod 755 "$NETHELPER_BIN"

rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/lib"
mkdir -p "$APPDIR/usr/share/spencer-macro-utilities/scripts"
mkdir -p "$APPDIR/usr/share/doc/spencer-macro-utilities"
mkdir -p "$APPDIR/scripts"

cp "$BUILD_DIR/suspend" "$APPDIR/usr/bin/suspend"
cp "$NETHELPER_BIN" "$APPDIR/usr/bin/nethelper"

cp "$ROOT_DIR/AppImage/AppRun" "$APPDIR/AppRun"
chmod 755 "$APPDIR/AppRun" "$APPDIR/usr/bin/suspend" "$APPDIR/usr/bin/nethelper"

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
test -x "$APPDIR/usr/bin/nethelper"

test -d "$APPDIR/usr/bin/assets"
test -f "$APPDIR/usr/bin/assets/LSANS.TTF"
test -f "$APPDIR/usr/bin/assets/smu_icon.bmp"
test -f "$APPDIR/usr/bin/assets/macro_tutorials/fullgeardesync.png"
test -f "$APPDIR/usr/bin/assets/macro_tutorials/gear-clip.jpg"
test -f "$APPDIR/usr/bin/assets/macro_tutorials/laugh.jpg"
test -f "$APPDIR/usr/bin/assets/macro_tutorials/wallhop.jpg"
test -f "$APPDIR/usr/bin/assets/macro_tutorials/wallwalk.jpg"

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

(
  cd "$BUILD_DIR"
  rm -rf squashfs-root
  "./$OUTPUT_NAME" --appimage-extract >/dev/null

  test -f "squashfs-root/usr/bin/assets/LSANS.TTF"
  test -f "squashfs-root/usr/bin/assets/smu_icon.bmp"
  test -f "squashfs-root/usr/bin/assets/macro_tutorials/fullgeardesync.png"
  test -f "squashfs-root/usr/bin/assets/macro_tutorials/gear-clip.jpg"
  test -f "squashfs-root/usr/bin/assets/macro_tutorials/laugh.jpg"
  test -f "squashfs-root/usr/bin/assets/macro_tutorials/wallhop.jpg"
  test -f "squashfs-root/usr/bin/assets/macro_tutorials/wallwalk.jpg"

  test -x "squashfs-root/usr/bin/nethelper"

  rm -rf squashfs-root
)

echo
echo "Built AppImage: $BUILD_DIR/$OUTPUT_NAME"
echo "AppImage asset diagnostics passed."

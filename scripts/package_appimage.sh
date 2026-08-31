#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD_DIR="${SMU_BUILD_DIR:-"$ROOT_DIR/build/linux-appimage"}"
APPDIR="$BUILD_DIR/AppDir"
TOOLS_DIR="${SMU_TOOLS_DIR:-"$ROOT_DIR/build/tools"}"
APPIMAGETOOL="${APPIMAGETOOL:-}"
APPIMAGE_RUNTIME="${APPIMAGE_RUNTIME:-}"
OUTPUT_NAME="${SMU_APPIMAGE_NAME:-Spencer-Macro-Utilities-x86_64.AppImage}"
APP_ICON_SOURCE="$ROOT_DIR/AppImage/SMU.png"
PINNED_APPIMAGETOOL_URL="https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage"
PINNED_APPIMAGETOOL_SHA256="ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0"
# AppImage/type2-runtime commit 75849dce7cc37e4319b633df1f116ca895c71a12.
# The asset API URL remains tied to this exact GitHub asset even when the
# project's mutable "continuous" release is updated.
PINNED_RUNTIME_URL="https://api.github.com/repos/AppImage/type2-runtime/releases/assets/456065460"
PINNED_RUNTIME_SHA256="1cc49bcf1e2ccd593c379adb17c9f85a36d619088296504de95b1d06215aebbf"

if [[ "$(uname -m)" != "x86_64" ]]; then
  echo "ERROR: the AppImage release target currently supports x86_64 Linux only." >&2
  echo "Use the Nix flake for native aarch64 Linux builds." >&2
  exit 1
fi

resolve_appimagetool() {
  if [[ -n "$APPIMAGETOOL" ]]; then
    return
  fi

  if command -v appimagetool >/dev/null 2>&1; then
    APPIMAGETOOL="$(command -v appimagetool)"
    return
  fi

  if ! command -v curl >/dev/null 2>&1; then
    echo "ERROR: curl is required to download appimagetool automatically." >&2
    exit 1
  fi

  mkdir -p "$TOOLS_DIR"
  APPIMAGETOOL="$TOOLS_DIR/appimagetool-x86_64.AppImage"
  if [[ -e "$APPIMAGETOOL" ]]; then
    if ! printf '%s  %s\n' "$PINNED_APPIMAGETOOL_SHA256" "$APPIMAGETOOL" |
      sha256sum --check --status; then
      echo "ERROR: cached appimagetool failed its pinned SHA-256 check: $APPIMAGETOOL" >&2
      echo "Remove that file and rerun the build to download the verified 1.9.1 tool." >&2
      exit 1
    fi
  else
    local download_path="$APPIMAGETOOL.download"
    echo "Downloading the official appimagetool 1.9.1 build..."
    curl --fail --location --retry 3 \
      "$PINNED_APPIMAGETOOL_URL" \
      --output "$download_path"
    if ! printf '%s  %s\n' "$PINNED_APPIMAGETOOL_SHA256" "$download_path" |
      sha256sum --check --status; then
      echo "ERROR: downloaded appimagetool failed its pinned SHA-256 check." >&2
      exit 1
    fi
    chmod 755 "$download_path"
    mv -f "$download_path" "$APPIMAGETOOL"
  fi
}

resolve_runtime() {
  if [[ -z "$APPIMAGE_RUNTIME" ]]; then
    mkdir -p "$TOOLS_DIR"
    APPIMAGE_RUNTIME="$TOOLS_DIR/type2-runtime-75849dce-x86_64"
  fi

  if [[ -e "$APPIMAGE_RUNTIME" ]]; then
    if ! printf '%s  %s\n' "$PINNED_RUNTIME_SHA256" "$APPIMAGE_RUNTIME" |
      sha256sum --check --status; then
      echo "ERROR: cached AppImage runtime failed its pinned SHA-256 check: $APPIMAGE_RUNTIME" >&2
      echo "Remove that file and rerun the build to download the verified runtime." >&2
      exit 1
    fi
    return
  fi

  if ! command -v curl >/dev/null 2>&1; then
    echo "ERROR: curl is required to download the pinned AppImage runtime." >&2
    exit 1
  fi

  local download_path="$APPIMAGE_RUNTIME.download"
  echo "Downloading the pinned official AppImage x86_64 runtime..."
  curl --fail --location --retry 3 \
    --header "Accept: application/octet-stream" \
    "$PINNED_RUNTIME_URL" \
    --output "$download_path"
  if ! printf '%s  %s\n' "$PINNED_RUNTIME_SHA256" "$download_path" |
    sha256sum --check --status; then
    echo "ERROR: downloaded AppImage runtime failed its pinned SHA-256 check." >&2
    exit 1
  fi
  mv -f "$download_path" "$APPIMAGE_RUNTIME"
}

resolve_appimagetool
resolve_runtime

cmake -S "$ROOT_DIR" -B "$BUILD_DIR" \
  -DCMAKE_BUILD_TYPE="${CMAKE_BUILD_TYPE:-Release}" \
  -DSMU_BUNDLE_SDL3=ON \
  -DSMU_LINK_SDL3_STATIC="${SMU_LINK_SDL3_STATIC:-OFF}" \
  -DSMU_REQUIRE_WAYLAND_SCREENCAST=ON \
  -DSMU_ENABLE_SOURCE_TREE_FALLBACK=OFF \
  "$@"

cmake --build "$BUILD_DIR" --target spencer_macro_utilities --parallel

echo "Building nethelper (Go)..."
NETHELPER_BIN="$BUILD_DIR/nethelper"

(
  cd "$ROOT_DIR/platform/linux/nethelper"
  GOOS=linux CGO_ENABLED=0 go build \
    -mod=vendor \
    -trimpath \
    -buildvcs=false \
    -ldflags="-s -w" \
    -o "$NETHELPER_BIN" \
    .
)

chmod 755 "$NETHELPER_BIN"

rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/lib"
mkdir -p "$APPDIR/usr/share/spencer-macro-utilities/scripts"
mkdir -p "$APPDIR/usr/share/doc/spencer-macro-utilities"
mkdir -p "$APPDIR/usr/share/doc/spencer-macro-utilities/licenses"
mkdir -p "$APPDIR/scripts"
mkdir -p "$APPDIR/licenses"

cp "$BUILD_DIR/Spencer-Macro-Utilities" "$APPDIR/usr/bin/Spencer-Macro-Utilities"
cp "$NETHELPER_BIN" "$APPDIR/usr/bin/nethelper"

cp "$ROOT_DIR/AppImage/AppRun" "$APPDIR/AppRun"
chmod 755 "$APPDIR/AppRun" "$APPDIR/usr/bin/Spencer-Macro-Utilities" "$APPDIR/usr/bin/nethelper"

cp -R "$BUILD_DIR/assets" "$APPDIR/usr/bin/assets"

if compgen -G "$BUILD_DIR/lib/libSDL3.so*" >/dev/null; then
  cp -a "$BUILD_DIR"/lib/libSDL3.so* "$APPDIR/usr/lib/"
fi

# libei is a comparatively new desktop library and is not present on every
# supported Linux distribution. The release build links it for modern
# Wayland RemoteDesktop/EIS pointer support, so bundle its ABI-stable SONAME
# instead of turning it into a host requirement for the AppImage.
LIBEI_LIBDIR="$(pkg-config --variable=libdir libei-1.0)"
LIBEI_LIBRARY="$LIBEI_LIBDIR/libei.so.1"
if [[ ! -e "$LIBEI_LIBRARY" ]]; then
  echo "ERROR: libei.so.1 was not found in pkg-config's libei library directory: $LIBEI_LIBDIR" >&2
  exit 1
fi
cp -L "$LIBEI_LIBRARY" "$APPDIR/usr/lib/libei.so.1"

cp "$ROOT_DIR/scripts/install_linux_permissions.sh" "$APPDIR/scripts/install_linux_permissions.sh"
cp "$ROOT_DIR/scripts/install_linux_permissions.sh" \
  "$APPDIR/usr/share/spencer-macro-utilities/scripts/install_linux_permissions.sh"
chmod 755 \
  "$APPDIR/scripts/install_linux_permissions.sh" \
  "$APPDIR/usr/share/spencer-macro-utilities/scripts/install_linux_permissions.sh"

cp "$ROOT_DIR/LINUX_SETUP.md" "$APPDIR/LINUX_SETUP.md"
cp "$ROOT_DIR/LINUX_SETUP.md" "$APPDIR/usr/share/doc/spencer-macro-utilities/LINUX_SETUP.md"
cp "$ROOT_DIR/LINUX_SETUP.md" "$APPDIR/usr/share/spencer-macro-utilities/LINUX_SETUP.md"
cp "$ROOT_DIR/LICENSE" "$APPDIR/LICENSE"
cp "$ROOT_DIR/PRIVACY.md" "$APPDIR/PRIVACY.md"
cp "$ROOT_DIR/THIRD_PARTY_NOTICES.md" "$APPDIR/THIRD_PARTY_NOTICES.md"
cp "$ROOT_DIR/third_party/SDL/LICENSE.txt" "$APPDIR/licenses/SDL.txt"
cp "$ROOT_DIR/third_party/appimage-runtime/LICENSE" \
  "$APPDIR/licenses/AppImage-type2-runtime.txt"
cp "$ROOT_DIR/third_party/libei/LICENSE.txt" "$APPDIR/licenses/libei-MIT.txt"
cp "$ROOT_DIR/platform/linux/nethelper/vendor/github.com/coreos/go-iptables/LICENSE" \
  "$APPDIR/licenses/go-iptables-LICENSE.txt"
cp "$ROOT_DIR/platform/linux/nethelper/vendor/github.com/coreos/go-iptables/NOTICE" \
  "$APPDIR/licenses/go-iptables-NOTICE.txt"
cp "$ROOT_DIR/third_party/windivert/LICENSE-LGPL-3.0.txt" \
  "$APPDIR/licenses/WinDivert-LGPL-3.0.txt"
cp "$ROOT_DIR/LICENSE" "$APPDIR/usr/share/doc/spencer-macro-utilities/LICENSE"
cp "$ROOT_DIR/PRIVACY.md" "$APPDIR/usr/share/doc/spencer-macro-utilities/PRIVACY.md"
cp "$ROOT_DIR/THIRD_PARTY_NOTICES.md" \
  "$APPDIR/usr/share/doc/spencer-macro-utilities/THIRD_PARTY_NOTICES.md"
cp "$ROOT_DIR/third_party/SDL/LICENSE.txt" \
  "$APPDIR/usr/share/doc/spencer-macro-utilities/licenses/SDL.txt"
cp "$ROOT_DIR/third_party/appimage-runtime/LICENSE" \
  "$APPDIR/usr/share/doc/spencer-macro-utilities/licenses/AppImage-type2-runtime.txt"
cp "$ROOT_DIR/third_party/libei/LICENSE.txt" \
  "$APPDIR/usr/share/doc/spencer-macro-utilities/licenses/libei-MIT.txt"
cp "$ROOT_DIR/platform/linux/nethelper/vendor/github.com/coreos/go-iptables/LICENSE" \
  "$APPDIR/usr/share/doc/spencer-macro-utilities/licenses/go-iptables-LICENSE.txt"
cp "$ROOT_DIR/platform/linux/nethelper/vendor/github.com/coreos/go-iptables/NOTICE" \
  "$APPDIR/usr/share/doc/spencer-macro-utilities/licenses/go-iptables-NOTICE.txt"
cp "$ROOT_DIR/third_party/windivert/LICENSE-LGPL-3.0.txt" \
  "$APPDIR/usr/share/doc/spencer-macro-utilities/licenses/WinDivert-LGPL-3.0.txt"

cat > "$APPDIR/spencer-macro-utilities.desktop" <<'EOF_DESKTOP'
[Desktop Entry]
Type=Application
Name=Spencer Macro Utilities
Exec=Spencer-Macro-Utilities
Icon=spencer-macro-utilities
Categories=Utility;
Terminal=false
EOF_DESKTOP

cp "$APP_ICON_SOURCE" "$APPDIR/spencer-macro-utilities.png"
cp "$APP_ICON_SOURCE" "$APPDIR/.DirIcon"

test -x "$APPDIR/AppRun"
test -x "$APPDIR/usr/bin/Spencer-Macro-Utilities"
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
test -f "$APPDIR/LICENSE"
test -f "$APPDIR/PRIVACY.md"
test -f "$APPDIR/THIRD_PARTY_NOTICES.md"
test -f "$APPDIR/licenses/SDL.txt"
test -f "$APPDIR/licenses/AppImage-type2-runtime.txt"
test -f "$APPDIR/licenses/libei-MIT.txt"
test -f "$APPDIR/usr/lib/libei.so.1"
test -f "$APPDIR/licenses/go-iptables-LICENSE.txt"
test -f "$APPDIR/licenses/go-iptables-NOTICE.txt"
test -f "$APPDIR/licenses/WinDivert-LGPL-3.0.txt"
test -f "$APPDIR/spencer-macro-utilities.desktop"
test -f "$APPDIR/spencer-macro-utilities.png"

if [[ "$APPIMAGETOOL" == */* ]]; then
  if [[ ! -x "$APPIMAGETOOL" ]]; then
    echo "ERROR: APPIMAGETOOL is not executable: $APPIMAGETOOL" >&2
    exit 1
  fi
elif ! command -v "$APPIMAGETOOL" >/dev/null 2>&1; then
  echo "ERROR: appimagetool was not found in PATH." >&2
  echo "Install appimagetool or set APPIMAGETOOL=/path/to/appimagetool." >&2
  exit 1
fi

ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 \
  "$APPIMAGETOOL" \
  --runtime-file "$APPIMAGE_RUNTIME" \
  "$APPDIR" \
  "$BUILD_DIR/$OUTPUT_NAME"
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
  test -f "squashfs-root/THIRD_PARTY_NOTICES.md"
  test -f "squashfs-root/PRIVACY.md"
  test -f "squashfs-root/licenses/SDL.txt"
  test -f "squashfs-root/licenses/AppImage-type2-runtime.txt"
  test -f "squashfs-root/licenses/libei-MIT.txt"
  test -f "squashfs-root/usr/lib/libei.so.1"
  test -f "squashfs-root/licenses/go-iptables-LICENSE.txt"
  test -f "squashfs-root/licenses/go-iptables-NOTICE.txt"
  test -f "squashfs-root/licenses/WinDivert-LGPL-3.0.txt"

  missing_deps="$(
    LD_LIBRARY_PATH="$PWD/squashfs-root/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
      ldd squashfs-root/usr/bin/Spencer-Macro-Utilities | grep 'not found' || true
  )"
  if [[ -n "$missing_deps" ]]; then
    echo "ERROR: AppImage executable has unresolved shared-library dependencies:" >&2
    echo "$missing_deps" >&2
    exit 1
  fi

  rm -rf squashfs-root
)

echo
echo "Built AppImage: $BUILD_DIR/$OUTPUT_NAME"
echo "AppImage asset diagnostics passed."

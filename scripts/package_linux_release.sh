#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$(tr -d '[:space:]' < "$ROOT_DIR/version")"
BUILD_DIR="${SMU_BUILD_DIR:-"$ROOT_DIR/build/linux-release"}"
DIST_DIR="${SMU_DIST_DIR:-"$ROOT_DIR/dist/release"}"
PACKAGE_WORK_DIR="$BUILD_DIR/nfpm"
BUNDLE_DIR="$BUILD_DIR/bundle"
APPIMAGE_NAME="Spencer-Macro-Utilities-V${VERSION}-Linux-x86_64.AppImage"
DEB_NAME="spencer-macro-utilities_${VERSION}_amd64.deb"
RPM_NAME="spencer-macro-utilities-${VERSION}-1.x86_64.rpm"
PORTABLE_NAME="Spencer-Macro-Utilities-V${VERSION}-Linux-x86_64-portable.tar.gz"
BUNDLE_NAME="Spencer-Macro-Utilities-V${VERSION}-Linux-x86_64.zip"
ARCHIVE_EPOCH="${SOURCE_DATE_EPOCH:-0}"
if [[ ! "$ARCHIVE_EPOCH" =~ ^[0-9]+$ ]]; then
  echo "ERROR: SOURCE_DATE_EPOCH must be a non-negative integer." >&2
  exit 1
fi
ZIP_EPOCH="$ARCHIVE_EPOCH"
if (( ZIP_EPOCH < 315532800 )); then
  # ZIP timestamps cannot represent dates before 1980-01-01.
  ZIP_EPOCH=315532800
fi

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "ERROR: $1 is required to build the Linux release bundle." >&2
    exit 1
  fi
}

case "$VERSION" in
  [0-9]*.[0-9]*.[0-9]*) ;;
  *)
    echo "ERROR: version must contain MAJOR.MINOR.PATCH." >&2
    exit 1
    ;;
esac

if [[ "$(uname -m)" != "x86_64" ]]; then
  echo "ERROR: the all-in-one Linux release bundle currently targets x86_64." >&2
  echo "Use the Nix flake for native aarch64 Linux builds." >&2
  exit 1
fi

require_command cmake
require_command go
require_command nfpm
require_command tar
require_command unzip
require_command zip

SMU_BUILD_DIR="$BUILD_DIR" \
SMU_APPIMAGE_NAME="$APPIMAGE_NAME" \
  bash "$ROOT_DIR/scripts/package_appimage.sh" "$@"

cmake --build "$BUILD_DIR" --target package-linux-dir --parallel

cmake -E rm -rf "$PACKAGE_WORK_DIR" "$BUNDLE_DIR"
cmake -E make_directory \
  "$PACKAGE_WORK_DIR/staged" \
  "$PACKAGE_WORK_DIR/AppImage" \
  "$BUNDLE_DIR/licenses" \
  "$BUNDLE_DIR" \
  "$DIST_DIR"

cp -a "$BUILD_DIR/AppDir/usr" "$PACKAGE_WORK_DIR/staged/usr"
cp \
  "$ROOT_DIR/AppImage/SMU.png" \
  "$ROOT_DIR/AppImage/launcher.sh" \
  "$ROOT_DIR/AppImage/postinstall.sh" \
  "$ROOT_DIR/AppImage/postremove.sh" \
  "$ROOT_DIR/AppImage/SMU.desktop" \
  "$PACKAGE_WORK_DIR/AppImage/"
mv \
  "$PACKAGE_WORK_DIR/AppImage/SMU.desktop" \
  "$PACKAGE_WORK_DIR/AppImage/spencer-macro-utilities.desktop"

(
  cd "$PACKAGE_WORK_DIR"
  nfpm pkg \
    --packager deb \
    --config "$ROOT_DIR/AppImage/nfpm.yaml" \
    --target "$BUNDLE_DIR/$DEB_NAME"
  nfpm pkg \
    --packager rpm \
    --config "$ROOT_DIR/AppImage/nfpm.yaml" \
    --target "$BUNDLE_DIR/$RPM_NAME"
)

cp "$BUILD_DIR/$APPIMAGE_NAME" "$BUNDLE_DIR/$APPIMAGE_NAME"
cp "$ROOT_DIR/LINUX_SETUP.md" "$BUNDLE_DIR/LINUX_SETUP.md"
cp "$ROOT_DIR/LICENSE" "$BUNDLE_DIR/LICENSE"
cp "$ROOT_DIR/PRIVACY.md" "$BUNDLE_DIR/PRIVACY.md"
cp "$ROOT_DIR/THIRD_PARTY_NOTICES.md" "$BUNDLE_DIR/THIRD_PARTY_NOTICES.md"
cp "$ROOT_DIR/third_party/SDL/LICENSE.txt" "$BUNDLE_DIR/licenses/SDL.txt"
cp "$ROOT_DIR/third_party/appimage-runtime/LICENSE" \
  "$BUNDLE_DIR/licenses/AppImage-type2-runtime.txt"
cp "$ROOT_DIR/platform/linux/nethelper/vendor/github.com/coreos/go-iptables/LICENSE" \
  "$BUNDLE_DIR/licenses/go-iptables-LICENSE.txt"
cp "$ROOT_DIR/platform/linux/nethelper/vendor/github.com/coreos/go-iptables/NOTICE" \
  "$BUNDLE_DIR/licenses/go-iptables-NOTICE.txt"
cp "$ROOT_DIR/third_party/windivert/LICENSE-LGPL-3.0.txt" \
  "$BUNDLE_DIR/licenses/WinDivert-LGPL-3.0.txt"
find "$BUILD_DIR/SpencerMacroUtilities" -exec touch -h -d "@$ARCHIVE_EPOCH" {} +
tar \
  --sort=name \
  --mtime="@$ARCHIVE_EPOCH" \
  --owner=0 \
  --group=0 \
  --numeric-owner \
  -C "$BUILD_DIR" \
  -czf "$BUNDLE_DIR/$PORTABLE_NAME" \
  SpencerMacroUtilities

(
  cd "$BUNDLE_DIR"
  sha256sum \
    "$APPIMAGE_NAME" \
    "$DEB_NAME" \
    "$RPM_NAME" \
    "$PORTABLE_NAME" \
    > SHA256SUMS
)

cmake -E rm -f "$DIST_DIR/$BUNDLE_NAME"
find "$BUNDLE_DIR" -exec touch -h -d "@$ZIP_EPOCH" {} +
(
  cd "$BUNDLE_DIR"
  zip -X -9 "$DIST_DIR/$BUNDLE_NAME" \
    "$APPIMAGE_NAME" \
    "$DEB_NAME" \
    "$RPM_NAME" \
    "$PORTABLE_NAME" \
    LINUX_SETUP.md \
    LICENSE \
    PRIVACY.md \
    THIRD_PARTY_NOTICES.md \
    licenses/SDL.txt \
    licenses/AppImage-type2-runtime.txt \
    licenses/go-iptables-LICENSE.txt \
    licenses/go-iptables-NOTICE.txt \
    licenses/WinDivert-LGPL-3.0.txt \
    SHA256SUMS
)

unzip -t "$DIST_DIR/$BUNDLE_NAME"

echo
echo "Built Linux release bundle: $DIST_DIR/$BUNDLE_NAME"
echo "The ZIP contains AppImage, deb, rpm, portable tarball, setup, license notices, and checksums."

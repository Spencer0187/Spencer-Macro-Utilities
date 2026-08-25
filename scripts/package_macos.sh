#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PRESET="${SMU_MACOS_PRESET:-macos-universal-release}"
BUILD_DIR="$ROOT_DIR/out/build/$PRESET"
BUILD_TARGET="spencer_macro_utilities"
BUILT_APP_PATH="$BUILD_DIR/Spencer-Macro-Utilities.app"
BINARY_PATH="$BUILT_APP_PATH/Contents/MacOS/Spencer-Macro-Utilities"
PUBLIC_APP_NAME="Spencer Macro Utilities.app"
STAGE_DIR="${SMU_MACOS_STAGE_DIR:-"$BUILD_DIR/package-macos"}"
STAGED_APP="$STAGE_DIR/$PUBLIC_APP_NAME"
ZIP_PATH="$STAGE_DIR/SpencerMacroUtilities-macOS-universal.zip"
DMG_PATH="$STAGE_DIR/SpencerMacroUtilities-macOS-universal.dmg"
DMG_STAGING_DIR="$STAGE_DIR/dmg-root"
SMU_MACOS_ADHOC_SIGN="${SMU_MACOS_ADHOC_SIGN:-ON}"
SMU_MACOS_SIGN_IDENTITY="${SMU_MACOS_SIGN_IDENTITY:--}"

create_dmg() {
  if ! command -v hdiutil >/dev/null 2>&1; then
    echo "hdiutil was not found; zip artifact only." >&2
    return
  fi

  # Once DMG creation is available, packaging is strict: Finder/AppleScript
  # layout failures must fail the package rather than publishing a broken DMG.
  bash "$ROOT_DIR/scripts/create_macos_dmg.sh" "$STAGED_APP" "$DMG_PATH" "$DMG_STAGING_DIR"
}

cmake --preset "$PRESET" "$@"
cmake --build --preset "$PRESET" --target "$BUILD_TARGET" --parallel

test -d "$BUILT_APP_PATH"
test -x "$BINARY_PATH"
test -f "$BUILT_APP_PATH/Contents/Resources/assets/LSANS.TTF"
test -f "$BUILT_APP_PATH/Contents/Resources/assets/smu_icon.bmp"
test -d "$BUILT_APP_PATH/Contents/Resources/assets/macro_tutorials"
test -f "$BUILT_APP_PATH/Contents/Resources/smu_icon.icns"
test -f "$BUILT_APP_PATH/Contents/Resources/LICENSE"
test -f "$BUILT_APP_PATH/Contents/Resources/PRIVACY.md"
test -f "$BUILT_APP_PATH/Contents/Resources/THIRD_PARTY_NOTICES.md"
test -f "$BUILT_APP_PATH/Contents/Resources/licenses/SDL.txt"
test -f "$BUILT_APP_PATH/Contents/Resources/licenses/AppImage-type2-runtime.txt"
test -f "$BUILT_APP_PATH/Contents/Resources/licenses/go-iptables-LICENSE.txt"
test -f "$BUILT_APP_PATH/Contents/Resources/licenses/go-iptables-NOTICE.txt"
test -f "$BUILT_APP_PATH/Contents/Resources/licenses/WinDivert-LGPL-3.0.txt"

echo
lipo -info "$BINARY_PATH"
ARCHS="$(lipo -archs "$BINARY_PATH")"
case " $ARCHS " in
  *" x86_64 "*) ;;
  *)
    echo "ERROR: $BINARY_PATH does not contain an x86_64 slice: $ARCHS" >&2
    exit 1
    ;;
esac
case " $ARCHS " in
  *" arm64 "*) ;;
  *)
    echo "ERROR: $BINARY_PATH does not contain an arm64 slice: $ARCHS" >&2
    exit 1
    ;;
esac

rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR"
ditto "$BUILT_APP_PATH" "$STAGED_APP"
touch "$STAGED_APP"

if [[ "$SMU_MACOS_ADHOC_SIGN" == "ON" ]]; then
  # The default identity is ad-hoc ("-"). Official unsigned GitHub macOS
  # releases should set SMU_MACOS_SIGN_IDENTITY to the stable self-signed
  # "SMU macOS Release" certificate so TCC permissions survive app updates.
  echo "Signing macOS app with identity: $SMU_MACOS_SIGN_IDENTITY"
  codesign --force --deep --sign "$SMU_MACOS_SIGN_IDENTITY" "$STAGED_APP"
  codesign --verify --deep --strict --verbose=2 "$STAGED_APP"
fi

ditto -c -k --sequesterRsrc --keepParent "$STAGED_APP" "$ZIP_PATH"

create_dmg

echo
echo "macOS app: $STAGED_APP"
echo "macOS zip: $ZIP_PATH"
if [[ -f "$DMG_PATH" ]]; then
  echo "macOS dmg: $DMG_PATH"
fi

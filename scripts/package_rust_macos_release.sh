#!/usr/bin/env bash
set -euo pipefail

# The public certificate-free macOS distribution is ad-hoc-signed to satisfy
# macOS code integrity. This is not Developer ID signing or notarization.
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="${SMU_MACOS_RUST_TARGET_DIR:-$ROOT_DIR/out/build/rust-macos-target}"
BUNDLE_DIR="${SMU_MACOS_RUST_BUNDLE_DIR:-$ROOT_DIR/out/build/rust-macos-app}"
STAGE_DIR="${SMU_MACOS_STAGE_DIR:-$ROOT_DIR/out/build/macos-universal-release/package-macos}"
IDENTITY="${SMU_MACOS_SIGN_IDENTITY:--}"
APP_NAME="Spencer Macro Utilities.app"
BUNDLE="$BUNDLE_DIR/$APP_NAME"
ZIP="$STAGE_DIR/SpencerMacroUtilities-macOS-universal.zip"
DMG="$STAGE_DIR/SpencerMacroUtilities-macOS-universal.dmg"

python3 "$ROOT_DIR/scripts/stage_rust_macos_app.py" \
  --root "$ROOT_DIR" \
  --target-dir "$TARGET_DIR" \
  --bundle "$BUNDLE" \
  --profile release \
  --sign-identity "$IDENTITY"

# Never package a partial .app; the staging script validates all runtime slices.
rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR"
ditto "$BUNDLE" "$STAGE_DIR/$APP_NAME"
codesign --verify --deep --strict --verbose=2 "$STAGE_DIR/$APP_NAME"
ditto -c -k --sequesterRsrc --keepParent "$STAGE_DIR/$APP_NAME" "$ZIP"
SMU_MACOS_DMG_LAYOUT="${SMU_MACOS_DMG_LAYOUT:-headless}" bash "$ROOT_DIR/scripts/create_macos_dmg.sh" \
  "$STAGE_DIR/$APP_NAME" "$DMG" "$STAGE_DIR/dmg-root"

unzip -t "$ZIP"
hdiutil verify "$DMG"
printf 'Rust macOS release app: %s\nZIP: %s\nDMG: %s\n' "$STAGE_DIR/$APP_NAME" "$ZIP" "$DMG"

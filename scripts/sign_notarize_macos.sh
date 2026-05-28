#!/usr/bin/env bash
set -euo pipefail

# Optional Developer ID path only. Normal public macOS releases for this
# project are non-notarized and use scripts/package_macos.sh with the stable
# self-signed "SMU macOS Release" certificate.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEFAULT_STAGE_DIR="$ROOT_DIR/out/build/macos-universal-release/package-macos"
STAGE_DIR="${SMU_MACOS_STAGE_DIR:-"$DEFAULT_STAGE_DIR"}"
APP_PATH="${1:-"$STAGE_DIR/suspend.app"}"
ZIP_PATH="$STAGE_DIR/SpencerMacroUtilities-macOS-universal.zip"
DMG_PATH="$STAGE_DIR/SpencerMacroUtilities-macOS-universal.dmg"
NOTARY_ZIP="$STAGE_DIR/SpencerMacroUtilities-macOS-universal-notary.zip"
DMG_STAGING_DIR="$STAGE_DIR/dmg-root"

: "${DEVELOPER_ID_APPLICATION:?Set DEVELOPER_ID_APPLICATION to the Developer ID Application signing identity.}"
if [[ -z "${NOTARYTOOL_KEYCHAIN_PROFILE:-}" ]]; then
  : "${APPLE_ID:?Set APPLE_ID or NOTARYTOOL_KEYCHAIN_PROFILE for notarization.}"
  : "${APPLE_TEAM_ID:?Set APPLE_TEAM_ID or NOTARYTOOL_KEYCHAIN_PROFILE for notarization.}"
  : "${APPLE_APP_SPECIFIC_PASSWORD:?Set APPLE_APP_SPECIFIC_PASSWORD or NOTARYTOOL_KEYCHAIN_PROFILE for notarization.}"
fi

test -d "$APP_PATH"
mkdir -p "$STAGE_DIR"

# TODO: add explicit entitlements here if a future macOS backend needs them.
codesign \
  --force \
  --deep \
  --options runtime \
  --timestamp \
  --sign "$DEVELOPER_ID_APPLICATION" \
  "$APP_PATH"
codesign --verify --deep --strict --verbose=2 "$APP_PATH"

rm -f "$NOTARY_ZIP"
ditto -c -k --sequesterRsrc --keepParent "$APP_PATH" "$NOTARY_ZIP"

NOTARY_ARGS=()
if [[ -n "${NOTARYTOOL_KEYCHAIN_PROFILE:-}" ]]; then
  NOTARY_ARGS+=(--keychain-profile "$NOTARYTOOL_KEYCHAIN_PROFILE")
else
  NOTARY_ARGS+=(
    --apple-id "$APPLE_ID"
    --team-id "$APPLE_TEAM_ID"
    --password "$APPLE_APP_SPECIFIC_PASSWORD"
  )
fi

xcrun notarytool submit "$NOTARY_ZIP" --wait "${NOTARY_ARGS[@]}"
xcrun stapler staple "$APP_PATH"
xcrun stapler validate "$APP_PATH"

rm -f "$ZIP_PATH"
ditto -c -k --sequesterRsrc --keepParent "$APP_PATH" "$ZIP_PATH"
if command -v hdiutil >/dev/null 2>&1; then
  bash "$ROOT_DIR/scripts/create_macos_dmg.sh" "$APP_PATH" "$DMG_PATH" "$DMG_STAGING_DIR"
fi

echo "Notarized macOS app: $APP_PATH"
echo "Notarized macOS zip: $ZIP_PATH"
if [[ -f "$DMG_PATH" ]]; then
  echo "Notarized macOS dmg: $DMG_PATH"
fi

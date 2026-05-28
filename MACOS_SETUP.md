# macOS Setup

SMU builds a normal macOS app bundle without an App Store sandbox. Public GitHub macOS releases are not Apple Developer ID signed and are not notarized. They are signed with one stable self-signed project certificate so macOS privacy permissions can survive updates more reliably than ad-hoc signatures.

## Build

Install the Xcode Command Line Tools:

```bash
xcode-select --install
```

Build a native Release app bundle:

```bash
cmake --preset macos-release
cmake --build --preset macos-release --target suspend
open out/build/macos-release/suspend.app
```

Build the single universal Intel and Apple Silicon release package:

```bash
bash scripts/package_macos.sh
```

The package script configures `macos-universal-release`, builds `suspend.app`, checks that `Contents/MacOS/suspend` contains both `arm64` and `x86_64` slices with `lipo`, verifies bundled runtime assets, and stages a zip plus a dmg when `hdiutil` is available.
Runtime assets are bundled in `suspend.app/Contents/Resources/assets`, and the dmg includes an `Applications` alias plus a drag-to-install arrow background.

## Permissions

SMU cannot grant macOS privacy permissions itself. Use the in-app macOS permission setup screen and approve the app in System Settings.

- Accessibility is required to send global keyboard and mouse input created by macros.
- Screen Recording is required for Lua `getPixelColor()` and `getPixelRect()` display reads.
- Automation and Apple Events are not used by this backend.

After granting a permission, return to SMU and click Restart & Check Permissions.

If SMU says a permission is missing even though System Settings shows an enabled old `suspend` or SMU entry, click Reset macOS Permission Entries in the in-app permission setup screen. SMU will run the supported `tccutil reset Accessibility com.spencer0187.smu` and `tccutil reset ScreenCapture com.spencer0187.smu` commands, then you can approve the current app again and restart.

## Installing GitHub Releases

Because releases are not Apple Developer ID signed or notarized, macOS will warn on first launch.

1. Download the universal macOS zip or dmg from GitHub Releases.
2. Drag `suspend.app` to Applications.
3. Try to open `suspend.app`.
4. Open System Settings > Privacy & Security and choose Open Anyway for SMU.
5. Grant Accessibility and Screen Recording from the in-app setup flow.
6. Click Restart & Check Permissions.

Do not disable Gatekeeper globally. If quarantine gets stuck after the Open Anyway flow, use this targeted fallback:

```bash
xattr -dr com.apple.quarantine /Applications/suspend.app
```

## Settings

macOS builds save user settings under `~/Library/Application Support/Spencer Macro Utilities/SMCSettings.json`. The app does not write settings into the `.app` bundle or a mounted dmg.

## Packaging And Signing

Local builds are signed by default so the app bundle has a complete macOS identity and sealed resources. The default identity is `-`, which means ad-hoc signing:

```bash
bash scripts/package_macos.sh
```

Ad-hoc signing is enough for packaging validation, but macOS TCC can bind privacy permissions to the changing code hash. If you rebuild or update from ad-hoc artifacts, Accessibility and Screen Recording may need to be reset and re-approved even when System Settings still shows an enabled stale `suspend` entry.

Official unsigned GitHub macOS releases should use one long-lived self-signed code-signing identity named `SMU macOS Release`:

```bash
export SMU_MACOS_RELEASE_CERTIFICATE_PASSWORD="choose-a-strong-password"
SMU_IMPORT_MACOS_RELEASE_CERT=ON bash scripts/create_macos_release_certificate.sh
SMU_MACOS_SIGN_IDENTITY="SMU macOS Release" bash scripts/package_macos.sh
```

Store the generated `.p12` contents and password in GitHub Actions secrets:

```text
SMU_MACOS_RELEASE_CERTIFICATE_BASE64
SMU_MACOS_RELEASE_CERTIFICATE_PASSWORD
SMU_MACOS_RELEASE_SIGN_IDENTITY
```

The release workflow imports this certificate and runs `scripts/package_macos.sh` with `SMU_MACOS_SIGN_IDENTITY="SMU macOS Release"`. Keep the private key stable; replacing it creates a new macOS privacy identity and may require users to approve permissions again.

For stable local permission testing with another local certificate:

```bash
cmake --preset macos-release -DSMU_MACOS_SIGN_IDENTITY="Apple Development: ..."
SMU_MACOS_SIGN_IDENTITY="Apple Development: ..." bash scripts/package_macos.sh
```

Developer ID signing, notarization, and stapling are optional if a maintainer later has an Apple Developer account:

```bash
export DEVELOPER_ID_APPLICATION="Developer ID Application: ..."
export APPLE_ID="..."
export APPLE_TEAM_ID="..."
export APPLE_APP_SPECIFIC_PASSWORD="..."
bash scripts/sign_notarize_macos.sh
```

`scripts/sign_notarize_macos.sh` also accepts `NOTARYTOOL_KEYCHAIN_PROFILE` instead of the Apple ID password variables. This is not required for normal GitHub releases.

Use `docs/macos_unsigned_release_notes.md` as the macOS section of each GitHub release note.

## Current Limits

- macOS lagswitch support is intentionally unavailable.
- Screen pixel reads fail with a clear error until macOS reports Screen Recording permission for the app.
- macOS auto-update can replace a writable installed `.app` bundle from the universal zip release asset. A copy launched directly from a mounted dmg can check for updates but cannot replace itself; drag it to Applications first.

# Releasing Spencer Macro Utilities

SMU keeps its version in the repository-root `version` file. The application, Windows resources, Linux packages, update metadata, and GitHub release names all read from that source.

## One-time repository setup

Configure these GitHub Actions secrets:

| Secret | Purpose |
| --- | --- |
| `SIGNPATH_API_TOKEN` | Submit the Windows executable to SignPath |
| `SIGNPATH_ORGANIZATION_ID` | Select the SignPath organization |
| `SIGNPATH_PROJECT_SLUG` | Select the SMU SignPath project |
| `SIGNPATH_SIGNING_POLICY` | Select the release signing policy |
| `SMU_MACOS_RELEASE_CERTIFICATE_BASE64` | Stable macOS release certificate in PKCS#12 format |
| `SMU_MACOS_RELEASE_CERTIFICATE_PASSWORD` | Password for that PKCS#12 file |
| `SMU_MACOS_RELEASE_SIGN_IDENTITY` | Optional identity override; defaults to `SMU macOS Release` |

The SignPath project's `initial` artifact configuration must accept `Spencer-Macro-Utilities.exe`. The release workflow grants the SignPath action read-only access to the source Actions artifact and verifies the returned Authenticode signature before packaging it. Release staging renames the signed binary to `Spencer-Macro-Utilities-V<version>-Windows.exe` without modifying its bytes.

Create the stable macOS certificate with:

```bash
bash scripts/create_macos_release_certificate.sh
```

Keep the exported certificate and password private. macOS automatic updates compare the new app's signing certificate with the currently installed app, so rotating this certificate requires a deliberate migration plan.

## Prepare a release

1. Bump the version:

   ```bash
   python3 scripts/version.py --bump minor
   ```

   An exact version also works:

   ```bash
   python3 scripts/version.py 3.4.0
   ```

2. Run **Actions -> Build Draft Release**, then write the release notes directly in the generated GitHub draft before publishing it. The draft starts with a placeholder and a generated comparison link.

3. Do **not** bump `.github/autoupdaterurl` after V3.4.0. It is intentionally frozen to the V3.4.0 `Spencer-Macro-Utilities-Windows.zip`. Keep the otherwise-ignored `?legacy={VERSION}` query placeholder: V3.0 rejects updater configuration that contains no `{VERSION}` token, while GitHub release downloads tolerate the resulting query parameter. The one-release root-level `suspend.exe` layout then lets V3.0/V3.1 clients cross onto the modern updater generation.

4. Validate synchronized metadata and local tooling:

   ```bash
   python3 scripts/version.py --check
   python3 -m unittest discover -s tests -p 'test_*.py'
   (cd platform/linux/nethelper && go test -mod=vendor ./... && go vet -mod=vendor ./...)
   cmake -S . -B build/release-check -DCMAKE_BUILD_TYPE=Release -DBUILD_TESTING=ON
   cmake --build build/release-check --parallel
   ctest --test-dir build/release-check --output-on-failure
   ```

5. Push the release commit, open **Actions -> Build Draft Release**, and run the workflow from that exact branch or commit.

## V3.4.0 one-release Windows migration bridge

V3.4.0 is the final automatic migration window for the pre-manifest Windows updater generations. There is no separate ugly bridge asset: for **V3.4.0 only**, the normal `Spencer-Macro-Utilities-Windows.zip` itself uses the historical root-level layout:

```text
Spencer-Macro-Utilities-Windows.zip
├── suspend.exe
├── README.md
├── LICENSE
├── PRIVACY.md
├── THIRD_PARTY_NOTICES.md
└── licenses/...
```

The `suspend.exe` bytes are the same signed V3.4.0 executable as the correctly named direct download. On startup, before any GUI/backend/macro initialization, V3.4.0 detects when its executable basename is exactly `suspend.exe`, copies itself to `Spencer-Macro-Utilities-V3.4.0-Windows.exe`, exits, deletes the legacy filename after Windows releases the image lock, and relaunches the correctly named executable.

`app/main_windows.cpp` contains an intentional compile-time tripwire requiring `SMU_VERSION_STRING == "3.4.0"`. **The V3.4.0 filename-migration block must be deleted when bumping to V3.4.1.** A Windows V3.4.1 build should fail until that cleanup is performed.

The compatibility requirements differ by old updater generation:

- V3.0/V3.1 read `.github/autoupdaterurl`, which is permanently pinned to the V3.4.0 `Spencer-Macro-Utilities-Windows.zip` with a compatibility `?legacy={VERSION}` query token. V3.0 requires that placeholder to exist; after substitution GitHub ignores the query value and serves the same release asset.
- V3.2 blindly chooses the first ZIP returned by the GitHub release asset API, so the V3.4.0 draft-release workflow uploads `Spencer-Macro-Utilities-Windows.zip` first and verifies that ordering.
- V3.3.x scores Windows ZIP filenames and selects `Spencer-Macro-Utilities-Windows.zip`; its root-level `suspend.exe` extraction therefore succeeds.

This does **not** make V3.2/V3.3 permanently bridgeable after newer releases become `/releases/latest`. The intended policy is to leave V3.4.0 as the latest release for a migration window (roughly a month). After V3.4.1 is published, dormant V3.2/V3.3 installs that never crossed the V3.4.0 window may need to update manually from GitHub.

Starting with V3.4.1, the same stable ZIP name switches to the final clean layout:

```text
Spencer-Macro-Utilities-Windows.zip
└── Spencer-Macro-Utilities/
    ├── Spencer-Macro-Utilities-V<version>-Windows.exe
    └── ...
```

No normal V3.4.1+ package should contain `suspend.exe`, and release asset ordering no longer matters to V3.4+ clients.

## Modern updater contract

V3.4+ uses `update-manifest.json` as the authoritative automatic-update contract. The manifest is generated from the actual built artifacts and records the exact asset name, byte size, and SHA-256 digest for each supported automatic-update target.

The current updater entries are:

- `windows-x64` -> `Spencer-Macro-Utilities-Windows.zip`
- `linux-x86_64` -> `Spencer-Macro-Utilities-V<version>-Linux-x86_64.zip`
- `macos-universal` -> `Spencer-Macro-Utilities-V<version>-macOS-universal.zip`

Direct Windows EXEs, DMGs, DEBs, RPMs, Nix outputs, and contributor-added release assets are not selected merely because they are attached to a release. V3.4+ clients use only the artifact named by the matching manifest entry and verify its size and SHA-256.

Contributors may attach additional release assets after publication without changing the V3.4+ updater contract. Do not replace or mutate an artifact already named in `update-manifest.json` without regenerating the manifest and treating the release as compromised/inconsistent until every hash matches again.

## What the workflow produces

The V3.4.0 draft release contains six official files:

- `Spencer-Macro-Utilities-Windows.zip` — V3.4.0-only compatibility layout with signed root-level `suspend.exe`; the executable self-migrates to `Spencer-Macro-Utilities-V3.4.0-Windows.exe` before showing the GUI.
- `Spencer-Macro-Utilities-V3.4.0-Windows.exe` — direct signed Windows executable with the correct public filename immediately.
- `Spencer-Macro-Utilities-V3.4.0-Linux-x86_64.zip` — AppImage, Debian package, Fedora/RHEL RPM, portable tarball, setup guide, notices, and checksums.
- `Spencer-Macro-Utilities-V3.4.0-macOS-universal.zip` — automatic-update macOS ZIP containing the branded app bundle.
- `Spencer-Macro-Utilities-V3.4.0-macOS-universal.dmg` — human-facing macOS installer image.
- `update-manifest.json` — authoritative V3.4+ updater mapping and hashes.

Starting with V3.4.1, the same six asset names/patterns remain, but the Windows ZIP switches to the clean nested/versioned layout and contains no `suspend.exe`.

The caller creates or updates a draft GitHub release tagged `V<version>` with an inline placeholder and a comparison link to the previous published release. It never publishes the draft automatically.

## Before publishing

Verify all of the following:

- The exact expected asset set is present and non-empty.
- `update-manifest.json` parses, names the expected Windows/Linux/macOS updater assets, and its byte sizes/SHA-256 values match the uploaded files.
- Windows reports a valid SignPath Authenticode signature for the direct EXE and executable inside the portable ZIP.
- For V3.4.0 only, `Spencer-Macro-Utilities-Windows.zip` is the first ZIP returned by the release asset API, contains signed root-level `suspend.exe`, and launching that file migrates/relaunches as `Spencer-Macro-Utilities-V3.4.0-Windows.exe` before the GUI appears.
- For V3.4.0 only, end-to-end test copies of V3.0/V3.1 (where practical), V3.2.1, and V3.3.x can cross to V3.4.0. At minimum, validate a stock V3.2.1 client and a stock V3.3.0/V3.3.1 Windows client against a controlled test release before publishing the production release.
- Before building V3.4.1, delete the V3.4.0-only migration block from `app/main_windows.cpp`; its compile-time tripwire is supposed to fail the Windows build until this happens. Verify the V3.4.1+ Windows ZIP uses `Spencer-Macro-Utilities/Spencer-Macro-Utilities-V<version>-Windows.exe` and contains no `suspend.exe`.
- The macOS app contains both `arm64` and `x86_64` slices, is named `Spencer Macro Utilities.app` in the public package, and uses the stable release certificate.
- The mounted DMG has the intended Finder layout and the app refuses normal execution from a read-only DMG/App Translocation while continuing to work from `/Applications` and writable external volumes.
- `nix flake check --no-update-lock-file` passes, and a trusted aarch64 builder validates the native aarch64 output when that build is part of the release process.
- The Linux ZIP contains exactly the documented package choices, `THIRD_PARTY_NOTICES.md`, and matching `SHA256SUMS`.
- Package-manager installation modes that cannot safely self-update direct the user to their package manager instead.
- A forced updater check/download/apply failure still exposes the hard-coded **Open latest release** fallback to `https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/latest`.

Do not publish merely because packaging completed. Platform-specific signing, migration, update, rollback, and installer behavior must be validated on the actual target operating systems.

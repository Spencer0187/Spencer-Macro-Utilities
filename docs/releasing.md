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

The SignPath project’s `initial` artifact configuration must accept `suspend.exe`. The release workflow grants the SignPath action read-only access to the source Actions artifact and verifies the returned Authenticode signature before packaging it.

Create the stable macOS certificate with:

```bash
bash scripts/create_macos_release_certificate.sh
```

Keep the exported certificate and password private. macOS automatic updates compare the new app’s signing certificate with the currently installed app, so rotating this certificate requires a deliberate migration plan.

## Prepare a release

1. Bump the version:

   ```bash
   python3 scripts/version.py --bump minor
   ```

   An exact version also works:

   ```bash
   python3 scripts/version.py 3.3.0
   ```

2. Run **Actions → Build Draft Release**, then write the release notes directly
   in the generated GitHub draft before publishing it. The draft starts with a
   placeholder and a generated comparison link.

3. Keep `.github/autoupdaterurl` synchronized with the new version, but do not
   merge that change to the branch read by older V3.0/V3.1 clients until the
   matching Windows asset is about to become publicly downloadable. Those
   clients read this file from the repository rather than GitHub's release API.

4. Validate the synchronized metadata and local tooling:

   ```bash
   python3 scripts/version.py --check
   python3 -m unittest discover -s tests
   (cd platform/linux/nethelper && go test -mod=vendor ./... && go vet -mod=vendor ./...)
   cmake -S . -B build/release-check -DCMAKE_BUILD_TYPE=Release
   cmake --build build/release-check --target suspend --parallel
   ```

5. Push the release commit, open **Actions → Build Draft Release**, and run the workflow from that exact branch or commit.

## What the workflow produces

The reusable build workflow creates:

* `Spencer-Macro-Utilities-<version>.zip` with a SignPath-signed Windows executable. This historical primary name also keeps older SMU updaters compatible.
* `Spencer-Macro-Utilities-V<version>-Linux-x86_64.zip` containing the AppImage, Debian package, Fedora/RHEL RPM, portable tarball, setup guide, privacy/license notices, and checksums.
* `Spencer-Macro-Utilities-V<version>-macOS-universal.zip`.
* `Spencer-Macro-Utilities-V<version>-macOS-universal.dmg`.

The caller then creates or updates a draft GitHub release tagged `V<version>`
with an inline placeholder and a comparison link to the previous published
release. It never publishes the draft automatically.

Before publishing, verify:

* All four assets are present and non-empty.
* The workflow's legacy compatibility check confirms that the generic Windows
  ZIP is the first ZIP returned by the created release's asset API.
* Windows reports a valid SignPath Authenticode signature.
* The macOS app contains both `arm64` and `x86_64` slices and uses the stable release certificate.
* `nix flake check --no-update-lock-file` passes, and a trusted aarch64 builder
  validates the native aarch64 output.
* The Linux ZIP contains exactly the documented package choices,
  `THIRD_PARTY_NOTICES.md`, and matching `SHA256SUMS`.
* Update checks choose the Windows ZIP, macOS ZIP, and Linux ZIP on their respective platforms.
* Any package mode that cannot self-update is called out in the GitHub draft
  release notes.
* A published test release proves that a stock V3.2.1 client downloads the
  generic Windows ZIP. The draft-order assertion is necessary but does not
  replace this end-to-end test against `/releases/latest`.

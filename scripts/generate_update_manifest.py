#!/usr/bin/env python3
"""Generate the authoritative SMU GitHub release update manifest."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path

SCHEMA_VERSION = 1
MINIMUM_UPDATER_VERSION = "3.4.0"
SEMVER_RE = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$")


def validate_version(value: str) -> str:
    value = value.strip()
    if not SEMVER_RE.fullmatch(value):
        raise ValueError("version must use canonical MAJOR.MINOR.PATCH")
    return value


def official_update_assets(version: str) -> dict[str, str]:
    version = validate_version(version)
    # V3.4.0-ONLY legacy bridge detail: stock V3.2 blindly picks the first
    # release asset whose name ends with the exact, case-sensitive suffix
    # ".zip". GitHub does not preserve upload order in release JSON, so the
    # Linux/macOS updater archives use uppercase .ZIP for this one release.
    # V3.2 therefore sees only Spencer-Macro-Utilities-Windows.zip, while
    # V3.3 lowercases asset names and V3.4+ uses this manifest's exact names.
    archive_suffix = ".ZIP" if version == "3.4.0" else ".zip"
    return {
        "windows-x64": "Spencer-Macro-Utilities-Windows.zip",
        "linux-x86_64": f"Spencer-Macro-Utilities-V{version}-Linux-x86_64{archive_suffix}",
        "macos-universal": f"Spencer-Macro-Utilities-V{version}-macOS-universal{archive_suffix}",
    }


def validate_asset_name(value: str) -> str:
    if not value or Path(value).name != value or value in {".", ".."}:
        raise ValueError("update asset override must be a plain filename")
    return value


def update_assets(version: str, windows_asset: str | None = None) -> dict[str, str]:
    assets = official_update_assets(version)
    if windows_asset is not None:
        assets["windows-x64"] = validate_asset_name(windows_asset)
    return assets


def file_contract(path: Path) -> dict[str, object]:
    if not path.is_file():
        raise FileNotFoundError(f"required updater asset is missing: {path.name}")
    size = path.stat().st_size
    if size <= 0:
        raise ValueError(f"required updater asset is empty: {path.name}")
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return {
        "asset": path.name,
        "size": size,
        "sha256": digest.hexdigest(),
    }


def build_manifest(
    release_dir: Path,
    version: str,
    windows_asset: str | None = None,
    only_platform: str | None = None,
) -> dict[str, object]:
    version = validate_version(version)
    selected = update_assets(version, windows_asset)
    if only_platform is not None:
        if only_platform not in selected:
            raise ValueError(f"unsupported update platform: {only_platform}")
        selected = {only_platform: selected[only_platform]}
    artifacts = {
        key: file_contract(release_dir / filename)
        for key, filename in selected.items()
    }
    return {
        "schema_version": SCHEMA_VERSION,
        "release_version": version,
        "minimum_updater_version": MINIMUM_UPDATER_VERSION,
        "artifacts": artifacts,
    }


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--release-dir", type=Path, required=True)
    result.add_argument("--version", required=True)
    result.add_argument("--output", type=Path, required=True)
    result.add_argument("--windows-asset")
    result.add_argument(
        "--only-platform",
        choices=("windows-x64", "linux-x86_64", "macos-universal"),
    )
    return result


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    manifest = build_manifest(
        args.release_dir,
        args.version,
        windows_asset=args.windows_asset,
        only_platform=args.only_platform,
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

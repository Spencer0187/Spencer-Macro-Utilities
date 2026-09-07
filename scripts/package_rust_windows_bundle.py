#!/usr/bin/env python3
"""Package a staged Rust Windows SMU runtime into the future release ZIP layout.

This is intentionally used only by the Rust rewrite validation workflow until the
signed production Windows release switches from the legacy C++ executable.
"""

from __future__ import annotations

import argparse
from pathlib import Path
import shutil
import tempfile
import zipfile

PACKAGE_ROOT = "Spencer-Macro-Utilities"
RUNTIME_FILES = (
    "Spencer-Macro-Utilities.exe",
    "SMU-Update-Helper.exe",
    "SDL3.dll",
    "SMCWinDivert.dll",
    "SMCWinDivert64.sys",
)
NOTICE_FILES = (
    ("README.md", "README.md"),
    ("LICENSE", "LICENSE"),
    ("PRIVACY.md", "PRIVACY.md"),
    ("THIRD_PARTY_NOTICES.md", "THIRD_PARTY_NOTICES.md"),
    ("third_party/SDL/LICENSE.txt", "licenses/SDL.txt"),
    ("third_party/appimage-runtime/LICENSE", "licenses/AppImage-type2-runtime.txt"),
    (
        "platform/linux/nethelper/vendor/github.com/coreos/go-iptables/LICENSE",
        "licenses/go-iptables-LICENSE.txt",
    ),
    (
        "platform/linux/nethelper/vendor/github.com/coreos/go-iptables/NOTICE",
        "licenses/go-iptables-NOTICE.txt",
    ),
    ("third_party/windivert/LICENSE-LGPL-3.0.txt", "licenses/WinDivert-LGPL-3.0.txt"),
)


def validate_staged_runtime(stage_dir: Path) -> None:
    missing = [stage_dir / name for name in RUNTIME_FILES if not (stage_dir / name).is_file()]
    assets = stage_dir / "assets"
    if not assets.is_dir():
        missing.append(assets)
    if missing:
        rendered = ", ".join(str(path) for path in missing)
        raise RuntimeError(f"staged Rust Windows runtime is incomplete: {rendered}")


def copy_bundle_tree(root: Path, stage_dir: Path, bundle_root: Path) -> None:
    validate_staged_runtime(stage_dir)
    shutil.copytree(stage_dir, bundle_root)
    for source_name, destination_name in NOTICE_FILES:
        source = root / source_name
        if not source.is_file():
            raise RuntimeError(f"required release notice is missing: {source}")
        destination = bundle_root / destination_name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)


def write_zip(source_root: Path, output: Path) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    if output.exists():
        output.unlink()
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for path in sorted(source_root.rglob("*")):
            if not path.is_file():
                continue
            relative = path.relative_to(source_root).as_posix()
            archive.write(path, f"{PACKAGE_ROOT}/{relative}")


def package_bundle(root: Path, stage_dir: Path, output: Path) -> Path:
    with tempfile.TemporaryDirectory(prefix="smu-rust-windows-package-") as directory:
        bundle_root = Path(directory) / PACKAGE_ROOT
        copy_bundle_tree(root, stage_dir, bundle_root)
        write_zip(bundle_root, output)
    return output


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--stage-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    output = package_bundle(args.root.resolve(), args.stage_dir.resolve(), args.output.resolve())
    print(f"Packaged Rust Windows validation bundle: {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

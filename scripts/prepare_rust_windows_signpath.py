#!/usr/bin/env python3
"""Prepare the exact four-file Rust Windows SignPath signing input.

Intentionally excludes the kernel driver and assets. The signed output is NOT
suitable for direct publication: production Windows CI must verify Authenticode
signer continuity, add the original upstream-signed driver and resources, and
build the complete runtime bundle before enabling automatic updates.
"""

from __future__ import annotations

import argparse
from pathlib import Path
import zipfile
import xml.etree.ElementTree as ET

RELEASE_SIGNER_FILES = (
    "Spencer-Macro-Utilities.exe",
    "SMU-Update-Helper.exe",
    "SDL3.dll",
    "SMCWinDivert.dll",
)
SIGNPATH_CONFIG = Path(__file__).resolve().parent / "signpath" / "rust-windows-bundle.xml"
NAMESPACE = "http://signpath.io/artifact-configuration/v1"
MAX_FILE_BYTES = 256 * 1024 * 1024


def verify_signpath_config(path: Path = SIGNPATH_CONFIG) -> None:
    root = ET.parse(path).getroot()
    if root.tag != f"{{{NAMESPACE}}}artifact-configuration":
        raise ValueError("unexpected SignPath artifact configuration namespace")
    children = list(root)
    if len(children) != 1 or children[0].tag != f"{{{NAMESPACE}}}zip-file":
        raise ValueError("SignPath signing input must be a ZIP")
    members = children[0]
    observed = []
    for child in members:
        if child.tag != f"{{{NAMESPACE}}}pe-file" or list(child)[0:] == []:
            raise ValueError("SignPath ZIP contains an unsupported entry")
        if len(child) != 1 or child[0].tag != f"{{{NAMESPACE}}}authenticode-sign":
            raise ValueError("SignPath ZIP must sign each file exactly once")
        if child.attrib.keys() != {"path"} or child[0].attrib:
            raise ValueError("SignPath ZIP uses unexpected signing parameters")
        observed.append(child.attrib["path"])
    if tuple(observed) != RELEASE_SIGNER_FILES:
        raise ValueError("SignPath signer file set differs from the Rust updater contract")


def prepare(stage: Path, output: Path) -> None:
    verify_signpath_config()
    for name in RELEASE_SIGNER_FILES:
        candidate = stage / name
        if candidate.is_symlink() or not candidate.is_file():
            raise ValueError(f"missing or unsafe Rust Windows signing input: {candidate}")
        if not 64 <= candidate.stat().st_size <= MAX_FILE_BYTES:
            raise ValueError(f"invalid Windows signing input size: {candidate}")
        with candidate.open("rb") as source:
            if source.read(2) != b"MZ":
                raise ValueError(f"not a Windows PE signing input: {candidate}")
    output.parent.mkdir(parents=True, exist_ok=True)
    if output.is_symlink():
        raise ValueError(f"refusing symlink signing output: {output}")
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for name in RELEASE_SIGNER_FILES:
            archive.write(stage / name, name)
    with zipfile.ZipFile(output) as archive:
        if tuple(archive.namelist()) != RELEASE_SIGNER_FILES:
            raise ValueError("incorrect SignPath ZIP contents")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stage-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    prepare(args.stage_dir.resolve(), args.output.absolute())
    print(f"Prepared four-user-mode-file signing request: {args.output}")


if __name__ == "__main__":
    main()

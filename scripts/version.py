#!/usr/bin/env python3
"""Read, validate, or update Spencer Macro Utilities' canonical version."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

SEMVER_RE = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$")


def parse_version(value: str) -> tuple[int, int, int]:
    match = SEMVER_RE.fullmatch(value.strip())
    if not match:
        raise ValueError("version must use MAJOR.MINOR.PATCH with no prefix or suffix")
    return tuple(int(part) for part in match.groups())


def bump_version(current: str, part: str) -> str:
    major, minor, patch = parse_version(current)
    if part == "major":
        return f"{major + 1}.0.0"
    if part == "minor":
        return f"{major}.{minor + 1}.0"
    return f"{major}.{minor}.{patch + 1}"


def replace_exact(pattern: re.Pattern[str], replacement: str, text: str, path: Path) -> str:
    updated, count = pattern.subn(replacement, text)
    if count != 1:
        raise RuntimeError(f"expected one version field in {path}, found {count}")
    return updated


def synchronized_contents(root: Path, version: str) -> dict[Path, str]:
    parse_version(version)
    updates: dict[Path, str] = {}

    nfpm_path = root / "AppImage" / "nfpm.yaml"
    updates[nfpm_path] = replace_exact(
        re.compile(r"(?m)^version:[ \t]*\S+[ \t]*$"),
        f"version: {version}",
        nfpm_path.read_text(encoding="utf-8"),
        nfpm_path,
    )

    updater_path = root / ".github" / "autoupdaterurl"
    updater_text = updater_path.read_text(encoding="utf-8")
    updates[updater_path] = replace_exact(
        re.compile(r"/releases/download/V[^/]+/"),
        f"/releases/download/V{version}/",
        updater_text,
        updater_path,
    )

    package_path = root / "package.json"
    package_data = json.loads(package_path.read_text(encoding="utf-8"))
    package_data["version"] = version
    updates[package_path] = json.dumps(package_data, indent=2) + "\n"

    updates[root / "version"] = version + "\n"
    return updates


def check_synchronized(root: Path) -> list[str]:
    version = (root / "version").read_text(encoding="utf-8").strip()
    expected = synchronized_contents(root, version)
    errors: list[str] = []
    for path, content in expected.items():
        if path.read_text(encoding="utf-8") != content:
            errors.append(str(path.relative_to(root)))
    return errors


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Print or update the SMU version and synchronized package metadata."
    )
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument("new_version", nargs="?", help="exact MAJOR.MINOR.PATCH version")
    selection.add_argument("--bump", choices=("major", "minor", "patch"))
    parser.add_argument("--check", action="store_true", help="fail if version metadata disagrees")
    parser.add_argument("--dry-run", action="store_true", help="print the result without writing files")
    parser.add_argument("--root", type=Path, help=argparse.SUPPRESS)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    root = (args.root or Path(__file__).resolve().parents[1]).resolve()
    version_path = root / "version"

    try:
        current = version_path.read_text(encoding="utf-8").strip()
        parse_version(current)

        if args.check:
            if args.new_version or args.bump or args.dry_run:
                raise ValueError("--check cannot be combined with a version change")
            mismatches = check_synchronized(root)
            if mismatches:
                print(
                    "Version metadata is out of sync: " + ", ".join(mismatches),
                    file=sys.stderr,
                )
                return 1
            print(current)
            return 0

        requested = args.new_version
        if args.bump:
            requested = bump_version(current, args.bump)
        if requested is None:
            print(current)
            return 0

        parse_version(requested)
        updates = synchronized_contents(root, requested)
        if args.dry_run:
            print(requested)
            return 0

        for path, content in updates.items():
            path.write_text(content, encoding="utf-8")
        print(f"{current} -> {requested}")
        return 0
    except (OSError, ValueError, RuntimeError, json.JSONDecodeError) as exc:
        print(f"version: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())

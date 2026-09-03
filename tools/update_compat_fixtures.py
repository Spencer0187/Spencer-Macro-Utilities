#!/usr/bin/env python3
"""Regenerate checked-in rewrite fixtures from production C++ compatibility oracles."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
ORACLE = ROOT / "build" / "smu_compat_oracle"
PROFILE_ORACLE = ROOT / "build" / "smu_profile_compat_oracle"

HOTKEY_CASES = [
    "A",
    "z",
    "0",
    "9",
    "Ctrl+A",
    "Ctrl+Shift+A",
    "Win+Ctrl+Alt+Shift+F12",
    "F24",
    "Numpad0",
    "Numpad9",
    "MouseWheelUp",
    "wheel-down",
    "LShift+A",
    "RCtrl+B",
    "Left Alt + C",
    "RightWin+D",
    "Ctrl+A+B",
    "Ctrl++A",
    "+Ctrl+A+",
    "shift",
    "Ctrl+Alt",
    "",
    "+",
    "Ctrl+NoSuchKey",
    "Back_Slash",
    "Page-Down",
    "left mouse",
    "Super+Escape",
    "Meta+F1",
    "f0",
    "f25",
]


def run_json(binary: Path, *arguments: str) -> dict[str, Any]:
    completed = subprocess.run(
        [str(binary), *arguments],
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(completed.stdout)


def compact(value: Any) -> str:
    return json.dumps(value, separators=(",", ":"), sort_keys=True)


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(path.relative_to(ROOT))


def write_hotkeys() -> None:
    output = {
        "oracle": "smu_compat_oracle hotkey",
        "cases": [run_json(ORACLE, "hotkey", case) for case in HOTKEY_CASES],
    }
    write_json(ROOT / "compat" / "hotkeys" / "cases.json", output)


def profile_format_cases() -> list[dict[str, Any]]:
    inputs = [
        ("profile-metadata", compact({"_metadata": {}, "Profile 1": {}})),
        ("metadata-only", compact({"_metadata": {}})),
        ("legacy-bool", compact({"macrotoggled": True})),
        ("legacy-numeric", compact({"selected_section": 15})),
        ("legacy-text", compact({"text": "hello"})),
        ("metadata-wrong-type-suppresses-legacy", compact({"_metadata": False, "macrotoggled": True})),
        ("char-buffer-only-is-unknown", compact({"RobloxSensValue": "0.6"})),
        ("screen-width-only-is-unknown", compact({"screen_width": 1180})),
        ("instances-only-is-unknown", compact({"wallhop_instances": []})),
        ("imported-scripts-only-is-unknown", compact({"imported_scripts": []})),
        ("arbitrary-object", compact({"Profile 1": {"marker": "A"}})),
        ("empty-object", compact({})),
        ("array-root", compact([])),
        ("scalar-root", compact(42)),
        ("invalid-json", "{"),
    ]
    cases = []
    for name, serialized in inputs:
        observed = run_json(PROFILE_ORACLE, "detect", serialized)
        cases.append({"name": name, "serialized": serialized, "format": observed["format"]})
    return cases


def best_profile_cases() -> list[dict[str, Any]]:
    cases = [
        (
            "requested-exact-wins",
            {"_metadata": {"last_active_profile": "Profile 2"}, "Profile 1": {}, "Profile 2": {}},
            "Profile 1",
        ),
        (
            "last-active-fallback",
            {"_metadata": {"last_active_profile": "Profile 2"}, "Profile 1": {}, "Profile 2": {}},
            "missing",
        ),
        (
            "default-last-active-is-ignored",
            {"_metadata": {"last_active_profile": "(default)"}, "(default)": {}, "Profile 1": {}},
            "",
        ),
        (
            "missing-last-active-falls-back-lexicographically",
            {"_metadata": {"last_active_profile": "missing"}, "Profile 10": {}, "Profile 2": {}, "Alpha": {}},
            "",
        ),
        (
            "non-object-profile-is-ignored",
            {"_metadata": {}, "Profile 1": 1, "Profile 2": {}},
            "Profile 1",
        ),
        ("only-default-has-no-startup-profile", {"_metadata": {}, "(default)": {}}, ""),
        ("metadata-only-has-no-profile", {"_metadata": {}}, ""),
        (
            "explicit-default-is-returned",
            {"_metadata": {}, "(default)": {}, "Profile 1": {}},
            "(default)",
        ),
        (
            "last-active-wrong-type-is-ignored",
            {"_metadata": {"last_active_profile": 7}, "Profile A": {}, "Profile B": {}},
            "",
        ),
        ("non-object-root", [], "Profile 1"),
    ]
    output = []
    for name, value, requested in cases:
        serialized = compact(value)
        observed = run_json(PROFILE_ORACLE, "best-profile", serialized, requested)
        output.append(
            {
                "name": name,
                "serialized": serialized,
                "requested": requested,
                "best": observed["best"],
            }
        )
    return output


def profile_name_cases() -> list[dict[str, Any]]:
    cases = [
        (
            "default-first-then-sorted",
            {
                "Profile 10": {},
                "Profile 2": {},
                "Alpha": {},
                "(default)": {},
                "_metadata": {},
                "not-a-profile": 7,
            },
        ),
        ("metadata-only", {"_metadata": {}}),
        ("no-metadata-still-lists-object-members", {"Zulu": {}, "Alpha": {}, "scalar": False}),
    ]
    output = []
    with tempfile.TemporaryDirectory(prefix="smu-profile-name-fixtures-") as temporary:
        root = Path(temporary)
        for index, (name, value) in enumerate(cases):
            path = root / f"case-{index}.json"
            path.write_text(json.dumps(value), encoding="utf-8")
            observed = run_json(PROFILE_ORACLE, "profile-names", str(path))
            output.append({"name": name, "document": value, "names": observed["names"]})
    return output


def mutation_cases() -> list[dict[str, Any]]:
    base = {
        "Profile A": {"marker": "A"},
        "Profile B": {"marker": "B"},
        "_metadata": {"last_active_profile": "Profile A"},
    }
    specifications = [
        ("duplicate-success", "duplicate", base, ["Profile A", "Profile C"]),
        ("duplicate-missing-source", "duplicate", base, ["missing", "Profile C"]),
        ("duplicate-existing-target", "duplicate", base, ["Profile A", "Profile B"]),
        (
            "duplicate-default-source-is-allowed",
            "duplicate",
            {**base, "(default)": {"marker": "defaults"}},
            ["(default)", "Defaults Copy"],
        ),
        ("duplicate-into-default-is-allowed-when-missing", "duplicate", base, ["Profile A", "(default)"]),
        ("rename-success", "rename", base, ["Profile A", "Profile C"]),
        ("rename-same-name-short-circuits-true", "rename", base, ["missing", "missing"]),
        ("rename-default-source-rejected", "rename", {**base, "(default)": {}}, ["(default)", "X"]),
        ("rename-to-default-rejected", "rename", base, ["Profile A", "(default)"]),
        ("rename-existing-target-rejected", "rename", base, ["Profile A", "Profile B"]),
        ("rename-missing-source", "rename", base, ["missing", "Profile C"]),
        ("delete-success", "delete", base, ["Profile B"]),
        ("delete-missing", "delete", base, ["missing"]),
        ("delete-default-rejected", "delete", {**base, "(default)": {}}, ["(default)"]),
    ]

    output = []
    with tempfile.TemporaryDirectory(prefix="smu-profile-mutation-fixtures-") as temporary:
        root = Path(temporary)
        for index, (name, operation, document, arguments) in enumerate(specifications):
            path = root / f"case-{index}.json"
            path.write_text(json.dumps(document), encoding="utf-8")
            observed = run_json(PROFILE_ORACLE, operation, str(path), *arguments)
            output.append(
                {
                    "name": name,
                    "operation": operation,
                    "arguments": arguments,
                    "document": document,
                    "success": observed["success"],
                    "result": observed["document"],
                    "backup_exists": observed["backup_exists"],
                }
            )
    return output


def recovery_cases() -> list[dict[str, Any]]:
    base = {
        "Profile A": {"marker": "A"},
        "Profile B": {"marker": "B"},
        "_metadata": {},
    }
    output = []
    with tempfile.TemporaryDirectory(prefix="smu-profile-recovery-fixtures-") as temporary:
        root = Path(temporary)

        path = root / "rollback.json"
        backup = Path(str(path) + ".bak")
        path.write_text(json.dumps(base), encoding="utf-8")
        first = run_json(PROFILE_ORACLE, "duplicate", str(path), "Profile A", "Profile C")
        first_primary = inspect_source(path)
        first_backup = inspect_source(backup)
        path.write_text("{", encoding="utf-8")
        second = run_json(PROFILE_ORACLE, "duplicate", str(path), "Profile A", "Profile D")
        output.append(
            {
                "name": "one-generation-backup-rolls-back-latest-write",
                "initial_primary": base,
                "initial_backup": None,
                "operation": ["duplicate", "Profile A", "Profile C"],
                "first_success": first["success"],
                "first_primary": first_primary,
                "first_backup": first_backup,
                "corrupt_primary_before_second": "{",
                "second_operation": ["duplicate", "Profile A", "Profile D"],
                "second_success": second["success"],
                "second_primary": inspect_source(path),
                "second_backup": inspect_source(backup),
            }
        )

        path = root / "valid-backup.json"
        backup = Path(str(path) + ".bak")
        path.write_text("{", encoding="utf-8")
        backup.write_text(json.dumps(base), encoding="utf-8")
        observed = run_json(PROFILE_ORACLE, "duplicate", str(path), "Profile A", "Profile D")
        output.append(
            {
                "name": "corrupt-primary-valid-backup-recovers",
                "initial_primary": "{",
                "initial_backup": base,
                "operation": ["duplicate", "Profile A", "Profile D"],
                "success": observed["success"],
                "primary": inspect_source(path),
                "backup": inspect_source(backup),
            }
        )

        path = root / "no-backup.json"
        backup = Path(str(path) + ".bak")
        path.write_text("{", encoding="utf-8")
        observed = run_json(PROFILE_ORACLE, "duplicate", str(path), "Profile A", "Profile Z")
        output.append(
            {
                "name": "corrupt-primary-no-valid-backup-refuses",
                "initial_primary": "{",
                "initial_backup": None,
                "operation": ["duplicate", "Profile A", "Profile Z"],
                "success": observed["success"],
                "primary": inspect_source(path),
                "backup": inspect_source(backup),
            }
        )

        path = root / "backup-only.json"
        backup = Path(str(path) + ".bak")
        backup.write_text(json.dumps(base), encoding="utf-8")
        observed = run_json(PROFILE_ORACLE, "duplicate", str(path), "Profile A", "Profile C")
        output.append(
            {
                "name": "missing-primary-valid-backup-recovers",
                "initial_primary": None,
                "initial_backup": base,
                "operation": ["duplicate", "Profile A", "Profile C"],
                "success": observed["success"],
                "primary": inspect_source(path),
                "backup": inspect_source(backup),
            }
        )

    return output


def source_path(layout: dict[str, Path], key: str) -> Path:
    location, filename = key.split("/", 1)
    return layout[location] / filename


def write_source(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if isinstance(value, str):
        path.write_text(value, encoding="utf-8")
    else:
        path.write_text(json.dumps(value), encoding="utf-8")


def inspect_source(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {"exists": False}
    text = path.read_text(encoding="utf-8")
    try:
        parsed = json.loads(text)
    except json.JSONDecodeError:
        return {"exists": True, "raw": text}
    return {"exists": True, "json": parsed}


def resolution_cases() -> list[dict[str, Any]]:
    marker = lambda value: {"marker": value}  # noqa: E731 - compact scenario table helper
    scenarios: list[tuple[str, dict[str, Any]]] = [
        ("no-source", {}),
        ("canonical-smc", {"canonical/SMCSettings.json": marker("canonical-smc")}),
        ("canonical-smc-backup", {"canonical/SMCSettings.json.bak": marker("canonical-smc-backup")}),
        (
            "canonical-smc-primary-beats-backup",
            {
                "canonical/SMCSettings.json": marker("primary"),
                "canonical/SMCSettings.json.bak": marker("backup"),
            },
        ),
        (
            "canonical-smc-backup-beats-later-smc-primary",
            {
                "canonical/SMCSettings.json.bak": marker("canonical-backup"),
                "executable/SMCSettings.json": marker("executable-primary"),
            },
        ),
        (
            "corrupt-canonical-smc-valid-backup-self-heals",
            {
                "canonical/SMCSettings.json": "{",
                "canonical/SMCSettings.json.bak": marker("recovered-backup"),
            },
        ),
        ("canonical-rmc", {"canonical/RMCSettings.json": marker("canonical-rmc")}),
        (
            "smc-globally-outranks-rmc",
            {
                "canonical/RMCSettings.json": marker("canonical-rmc"),
                "executable/SMCSettings.json": marker("exe-smc"),
            },
        ),
        (
            "corrupt-canonical-smc-valid-rmc",
            {
                "canonical/SMCSettings.json": "{",
                "canonical/RMCSettings.json": marker("recovered-rmc"),
            },
        ),
        ("executable-smc", {"executable/SMCSettings.json": marker("exe-smc")}),
        (
            "same-filename-directory-precedence",
            {
                "executable/SMCSettings.json": marker("executable"),
                "parent/SMCSettings.json": marker("parent"),
                "current/SMCSettings.json": marker("current"),
            },
        ),
        (
            "parseable-scalar-smc-masks-rmc",
            {
                "canonical/SMCSettings.json": 42,
                "canonical/RMCSettings.json": marker("rmc-profile-data"),
            },
        ),
        (
            "parseable-array-smc-masks-rmc",
            {
                "canonical/SMCSettings.json": [1, 2, 3],
                "executable/RMCSettings.json": marker("rmc-profile-data"),
            },
        ),
        ("executable-rmc-backup", {"executable/RMCSettings.json.bak": marker("exe-rmc-backup")}),
        ("parent-smc", {"parent/SMCSettings.json": marker("parent-smc")}),
        ("cwd-rmc", {"current/RMCSettings.json": marker("cwd-rmc")}),
        (
            "invalid-smc-is-skipped-for-valid-rmc",
            {
                "executable/SMCSettings.json": "{",
                "current/RMCSettings.json": marker("cwd-rmc"),
            },
        ),
    ]

    output = []
    with tempfile.TemporaryDirectory(prefix="smu-profile-resolution-fixtures-") as temporary:
        temporary_root = Path(temporary)
        for index, (name, sources) in enumerate(scenarios):
            root = temporary_root / f"case-{index}"
            layout = {
                "canonical": root / "config",
                "executable": root / "app" / "bin",
                "parent": root / "app",
                "current": root / "cwd",
            }
            for directory in layout.values():
                directory.mkdir(parents=True, exist_ok=True)
            for key, value in sources.items():
                write_source(source_path(layout, key), value)

            observed = run_json(
                PROFILE_ORACLE,
                "resolve",
                str(layout["canonical"]),
                str(layout["executable"]),
                str(layout["current"]),
            )
            resolved = Path(observed["resolved"])
            try:
                resolved_relative = resolved.relative_to(root).as_posix()
            except ValueError:
                resolved_relative = resolved.as_posix()

            canonical_primary = layout["canonical"] / "SMCSettings.json"
            inspected_sources = {
                key: inspect_source(source_path(layout, key)) for key in sources
            }
            corrupt_files = sorted(layout["canonical"].glob("SMCSettings.json.corrupt-*"))
            output.append(
                {
                    "name": name,
                    "sources": sources,
                    "resolved": resolved_relative,
                    "canonical_primary": inspect_source(canonical_primary),
                    "source_state_after": inspected_sources,
                    "corrupt_primary_count": len(corrupt_files),
                }
            )
    return output


def write_generated_legacy_keys(keys: list[str]) -> None:
    lines = [
        "// Generated by tools/update_compat_fixtures.py from production C++; do not edit by hand.",
        "pub const LEGACY_FORMAT_KEYS: &[&str] = &[",
    ]
    lines.extend(f"    {json.dumps(key)}," for key in keys)
    lines.append("];")
    lines.append("")
    path = ROOT / "rust" / "crates" / "smu-profile" / "src" / "generated_legacy_keys.rs"
    path.write_text("\n".join(lines), encoding="utf-8")
    print(path.relative_to(ROOT))


def write_profiles() -> None:
    legacy_keys = run_json(PROFILE_ORACLE, "legacy-keys")["keys"]
    write_generated_legacy_keys(legacy_keys)
    output = {
        "oracle": "smu_profile_compat_oracle",
        "legacy_keys": legacy_keys,
        "format_cases": profile_format_cases(),
        "best_profile_cases": best_profile_cases(),
        "profile_name_cases": profile_name_cases(),
        "mutation_cases": mutation_cases(),
        "recovery_cases": recovery_cases(),
        "resolution_cases": resolution_cases(),
    }
    write_json(ROOT / "compat" / "profiles" / "cases.json", output)


def ensure_oracle(path: Path, target: str, parser: argparse.ArgumentParser) -> None:
    if not path.is_file():
        parser.error(f"C++ oracle not found at {path}; build target {target} first")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("fixture", choices=["hotkeys", "profiles", "all"])
    args = parser.parse_args()

    if args.fixture in {"hotkeys", "all"}:
        ensure_oracle(ORACLE, "smu_compat_oracle", parser)
        write_hotkeys()
    if args.fixture in {"profiles", "all"}:
        ensure_oracle(PROFILE_ORACLE, "smu_profile_compat_oracle", parser)
        write_profiles()
    return 0


if __name__ == "__main__":
    sys.exit(main())

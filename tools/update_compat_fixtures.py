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
RUNTIME_ORACLE = ROOT / "build" / "smu_runtime_compat_oracle"

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


def payload_validation_cases() -> list[dict[str, Any]]:
    specifications: list[tuple[str, Any]] = [
        ("empty-object", {}),
        ("non-object-array", []),
        ("non-object-number", 7),
        ("known-bool", {"macrotoggled": False}),
        ("bool-wrong-type", {"macrotoggled": 0}),
        ("known-numeric-integer", {"selected_section": 15}),
        ("known-numeric-fractional", {"selected_section": 1.9}),
        ("numeric-wrong-type", {"selected_section": "15"}),
        ("char-buffer", {"ItemClipDelay": "34"}),
        ("char-buffer-wrong-type", {"ItemClipDelay": 34}),
        ("screen-width-integer", {"screen_width": 1180}),
        ("screen-width-fractional-rejected", {"screen_width": 1180.5}),
        ("section-toggles-short", {"section_toggles": [True]}),
        ("section-toggles-bad-member", {"section_toggles": [True, 0]}),
        ("disable-outside-empty", {"disable_outside_roblox": []}),
        ("section-order-wide-integers", {"section_order_vector": [0, 4294967296, -1]}),
        ("section-order-fractional", {"section_order_vector": [0, 1.5]}),
        ("extra-wallhop-empty-object", {"extra_wallhop_instances": [{}]}),
        ("extra-wallhop-negative-unsigned", {"extra_wallhop_instances": [{"vk_trigger": -1}]}),
        ("extra-wallhop-fractional-unsigned", {"extra_wallhop_instances": [{"vk_trigger": 1.5}]}),
        ("extra-wallhop-wide-unsigned", {"extra_wallhop_instances": [{"vk_trigger": 4294967296}]}),
        ("extra-presskey-empty-object", {"extra_presskey_instances": [{}]}),
        ("extra-spamkey-empty-object", {"extra_spamkey_instances": [{}]}),
        ("imported-scripts-empty", {"imported_scripts": []}),
        ("imported-script-valid-minimal", {"imported_scripts": [{"path": "/tmp/test.lua"}]}),
        ("imported-script-missing-path", {"imported_scripts": [{}]}),
        ("imported-script-empty-path", {"imported_scripts": [{"path": ""}]}),
        ("imported-script-negative-hotkey", {"imported_scripts": [{"path": "x", "hotkey": -1}]}),
        ("imported-script-wide-hotkey", {"imported_scripts": [{"path": "x", "hotkey": 4294967296}]}),
        ("imported-script-bad-ui-state", {"imported_scripts": [{"path": "x", "ui_state": []}]}),
        ("unknown-key-accepted", {"future_setting": {"anything": True}}),
    ]
    output = []
    for name, payload in specifications:
        observed = run_json(PROFILE_ORACLE, "validate-payload", compact(payload))
        output.append({"name": name, "payload": payload, "valid": observed["valid"]})
    return output


def payload_apply_cases() -> list[dict[str, Any]]:
    long_slot = "7" * 300
    specifications: list[tuple[str, Any]] = [
        ("empty-object-rebuilds-base-instances", {}),
        ("numeric-coercion", {"selected_section": 1.9, "vk_f5": -1}),
        ("wide-integer-wrap", {"selected_section": 4294967296, "vk_f6": 4294967296}),
        ("buffer-overrides-numeric", {"clip_delay": 99, "ItemClipDelay": "42"}),
        ("invalid-buffer-keeps-numeric", {"clip_delay": 99, "ItemClipDelay": "42junk"}),
        ("char-buffer-truncates", {"desync_slot": 12, "ItemDesyncSlot": long_slot}),
        ("wallhop-pixels-sync", {"wallhop_dx": 1, "wallhop_dy": 2, "WallhopPixels": "321"}),
        (
            "pixel-strength-sync",
            {"RobloxPixelValue": 1, "speed_strengthx": 2, "speed_strengthy": 3, "RobloxPixelValueChar": "777"},
        ),
        ("spam-delay-sync", {"spam_delay": 1.0, "real_delay": 2, "SpamDelay": "20.5"}),
        (
            "legacy-disable-aliases-without-array",
            {"freezeoutsideroblox": True, "unequipinroblox": False, "presskeyinroblox": True},
        ),
        (
            "present-empty-disable-array-wins-over-aliases",
            {
                "disable_outside_roblox": [],
                "freezeoutsideroblox": True,
                "unequipinroblox": False,
                "presskeyinroblox": True,
            },
        ),
        ("old-section-order-upgrade", {"section_order_vector": [0, 1, 2, 3, 4, 5, 6]}),
        (
            "section-order-deduplicate-filter-fill",
            {"section_order_vector": [4, 4, -1, 99, 2, 15, 13, 2]},
        ),
        ("empty-extra-instances-use-historical-defaults", {
            "extra_wallhop_instances": [{}],
            "extra_presskey_instances": [{}],
            "extra_spamkey_instances": [{}],
        }),
        (
            "extra-presskey-disable-alias-precedence",
            {
                "disable_outside_roblox": [False, False, False, False, False, True],
                "extra_presskey_instances": [
                    {"presskeyinroblox": False},
                    {"disable_outside_roblox": True, "presskeyinroblox": False},
                ],
            },
        ),
        (
            "selected-wallhop-clamped-high",
            {"selected_wallhop_instance": 99, "extra_wallhop_instances": [{}, {}]},
        ),
        ("selected-wallhop-clamped-negative", {"selected_wallhop_instance": -3}),
        (
            "extra-instance-numeric-string-pairs-remain-independent",
            {
                "extra_wallhop_instances": [{"WallhopDelay": 41, "WallhopDelayChar": "99"}],
                "extra_presskey_instances": [{"PressKeyDelay": 12, "PressKeyDelayChar": "77"}],
                "extra_spamkey_instances": [{"spam_delay": 8.0, "real_delay": 9, "SpamDelay": "123"}],
            },
        ),
        ("unknown-top-level-key-ignored", {"unknown_future_field": 12345}),
    ]
    output = []
    for name, payload in specifications:
        observed = run_json(PROFILE_ORACLE, "apply-payload", compact(payload))
        output.append(
            {
                "name": name,
                "payload": payload,
                "success": observed["success"],
                "result": observed["payload"],
            }
        )
    return output


def payload_sequence_cases() -> list[dict[str, Any]]:
    specifications: list[tuple[str, list[Any]]] = [
        (
            "short-section-toggle-retains-tail",
            [
                {"section_toggles": [False] * 16},
                {"section_toggles": [True, True]},
            ],
        ),
        (
            "short-disable-array-retains-tail-and-overrides-aliases",
            [
                {"disable_outside_roblox": [False] * 16},
                {"disable_outside_roblox": [True], "unequipinroblox": True, "presskeyinroblox": True},
            ],
        ),
        (
            "missing-buffer-does-not-resync-runtime",
            [
                {"ItemClipDelay": "77"},
                {"clip_delay": 88},
            ],
        ),
        (
            "missing-section-order-retains-prior-order",
            [
                {"section_order_vector": [8, 7, 6, 5, 4, 3, 2, 1, 0]},
                {"macrotoggled": False},
            ],
        ),
        (
            "missing-extra-arrays-reset-instance-vectors",
            [
                {"extra_wallhop_instances": [{}, {}], "extra_presskey_instances": [{}], "extra_spamkey_instances": [{}]},
                {"macrotoggled": False},
            ],
        ),
        (
            "missing-selected-wallhop-resets-to-zero",
            [
                {"selected_wallhop_instance": 2, "extra_wallhop_instances": [{}, {}]},
                {"macrotoggled": False},
            ],
        ),
    ]
    output = []
    for name, payloads in specifications:
        observed = run_json(
            PROFILE_ORACLE,
            "apply-sequence",
            *(compact(payload) for payload in payloads),
        )
        output.append(
            {
                "name": name,
                "payloads": payloads,
                "success": observed["success"],
                "steps": observed["steps"],
            }
        )
    return output


def write_payload_cases() -> None:
    schema = run_json(PROFILE_ORACLE, "payload-schema")["schema"]
    default_payload = run_json(PROFILE_ORACLE, "snapshot-default")["payload"]
    specification = {
        "schema": schema,
        "default_payload": default_payload,
    }
    write_json(ROOT / "compat" / "profiles" / "payload_spec.json", specification)
    output = {
        "oracle": "smu_profile_compat_oracle payload operations",
        **specification,
        "validation_cases": payload_validation_cases(),
        "apply_cases": payload_apply_cases(),
        "sequence_cases": payload_sequence_cases(),
    }
    write_json(ROOT / "compat" / "profiles" / "payload_cases.json", output)


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
    write_payload_cases()
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


def runtime_frame_delay_cases() -> list[dict[str, Any]]:
    cases = []
    for fps in range(0, 1001):
        observed = run_json(RUNTIME_ORACLE, "frame-delays", str(fps))
        cases.append(
            {
                "fps": fps,
                "first_ms": observed["first_ms"],
                "second_ms": observed["second_ms"],
            }
        )
    return cases


def runtime_rising_edge_cases() -> list[dict[str, Any]]:
    cases = []
    for pressed in (False, True):
        for was_pressed in (False, True):
            observed = run_json(
                RUNTIME_ORACLE,
                "rising-edge",
                str(int(pressed)),
                str(int(was_pressed)),
            )
            cases.append(
                {
                    "pressed": pressed,
                    "was_pressed": was_pressed,
                    "rising": observed["rising"],
                    "next_was_pressed": observed["next_was_pressed"],
                }
            )
    return cases


def runtime_presskey_cases() -> list[dict[str, Any]]:
    specifications = [
        ("normal", 90, 68, 0, 16, True),
        ("same-trigger-output", 68, 68, 0, 16, True),
        ("bonus-delay", 90, 68, 7, 16, True),
        ("negative-bonus-clamps-zero", 90, 68, -5, 16, True),
        ("zero-press-clamps-one", 90, 68, 0, 0, True),
        ("negative-press-clamps-one", 90, 68, 3, -99, True),
        ("cancel-after-bonus", 90, 68, 7, 16, False),
        ("cancel-without-bonus", 90, 68, 0, 16, False),
        ("same-key-cancel-after-release", 68, 68, 5, 16, False),
    ]
    cases = []
    for name, trigger, output, bonus, press, running in specifications:
        observed = run_json(
            RUNTIME_ORACLE,
            "presskey-worker",
            str(trigger),
            str(output),
            str(bonus),
            str(press),
            str(int(running)),
        )
        cases.append(
            {
                "name": name,
                "trigger": trigger,
                "output": output,
                "bonus_delay_ms": bonus,
                "press_delay_ms": press,
                "running_after_bonus": running,
                "plan": observed["plan"],
                "events": observed["events"],
            }
        )
    return cases


def runtime_speedglitch_cases() -> list[dict[str, Any]]:
    cases = []
    case_index = 0
    for foreground in (False, True):
        for section in (False, True):
            for hotkey in (False, True):
                for toggle_switch in (False, True):
                    for was_pressed in (False, True):
                        for is_speed in (False, True):
                            fps = 200
                            strength_x = 600
                            strength_y = -600
                            observed = run_json(
                                RUNTIME_ORACLE,
                                "speedglitch-step",
                                str(int(foreground)),
                                str(int(section)),
                                str(int(hotkey)),
                                str(int(toggle_switch)),
                                str(int(was_pressed)),
                                str(int(is_speed)),
                                str(fps),
                                str(strength_x),
                                str(strength_y),
                            )
                            cases.append(
                                {
                                    "name": f"matrix-{case_index}",
                                    "foreground_allowed": foreground,
                                    "section_enabled": section,
                                    "hotkey_pressed": hotkey,
                                    "toggle_switch": toggle_switch,
                                    "was_pressed": was_pressed,
                                    "is_speed": is_speed,
                                    "fps": fps,
                                    "strength_x": strength_x,
                                    "strength_y": strength_y,
                                    "pressed": observed["pressed"],
                                    "next_was_pressed": observed["next_was_pressed"],
                                    "state_action": observed["state_action"],
                                    "next_is_speed": observed["next_is_speed"],
                                    "execute_cycle": observed["execute_cycle"],
                                    "plan": observed["plan"],
                                    "events": observed["events"],
                                }
                            )
                            case_index += 1

    for fps in (0, 1, 30, 60, 120, 144, 165, 240, 360, 500, 1000):
        observed = run_json(
            RUNTIME_ORACLE,
            "speedglitch-step",
            "1",
            "1",
            "0",
            "0",
            "0",
            "1",
            str(fps),
            "321",
            "-123",
        )
        cases.append(
            {
                "name": f"active-fps-{fps}",
                "foreground_allowed": True,
                "section_enabled": True,
                "hotkey_pressed": False,
                "toggle_switch": False,
                "was_pressed": False,
                "is_speed": True,
                "fps": fps,
                "strength_x": 321,
                "strength_y": -123,
                "pressed": observed["pressed"],
                "next_was_pressed": observed["next_was_pressed"],
                "state_action": observed["state_action"],
                "next_is_speed": observed["next_is_speed"],
                "execute_cycle": observed["execute_cycle"],
                "plan": observed["plan"],
                "events": observed["events"],
            }
        )
    return cases


def write_runtime() -> None:
    output = {
        "oracle": "smu_runtime_compat_oracle",
        "frame_delay_cases": runtime_frame_delay_cases(),
        "rising_edge_cases": runtime_rising_edge_cases(),
        "presskey_cases": runtime_presskey_cases(),
        "speedglitch_cases": runtime_speedglitch_cases(),
    }
    write_json(ROOT / "compat" / "timing" / "runtime_cases.json", output)


def ensure_oracle(path: Path, target: str, parser: argparse.ArgumentParser) -> None:
    if not path.is_file():
        parser.error(f"C++ oracle not found at {path}; build target {target} first")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("fixture", choices=["hotkeys", "profiles", "runtime", "all"])
    args = parser.parse_args()

    if args.fixture in {"hotkeys", "all"}:
        ensure_oracle(ORACLE, "smu_compat_oracle", parser)
        write_hotkeys()
    if args.fixture in {"profiles", "all"}:
        ensure_oracle(PROFILE_ORACLE, "smu_profile_compat_oracle", parser)
        write_profiles()
    if args.fixture in {"runtime", "all"}:
        ensure_oracle(RUNTIME_ORACLE, "smu_runtime_compat_oracle", parser)
        write_runtime()
    return 0


if __name__ == "__main__":
    sys.exit(main())

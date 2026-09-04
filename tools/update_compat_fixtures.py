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


def input_hotkey_inputs() -> list[dict[str, Any]]:
    shift = 0x10000
    ctrl = 0x20000
    alt = 0x40000
    win = 0x80000
    cases: list[dict[str, Any]] = [
        {"name": "unbound", "hotkey": ctrl, "strict": False, "pressed": [0x11]},
        {"name": "plain-down", "hotkey": 0x41, "strict": False, "pressed": [0x41]},
        {"name": "plain-up", "hotkey": 0x41, "strict": False, "pressed": []},
        {"name": "loose-extra-shift", "hotkey": ctrl | 0x41, "strict": False, "pressed": [0x11, 0x10, 0x41]},
        {"name": "strict-extra-shift", "hotkey": ctrl | 0x41, "strict": True, "pressed": [0x11, 0x10, 0x41]},
        {"name": "strict-exact", "hotkey": ctrl | shift | 0x41, "strict": True, "pressed": [0x11, 0x10, 0x41]},
        {"name": "left-ctrl-satisfies-generic", "hotkey": ctrl | 0x41, "strict": True, "pressed": [0xA2, 0x41]},
        {"name": "right-ctrl-satisfies-generic", "hotkey": ctrl | 0x41, "strict": True, "pressed": [0xA3, 0x41]},
        {"name": "left-shift-satisfies-generic", "hotkey": shift | 0x41, "strict": True, "pressed": [0xA0, 0x41]},
        {"name": "right-shift-satisfies-generic", "hotkey": shift | 0x41, "strict": True, "pressed": [0xA1, 0x41]},
        {"name": "left-alt-satisfies-generic", "hotkey": alt | 0x41, "strict": True, "pressed": [0xA4, 0x41]},
        {"name": "right-alt-satisfies-generic", "hotkey": alt | 0x41, "strict": True, "pressed": [0xA5, 0x41]},
        {"name": "left-win-satisfies-win", "hotkey": win | 0x41, "strict": True, "pressed": [0x5B, 0x41]},
        {"name": "right-win-satisfies-win", "hotkey": win | 0x41, "strict": True, "pressed": [0x5C, 0x41]},
        {"name": "generic-ctrl-main-strict", "hotkey": 0x11, "strict": True, "pressed": [0xA2]},
        {"name": "left-ctrl-main-strict", "hotkey": 0xA2, "strict": True, "pressed": [0x11]},
        {"name": "right-ctrl-main-strict", "hotkey": 0xA3, "strict": True, "pressed": [0xA2]},
        {"name": "generic-shift-main-strict", "hotkey": 0x10, "strict": True, "pressed": [0xA1]},
        {"name": "left-alt-main-strict", "hotkey": 0xA4, "strict": True, "pressed": [0x12]},
        {"name": "right-win-main-strict", "hotkey": 0x5C, "strict": True, "pressed": [0x5B]},
        {"name": "modifier-main-extra-family-rejected", "hotkey": 0xA2, "strict": True, "pressed": [0xA2, 0x10]},
        {"name": "mouse-button", "hotkey": 0x01, "strict": True, "pressed": [0x01]},
        {"name": "wheel-up-predicate", "hotkey": 256, "strict": False, "pressed": [256]},
        {"name": "wheel-down-predicate", "hotkey": 257, "strict": False, "pressed": [257]},
        {"name": "high-bits-ignored", "hotkey": 0xF0000000 | 0x41, "strict": True, "pressed": [0x41]},
        {"name": "all-modifiers-left", "hotkey": win | ctrl | alt | shift | 0x41, "strict": True, "pressed": [0x5B, 0xA2, 0xA4, 0xA0, 0x41]},
        {"name": "all-modifiers-right", "hotkey": win | ctrl | alt | shift | 0x41, "strict": True, "pressed": [0x5C, 0xA3, 0xA5, 0xA1, 0x41]},
        {"name": "missing-required-alt", "hotkey": ctrl | alt | 0x41, "strict": False, "pressed": [0x11, 0x41]},
    ]
    return cases


def write_input_hotkeys() -> None:
    cases = []
    for case in input_hotkey_inputs():
        observed = run_json(ORACLE, "input-hotkey", compact({
            "hotkey": case["hotkey"],
            "strict": case["strict"],
            "pressed": case["pressed"],
        }))
        cases.append({**case, "result": observed["result"]})
    write_json(
        ROOT / "compat" / "input" / "hotkey_cases.json",
        {"oracle": "smu_compat_oracle input-hotkey", "cases": cases},
    )



def write_linux_keymap() -> None:
    observed = run_json(ORACLE, "linux-keymap", "all")
    write_json(
        ROOT / "compat" / "input" / "linux_keymap.json",
        {
            "oracle": "smu_compat_oracle linux-keymap",
            "win_to_evdev": observed["win_to_evdev"],
            "evdev_to_win": observed["evdev_to_win"],
        },
    )

def roblox_regex_inputs() -> list[dict[str, str]]:
    return [
        {"name": "literal", "pattern": "server", "text": "connected to server 123"},
        {"name": "digit-class", "pattern": r"server [0-9]+", "text": "server 123"},
        {"name": "anchors-match", "pattern": r"^foo$", "text": "foo"},
        {"name": "anchors-reject", "pattern": r"^foo$", "text": "xfoo"},
        {"name": "alternation", "pattern": r"(?:foo|bar)", "text": "xxbarxx"},
        {"name": "backreference-match", "pattern": r"(ab)\1", "text": "zzababzz"},
        {"name": "backreference-reject", "pattern": r"(ab)\1", "text": "zzabaczz"},
        {"name": "positive-lookahead", "pattern": r"a(?=b)", "text": "ab"},
        {"name": "negative-lookahead", "pattern": r"a(?!b)", "text": "ac"},
        {"name": "word-class", "pattern": r"\w+", "text": "abc_123"},
        {"name": "digit-class-escape", "pattern": r"\d+", "text": "x42y"},
        {"name": "invalid-open-class", "pattern": "[", "text": "anything"},
        {"name": "invalid-open-group", "pattern": "(", "text": "anything"},
    ]


def write_roblox_regex() -> None:
    cases = []
    for case in roblox_regex_inputs():
        observed = run_json(
            ORACLE,
            "roblox-regex",
            compact({"pattern": case["pattern"], "text": case["text"]}),
        )
        cases.append({**case, "valid": observed["valid"], "matches": observed["matches"]})
    write_json(
        ROOT / "compat" / "scripting" / "roblox_regex_cases.json",
        {"oracle": "smu_compat_oracle roblox-regex", "cases": cases},
    )


def move_degrees_inputs() -> list[dict[str, Any]]:
    return [
        {"name": "default-calibration", "camfix": False, "sensitivity": "0.5", "dx": 10.0, "dy": 5.0},
        {"name": "camfix-calibration", "camfix": True, "sensitivity": "0.5", "dx": 1.0, "dy": 1.0},
        {"name": "trimmed-plus", "camfix": False, "sensitivity": "  +0.5  ", "dx": 0.125, "dy": -0.125},
        {"name": "hex-float", "camfix": False, "sensitivity": "0x1p-1", "dx": 2.5, "dy": 2.5},
        {"name": "hex-no-exponent", "camfix": False, "sensitivity": "0x1", "dx": 1.0, "dy": 1.0},
        {"name": "missing-camfix", "sensitivity": "0.5", "dx": 1.0, "dy": 1.0},
        {"name": "wrong-camfix-type", "camfix": 1, "sensitivity": "0.5", "dx": 1.0, "dy": 1.0},
        {"name": "missing-sensitivity", "camfix": False, "dx": 1.0, "dy": 1.0},
        {"name": "wrong-sensitivity-type", "camfix": False, "sensitivity": 0.5, "dx": 1.0, "dy": 1.0},
        {"name": "zero-sensitivity", "camfix": False, "sensitivity": "0", "dx": 1.0, "dy": 1.0},
        {"name": "negative-sensitivity", "camfix": False, "sensitivity": "-0.5", "dx": 1.0, "dy": 1.0},
        {"name": "nan-sensitivity", "camfix": False, "sensitivity": "nan", "dx": 1.0, "dy": 1.0},
        {"name": "junk-sensitivity", "camfix": False, "sensitivity": "0.5x", "dx": 1.0, "dy": 1.0},
        {"name": "round-positive-half", "camfix": False, "sensitivity": "4", "dx": 1.0, "dy": 0.0},
        {"name": "round-negative-half", "camfix": False, "sensitivity": "4", "dx": -1.0, "dy": 0.0},
        {"name": "range-overflow", "camfix": False, "sensitivity": "0.5", "dx": 1.0e10, "dy": 0.0},
        {"name": "floating-overflow", "camfix": False, "sensitivity": "0.5", "dx": 1.0e308, "dy": 0.0},
    ]


def write_move_degrees() -> None:
    cases = []
    for case in move_degrees_inputs():
        observed = run_json(ORACLE, "move-degrees", compact({key: value for key, value in case.items() if key != "name"}))
        cases.append({"name": case["name"], "input": {key: value for key, value in case.items() if key != "name"}, "observed": observed})
    write_json(
        ROOT / "compat" / "scripting" / "move_degrees_cases.json",
        {"oracle": "smu_compat_oracle move-degrees", "cases": cases},
    )


def script_metadata_inputs() -> list[tuple[str, str, str]]:
    return [
        ("empty-fallback", "Fallback.lua", ""),
        (
            "full-header",
            "original.smus",
            "-- @name: Custom Name\n"
            "-- @description: Long description\n"
            "-- @author: Spencer\n"
            "-- @version: 3.4.0\n"
            "-- @memoryLimitMB: 512\n"
            "-- @keybind: Ctrl+Shift+F12\n",
        ),
        (
            "normalized-keys-and-desc-precedence",
            "normalized.HSS",
            "\n-- @d_e-s c: first desc\n"
            "-- @description: canonical desc\n"
            "-- @desc: ignored desc\n"
            "-- @memory_limit-mb: +42\n"
            "-- @key_bind: Alt+F2\n",
        ),
        (
            "code-stops-scan",
            "stop.lua",
            "-- @name: Before Code\n"
            "-- ordinary comment\n"
            "print('stop')\n"
            "-- @author: ignored\n"
            "-- @keybind: F5\n",
        ),
        (
            "malformed-and-empty-values",
            "malformed.txt",
            "-- @name\n"
            "-- @author:   \n"
            "-- @unknown: value\n"
            "-- @version: 1.2.3\n",
        ),
        ("signed-memory-limit", "negative.lua", "-- @memoryLimitMB: -1\n"),
        ("overflow-memory-limit", "overflow.lua", "-- @memoryLimitMB: 18446744073709551616\n"),
        ("junk-memory-limit", "junk.lua", "-- @memoryLimitMB: 42x\n"),
        ("invalid-hotkey", "invalid.lua", "-- @keybind: Ctrl+NoSuchKey\n"),
        ("empty-name-falls-back", "fallback-name.lua", "-- @name:     \n"),
        (
            "byte-clamps",
            "clamp.lua",
            "-- @name: " + "N" * 140 + "\n"
            "-- @author: " + "A" * 140 + "\n"
            "-- @version: " + "V" * 80 + "\n"
            "-- @description: " + "D" * 1100 + "\n",
        ),
        ("unsupported-extension", "script.json", "-- @name: JSON script\n"),
    ]


def write_script_metadata() -> None:
    cases: list[dict[str, Any]] = []
    with tempfile.TemporaryDirectory(prefix="smu-script-metadata-fixtures-") as temporary:
        root = Path(temporary)
        for index, (name, filename, content) in enumerate(script_metadata_inputs()):
            directory = root / f"case-{index}"
            directory.mkdir()
            script_path = directory / filename
            script_path.write_text(content, encoding="utf-8", newline="")
            observed = run_json(ORACLE, "script-metadata", str(script_path))
            observed["path"] = script_path.name
            cases.append({"name": name, "filename": filename, "content": content, "observed": observed})

    write_json(
        ROOT / "compat" / "scripting" / "metadata_cases.json",
        {"oracle": "smu_compat_oracle script-metadata", "cases": cases},
    )


def roblox_log_inputs() -> list[tuple[str, str]]:
    return [
        (
            "basic-game-join",
            "Report game_join_loadtime placeid:123, userid:456, universeid:789,\n"
            "Joining game 'test-job'\n"
            "existing console message\n",
        ),
        (
            "udmux-and-version-metadata",
            "Connecting to UDMUX server 10.0.0.1:5000 RCC server 20.0.0.2:6000\n"
            "Server RobloxGitHash: server-hash\n"
            "Server Prefix: 0.650.2.6500123\n"
            "[FLog::ClientRunInfo] RobloxGitHash: client-hash\n"
            "[FLog::ClientRunInfo] The channel is zlive\n"
            "resizing main targets to 1920x1080\n",
        ),
        (
            "alternative-udmux-address-and-port-wrap",
            "UDMUX Address = 1.2.3.4, Port = 70000, RCC Server Address = 5.6.7.8, Port = 65537\n",
        ),
        (
            "state-transition-order",
            "returnToLuaApp\n"
            "setStage: (stage:None)\n"
            "Report game_join_loadtime placeid:44, userid:55, universeid:66,\n"
            "setStage: (stage:None)\n",
        ),
        (
            "c-numeric-prefix-and-overflow",
            "Report game_join_loadtime placeid:123abc, userid:18446744073709551616, universeid:-1,\n"
            "resizing main targets to 70000abcx65537xyz\n",
        ),
        (
            "rcc-connection",
            "Connecting to 127.0.0.1:53640\n",
        ),
        (
            "main-string-fields",
            "Joining game 'job-with-dashes' trailing text\n"
            "Server RobloxGitHash: hash:value\r\n"
            "Server Prefix: 1.2.3.4\r\n"
            "[FLog::ClientRunInfo] The channel is channel-name\r\n",
        ),
    ]


def write_roblox() -> None:
    cases: list[dict[str, Any]] = []
    with tempfile.TemporaryDirectory(prefix="smu-roblox-fixtures-") as temporary:
        root = Path(temporary)
        for index, (name, content) in enumerate(roblox_log_inputs()):
            directory = root / f"case-{index}"
            directory.mkdir()
            log_path = directory / "Player_test.log"
            log_path.write_text(content, encoding="utf-8", newline="")
            observed = run_json(ORACLE, "roblox-file", str(log_path))["snapshot"]
            observed["path"] = Path(observed["path"]).name
            cases.append({"name": name, "content": content, "snapshot": observed})

    write_json(
        ROOT / "compat" / "roblox" / "log_cases.json",
        {"oracle": "smu_compat_oracle roblox-file", "cases": cases},
    )


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


def runtime_freeze_threshold_cases() -> list[dict[str, Any]]:
    values = (-1.0, -0.001, 0.0, 0.0005, 0.001, 0.016, 0.333, 1.2345, 10.0)
    cases = []
    for seconds in values:
        observed = run_json(RUNTIME_ORACLE, "freeze-threshold", str(seconds))
        cases.append({"seconds": seconds, "threshold_ms": observed["threshold_ms"]})
    return cases


def runtime_freeze_cases() -> list[dict[str, Any]]:
    # section, foreground, pressed, was_pressed, configured_toggle, freeze_key,
    # suspended, elapsed_ms, max_seconds, override_ms, running_after_override,
    # pressed_after_override, suspend_success, resume_success
    specifications = [
        ("section-disabled-idle", False, True, True, True, False, 4, False, 0, 5.0, 100, True, True, True, True),
        ("section-disabled-resume-success", False, True, False, True, False, 4, True, 100, 5.0, 100, True, True, True, True),
        ("section-disabled-resume-failure", False, True, False, True, False, 4, True, 100, 5.0, 100, True, True, True, False),
        ("hold-press-suspends", True, True, True, False, False, 4, False, 50, 5.0, 100, True, True, True, True),
        ("hold-still-held-resuspends-without-edge", True, True, True, True, False, 4, False, 50, 5.0, 100, True, True, True, True),
        ("hold-press-suspend-fails", True, True, True, False, False, 4, False, 50, 5.0, 100, True, True, False, True),
        ("hold-release-resumes", True, True, False, True, False, 4, True, 50, 5.0, 100, True, False, True, True),
        ("hold-foreground-loss-resumes-even-if-pressed", True, False, True, True, False, 4, True, 50, 5.0, 100, True, True, True, True),
        ("toggle-rising-suspends", True, True, True, False, True, 4, False, 50, 5.0, 100, True, True, True, True),
        ("toggle-held-does-not-retrigger", True, True, True, True, True, 4, False, 50, 5.0, 100, True, True, True, True),
        ("toggle-rising-resumes", True, True, True, False, True, 4, True, 50, 5.0, 100, True, True, True, True),
        ("toggle-rising-outside-consumes-edge", True, False, True, False, True, 4, False, 50, 5.0, 100, True, True, True, True),
        ("toggle-survives-foreground-loss", True, False, False, True, True, 4, True, 50, 5.0, 100, True, False, True, True),
        ("wheel-up-forces-toggle", True, True, True, False, False, 256, False, 50, 5.0, 100, True, True, True, True),
        ("wheel-up-held-does-not-retrigger", True, True, True, True, False, 256, False, 50, 5.0, 100, True, True, True, True),
        ("wheel-down-forces-toggle", True, True, True, False, False, 257, False, 50, 5.0, 100, True, True, True, True),
        ("timeout-below-threshold", True, True, False, False, True, 4, True, 4999, 5.0, 100, True, False, True, True),
        ("timeout-at-threshold-toggle-resuspends", True, True, False, False, True, 4, True, 5000, 5.0, 100, True, False, True, True),
        ("timeout-hold-recheck-pressed-resuspends", True, True, True, True, False, 4, True, 5000, 5.0, 100, True, True, True, True),
        ("timeout-hold-recheck-released-stays-resumed", True, True, True, True, False, 4, True, 5000, 5.0, 100, True, False, True, True),
        ("timeout-running-false-skips-hotkey-recheck", True, True, True, True, False, 4, True, 5000, 5.0, 100, False, True, True, True),
        ("timeout-negative-override-clamps-zero", True, True, False, False, True, 4, True, 5000, 5.0, -99, True, False, True, True),
        ("timeout-resume-failure-still-sleeps-and-resuspends", True, True, False, False, True, 4, True, 5000, 5.0, 25, True, False, True, False),
        ("zero-timeout-new-hold-suspend-immediately-cycles", True, True, True, False, False, 4, False, 123, 0.0, 10, True, True, True, True),
        ("negative-timeout-is-immediate", True, True, False, False, True, 4, True, 0, -0.001, 1, True, False, True, True),
    ]
    cases = []
    for spec in specifications:
        (
            name,
            section,
            foreground,
            pressed,
            was_pressed,
            configured_toggle,
            freeze_key,
            suspended,
            elapsed_ms,
            max_seconds,
            override_ms,
            running_after_override,
            pressed_after_override,
            suspend_success,
            resume_success,
        ) = spec
        observed = run_json(
            RUNTIME_ORACLE,
            "freeze-step",
            str(int(section)),
            str(int(foreground)),
            str(int(pressed)),
            str(int(was_pressed)),
            str(int(configured_toggle)),
            str(freeze_key),
            str(int(suspended)),
            str(elapsed_ms),
            str(max_seconds),
            str(override_ms),
            str(int(running_after_override)),
            str(int(pressed_after_override)),
            str(int(suspend_success)),
            str(int(resume_success)),
        )
        cases.append(
            {
                "name": name,
                "section_enabled": section,
                "foreground_allowed": foreground,
                "pressed": pressed,
                "was_pressed": was_pressed,
                "configured_toggle": configured_toggle,
                "freeze_key": freeze_key,
                "suspended": suspended,
                "elapsed_ms": elapsed_ms,
                "max_seconds": max_seconds,
                "override_ms": override_ms,
                "running_after_override": running_after_override,
                "pressed_after_override": pressed_after_override,
                "suspend_success": suspend_success,
                "resume_success": resume_success,
                "toggle_freeze": observed["toggle_freeze"],
                "next_was_pressed": observed["next_was_pressed"],
                "final_suspended": observed["final_suspended"],
                "events": observed["events"],
            }
        )
    return cases


def runtime_target_suspend_cases() -> list[dict[str, Any]]:
    specifications = [
        (
            "suspend-no-target-after-refresh",
            {
                "target_pids": [],
                "frozen_pids": [],
                "freeze_suspended": False,
                "request_suspended": True,
                "refresh_results": [[]],
                "attempt_results": [],
            },
        ),
        (
            "suspend-single-success",
            {
                "target_pids": [10],
                "frozen_pids": [],
                "freeze_suspended": False,
                "request_suspended": True,
                "refresh_results": [],
                "attempt_results": [True],
            },
        ),
        (
            "suspend-partial-success-skips-refresh",
            {
                "target_pids": [10, 20],
                "frozen_pids": [],
                "freeze_suspended": False,
                "request_suspended": True,
                "refresh_results": [[30]],
                "attempt_results": [False, True],
            },
        ),
        (
            "suspend-all-fail-same-pids-no-retry",
            {
                "target_pids": [10, 20],
                "frozen_pids": [],
                "freeze_suspended": False,
                "request_suspended": True,
                "refresh_results": [[10, 20]],
                "attempt_results": [False, False],
            },
        ),
        (
            "suspend-all-fail-reordered-pids-retries",
            {
                "target_pids": [10, 20],
                "frozen_pids": [],
                "freeze_suspended": False,
                "request_suspended": True,
                "refresh_results": [[20, 10]],
                "attempt_results": [False, False, True, False],
            },
        ),
        (
            "suspend-all-fail-new-pids-partial-retry-success",
            {
                "target_pids": [10, 20],
                "frozen_pids": [],
                "freeze_suspended": False,
                "request_suspended": True,
                "refresh_results": [[30, 40]],
                "attempt_results": [False, False, False, True],
            },
        ),
        (
            "suspend-all-fail-refresh-empty-keeps-old-frozen",
            {
                "target_pids": [10, 20],
                "frozen_pids": [],
                "freeze_suspended": False,
                "request_suspended": True,
                "refresh_results": [[]],
                "attempt_results": [False, False],
            },
        ),
        (
            "suspend-empty-target-two-stage-refresh-retry",
            {
                "target_pids": [],
                "frozen_pids": [],
                "freeze_suspended": False,
                "request_suspended": True,
                "refresh_results": [[7], [8]],
                "attempt_results": [False, True],
            },
        ),
        (
            "suspend-empty-target-refresh-empty-preserves-old-frozen",
            {
                "target_pids": [],
                "frozen_pids": [99],
                "freeze_suspended": True,
                "request_suspended": True,
                "refresh_results": [[]],
                "attempt_results": [],
            },
        ),
        (
            "resume-partial-success-clears-entire-frozen-set",
            {
                "target_pids": [30, 40],
                "frozen_pids": [10, 20],
                "freeze_suspended": True,
                "request_suspended": False,
                "refresh_results": [[50]],
                "attempt_results": [False, True],
            },
        ),
        (
            "resume-all-fail-retains-frozen-and-state",
            {
                "target_pids": [30],
                "frozen_pids": [10, 20],
                "freeze_suspended": True,
                "request_suspended": False,
                "refresh_results": [[50]],
                "attempt_results": [False, False],
            },
        ),
        (
            "resume-empty-frozen-does-nothing",
            {
                "target_pids": [30],
                "frozen_pids": [],
                "freeze_suspended": True,
                "request_suspended": False,
                "refresh_results": [[50]],
                "attempt_results": [],
            },
        ),
        (
            "suspend-while-already-suspended-still-calls-backend",
            {
                "target_pids": [10],
                "frozen_pids": [9],
                "freeze_suspended": True,
                "request_suspended": True,
                "refresh_results": [],
                "attempt_results": [True],
            },
        ),
        (
            "resume-while-state-false-still-uses-frozen-pids",
            {
                "target_pids": [30],
                "frozen_pids": [10],
                "freeze_suspended": False,
                "request_suspended": False,
                "refresh_results": [],
                "attempt_results": [True],
            },
        ),
        (
            "duplicate-target-pids-are-not-deduplicated",
            {
                "target_pids": [10, 10],
                "frozen_pids": [],
                "freeze_suspended": False,
                "request_suspended": True,
                "refresh_results": [],
                "attempt_results": [False, True],
            },
        ),
    ]
    cases = []
    for name, spec in specifications:
        observed = run_json(RUNTIME_ORACLE, "target-suspend", compact(spec))
        cases.append(
            {
                "name": name,
                "spec": spec,
                "any_success": observed["any_success"],
                "warning": observed["warning"],
                "reset_freeze_start": observed["reset_freeze_start"],
                "target_pids": observed["target_pids"],
                "frozen_pids": observed["frozen_pids"],
                "freeze_suspended": observed["freeze_suspended"],
                "events": observed["events"],
            }
        )
    return cases


def runtime_item_desync_cases() -> list[dict[str, Any]]:
    specifications = []
    case_index = 0
    for foreground in (False, True):
        for section in (False, True):
            for hotkey in (False, True):
                specifications.append((f"matrix-{case_index}", foreground, section, hotkey, 1))
                case_index += 1
    for slot in (-99, -1, 0, 1, 9, 10, 99):
        specifications.append((f"active-slot-{slot}", True, True, True, slot))

    cases = []
    for name, foreground, section, hotkey, slot in specifications:
        observed = run_json(
            RUNTIME_ORACLE,
            "item-desync",
            str(int(foreground)),
            str(int(section)),
            str(int(hotkey)),
            str(slot),
        )
        cases.append(
            {
                "name": name,
                "foreground_allowed": foreground,
                "section_enabled": section,
                "hotkey_pressed": hotkey,
                "slot": slot,
                "pressed": observed["pressed"],
                "loop_state": observed["loop_state"],
                "slot_key": observed["slot_key"],
                "events": observed["events"],
            }
        )
    return cases


def runtime_itemclip_cases() -> list[dict[str, Any]]:
    cases = []
    case_index = 0
    for foreground in (False, True):
        for section_press in (False, True):
            for section_execute in (False, True):
                for hotkey in (False, True):
                    for toggle_switch in (False, True):
                        for was_pressed in (False, True):
                            for is_loop in (False, True):
                                observed = run_json(
                                    RUNTIME_ORACLE,
                                    "itemclip-step",
                                    str(int(foreground)),
                                    str(int(section_press)),
                                    str(int(section_execute)),
                                    str(int(hotkey)),
                                    str(int(toggle_switch)),
                                    str(int(was_pressed)),
                                    str(int(is_loop)),
                                    "5",
                                    "1",
                                )
                                cases.append(
                                    {
                                        "name": f"matrix-{case_index}",
                                        "foreground_allowed": foreground,
                                        "section_press": section_press,
                                        "section_execute": section_execute,
                                        "hotkey_pressed": hotkey,
                                        "toggle_switch": toggle_switch,
                                        "was_pressed": was_pressed,
                                        "is_loop": is_loop,
                                        "clip_delay": 5,
                                        "clip_slot": 1,
                                        "pressed": observed["pressed"],
                                        "next_was_pressed": observed["next_was_pressed"],
                                        "state_action": observed["state_action"],
                                        "next_is_loop": observed["next_is_loop"],
                                        "execute_cycle": observed["execute_cycle"],
                                        "plan": observed["plan"],
                                        "events": observed["events"],
                                    }
                                )
                                case_index += 1

    for clip_delay in (-99, -3, -2, -1, 0, 1, 2, 3, 4, 5, 16, 17, 1001):
        for clip_slot in (-99, 0, 1, 9, 99):
            observed = run_json(
                RUNTIME_ORACLE,
                "itemclip-step",
                "1",
                "1",
                "1",
                "0",
                "0",
                "0",
                "1",
                str(clip_delay),
                str(clip_slot),
            )
            cases.append(
                {
                    "name": f"active-delay-{clip_delay}-slot-{clip_slot}",
                    "foreground_allowed": True,
                    "section_press": True,
                    "section_execute": True,
                    "hotkey_pressed": False,
                    "toggle_switch": False,
                    "was_pressed": False,
                    "is_loop": True,
                    "clip_delay": clip_delay,
                    "clip_slot": clip_slot,
                    "pressed": observed["pressed"],
                    "next_was_pressed": observed["next_was_pressed"],
                    "state_action": observed["state_action"],
                    "next_is_loop": observed["next_is_loop"],
                    "execute_cycle": observed["execute_cycle"],
                    "plan": observed["plan"],
                    "events": observed["events"],
                }
            )
    return cases


def runtime_wallwalk_delay_cases() -> list[dict[str, Any]]:
    cases = []
    for fps in (*range(0, 1001), 10_000, 1_000_000, 4_294_967_295):
        observed = run_json(
            RUNTIME_ORACLE,
            "wallwalk-step",
            "1",
            "1",
            "1",
            "0",
            "0",
            "0",
            "1",
            str(fps),
            "0",
            "1",
            "0",
            "2",
            "0",
        )
        cases.append({"fps": fps, "delay_ms": observed["plan"]["frame_delay_ms"]})
    return cases


def runtime_wallwalk_cases() -> list[dict[str, Any]]:
    cases = []
    case_index = 0
    for foreground in (False, True):
        for section_press in (False, True):
            for section_execute in (False, True):
                for hotkey in (False, True):
                    for toggle_switch in (False, True):
                        for was_pressed in (False, True):
                            for is_loop in (False, True):
                                observed = run_json(
                                    RUNTIME_ORACLE,
                                    "wallwalk-step",
                                    str(int(foreground)),
                                    str(int(section_press)),
                                    str(int(section_execute)),
                                    str(int(hotkey)),
                                    str(int(toggle_switch)),
                                    str(int(was_pressed)),
                                    str(int(is_loop)),
                                    "200",
                                    "0",
                                    "300",
                                    "1",
                                    "301",
                                    "250",
                                )
                                cases.append(
                                    {
                                        "name": f"matrix-{case_index}",
                                        "foreground_allowed": foreground,
                                        "section_press": section_press,
                                        "section_execute": section_execute,
                                        "hotkey_pressed": hotkey,
                                        "toggle_switch": toggle_switch,
                                        "was_pressed": was_pressed,
                                        "is_loop": is_loop,
                                        "fps": 200,
                                        "side_first": False,
                                        "strength_x": 300,
                                        "side_second": True,
                                        "strength_y": 301,
                                        "tail_delay_us": 250,
                                        "pressed": observed["pressed"],
                                        "next_was_pressed": observed["next_was_pressed"],
                                        "state_action": observed["state_action"],
                                        "next_is_loop": observed["next_is_loop"],
                                        "execute_cycle": observed["execute_cycle"],
                                        "plan": observed["plan"],
                                        "events": observed["events"],
                                    }
                                )
                                case_index += 1

    variants = [
        ("tail-negative-clamps-zero", 144, False, 321, False, -123, -99),
        ("tail-zero", 144, False, 321, False, -123, 0),
        ("tail-one-us", 144, False, 321, False, -123, 1),
        ("tail-999-us", 144, False, 321, False, -123, 999),
        ("tail-1000-us", 144, False, 321, False, -123, 1000),
        ("tail-1001-us", 144, False, 321, False, -123, 1001),
        ("both-sides-negative", 60, True, 321, True, 123, 25),
        ("mixed-side-snapshots", 60, True, 321, False, 123, 25),
    ]
    for name, fps, side_first, strength_x, side_second, strength_y, tail_delay_us in variants:
        observed = run_json(
            RUNTIME_ORACLE,
            "wallwalk-step",
            "1",
            "1",
            "1",
            "0",
            "0",
            "0",
            "1",
            str(fps),
            str(int(side_first)),
            str(strength_x),
            str(int(side_second)),
            str(strength_y),
            str(tail_delay_us),
        )
        cases.append(
            {
                "name": name,
                "foreground_allowed": True,
                "section_press": True,
                "section_execute": True,
                "hotkey_pressed": False,
                "toggle_switch": False,
                "was_pressed": False,
                "is_loop": True,
                "fps": fps,
                "side_first": side_first,
                "strength_x": strength_x,
                "side_second": side_second,
                "strength_y": strength_y,
                "tail_delay_us": tail_delay_us,
                "pressed": observed["pressed"],
                "next_was_pressed": observed["next_was_pressed"],
                "state_action": observed["state_action"],
                "next_is_loop": observed["next_is_loop"],
                "execute_cycle": observed["execute_cycle"],
                "plan": observed["plan"],
                "events": observed["events"],
            }
        )
    return cases


def runtime_wallhop_cases() -> list[dict[str, Any]]:
    cases = []
    case_index = 0
    for delay in (-5, 0, 1, 19, 99, 100, 101):
        for bonus in (-5, 0, 1, 18, 19, 100):
            for left in (False, True):
                for jump in (False, True):
                    for flick_back in (False, True):
                        observed = run_json(
                            RUNTIME_ORACLE,
                            "wallhop-worker",
                            "300",
                            "-301",
                            "7",
                            str(delay),
                            str(bonus),
                            str(int(left)),
                            str(int(jump)),
                            str(int(flick_back)),
                            "32",
                        )
                        cases.append(
                            {
                                "name": f"matrix-{case_index}",
                                "dx": 300,
                                "dy": -301,
                                "vertical": 7,
                                "delay_ms": delay,
                                "bonus_delay_ms": bonus,
                                "left": left,
                                "jump": jump,
                                "flick_back": flick_back,
                                "jump_key": 32,
                                "plan": observed["plan"],
                                "events": observed["events"],
                            }
                        )
                        case_index += 1
    return cases


def runtime_walless_cases() -> list[dict[str, Any]]:
    cases = []
    case_index = 0
    pid_sets = ([], [10], [10, 20])
    for pids in pid_sets:
        for side_left in (False, True):
            for global_zoom_action in (False, True):
                for zoom_reverse in (False, True):
                    for global_zoom_release in (False, True):
                        spec = {
                            "pids": pids,
                            "side_left": side_left,
                            "global_zoom_action": global_zoom_action,
                            "shift_key_action": 160,
                            "zoom_reverse": zoom_reverse,
                            "global_zoom_release": global_zoom_release,
                            "shift_key_release": 161,
                        }
                        observed = run_json(
                            RUNTIME_ORACLE,
                            "walless-worker",
                            compact(spec),
                        )
                        cases.append(
                            {
                                "name": f"matrix-{case_index}",
                                "spec": spec,
                                "events": observed["events"],
                            }
                        )
                        case_index += 1

    for name, spec in [
        (
            "shift-key-changes-before-release",
            {
                "pids": [7, 8],
                "side_left": True,
                "global_zoom_action": False,
                "shift_key_action": 160,
                "zoom_reverse": False,
                "global_zoom_release": False,
                "shift_key_release": 161,
            },
        ),
        (
            "shift-action-then-zoom-release-skips-key-up",
            {
                "pids": [7],
                "side_left": False,
                "global_zoom_action": False,
                "shift_key_action": 160,
                "zoom_reverse": False,
                "global_zoom_release": True,
                "shift_key_release": 161,
            },
        ),
        (
            "zoom-action-then-shift-release-releases-live-shift",
            {
                "pids": [7],
                "side_left": False,
                "global_zoom_action": True,
                "shift_key_action": 160,
                "zoom_reverse": True,
                "global_zoom_release": False,
                "shift_key_release": 162,
            },
        ),
        (
            "wheel-up-action",
            {
                "pids": [9],
                "side_left": False,
                "global_zoom_action": True,
                "shift_key_action": 160,
                "zoom_reverse": False,
                "global_zoom_release": True,
                "shift_key_release": 161,
            },
        ),
        (
            "wheel-down-action",
            {
                "pids": [9],
                "side_left": True,
                "global_zoom_action": True,
                "shift_key_action": 160,
                "zoom_reverse": True,
                "global_zoom_release": True,
                "shift_key_release": 161,
            },
        ),
    ]:
        observed = run_json(RUNTIME_ORACLE, "walless-worker", compact(spec))
        cases.append({"name": name, "spec": spec, "events": observed["events"]})
    return cases


def runtime_hhj_cases() -> list[dict[str, Any]]:
    base = {
        "pids": [10, 20],
        "auto_toggle_start": False,
        "auto_toggle_release": False,
        "auto_key1_action": 32,
        "auto_key1_release": 32,
        "auto_key1_time": 550,
        "auto_key2_action": 87,
        "auto_key2_release": 87,
        "auto_key2_time": 68,
        "freeze_delay_apply": False,
        "fast_hhj": False,
        "freeze_delay_override_check": 500,
        "freeze_delay_override_value": 500,
        "delay1": 9,
        "global_zoom_action": False,
        "shift_key_action": 16,
        "zoom_reverse": False,
        "delay2": 17,
        "length_activation_check": 243,
        "delay3": 16,
        "global_zoom_release": False,
        "shift_key_release": 16,
        "length_finish_check": 243,
        "length_sleep_value": 243,
    }
    variants = [
        ("baseline", {}),
        ("empty-pid-snapshot", {"pids": []}),
        ("fast-hhj-skips-300ms", {"fast_hhj": True}),
        (
            "freeze-override-positive",
            {
                "freeze_delay_apply": True,
                "freeze_delay_override_check": 37,
                "freeze_delay_override_value": 37,
            },
        ),
        (
            "freeze-override-check-zero-skips-wait",
            {
                "freeze_delay_apply": True,
                "freeze_delay_override_check": 0,
                "freeze_delay_override_value": 99,
            },
        ),
        (
            "freeze-override-value-reread-zero",
            {
                "freeze_delay_apply": True,
                "freeze_delay_override_check": 99,
                "freeze_delay_override_value": 0,
            },
        ),
        (
            "auto-toggle-press-and-release",
            {
                "auto_toggle_start": True,
                "auto_toggle_release": True,
                "auto_key1_time": 5,
                "auto_key2_time": 7,
            },
        ),
        (
            "auto-keys-change-before-release",
            {
                "auto_toggle_start": True,
                "auto_toggle_release": True,
                "auto_key1_action": 32,
                "auto_key1_release": 33,
                "auto_key1_time": 5,
                "auto_key2_action": 87,
                "auto_key2_release": 88,
                "auto_key2_time": 7,
            },
        ),
        (
            "auto-toggle-turns-off-before-release",
            {
                "auto_toggle_start": True,
                "auto_toggle_release": False,
                "auto_key1_time": 1,
                "auto_key2_time": 1,
            },
        ),
        (
            "auto-toggle-turns-on-before-release",
            {
                "auto_toggle_start": False,
                "auto_toggle_release": True,
                "auto_key1_release": 33,
                "auto_key2_release": 88,
            },
        ),
        (
            "negative-auto-times-clamp-zero",
            {
                "auto_toggle_start": True,
                "auto_toggle_release": False,
                "auto_key1_time": -50,
                "auto_key2_time": -1,
            },
        ),
        (
            "negative-tail-delays-clamp-zero",
            {"delay1": -9, "delay2": -17, "delay3": -16},
        ),
        (
            "shift-key-changes-before-release",
            {"shift_key_action": 160, "shift_key_release": 161},
        ),
        (
            "shift-action-then-zoom-release-skips-key-up",
            {"shift_key_action": 160, "global_zoom_release": True},
        ),
        (
            "zoom-up-action-then-shift-release",
            {
                "global_zoom_action": True,
                "zoom_reverse": False,
                "global_zoom_release": False,
                "shift_key_release": 162,
            },
        ),
        (
            "zoom-down-action",
            {
                "global_zoom_action": True,
                "zoom_reverse": True,
                "global_zoom_release": True,
            },
        ),
        (
            "length-activation-positive-finish-zero-leaves-active",
            {
                "length_activation_check": 100,
                "length_finish_check": 0,
                "length_sleep_value": 999,
            },
        ),
        (
            "length-activation-zero-finish-positive-clears-without-activation",
            {
                "length_activation_check": 0,
                "length_finish_check": 100,
                "length_sleep_value": 11,
            },
        ),
        (
            "length-sleep-value-is-third-live-read",
            {
                "length_activation_check": 200,
                "length_finish_check": 300,
                "length_sleep_value": 7,
            },
        ),
        (
            "zero-length-everywhere",
            {
                "length_activation_check": 0,
                "length_finish_check": 0,
                "length_sleep_value": 0,
            },
        ),
    ]
    cases = []
    for name, changes in variants:
        spec = dict(base)
        spec.update(changes)
        observed = run_json(RUNTIME_ORACLE, "hhj-worker", compact(spec))
        cases.append({"name": name, "spec": spec, "events": observed["events"]})
    return cases


def runtime_floorbounce_cases() -> list[dict[str, Any]]:
    base = {
        "pids": [10, 20],
        "hhj_enabled": False,
        "delay1": 5,
        "shift_key_action": 16,
        "delay2": 8,
        "delay3": 100,
        "shift_key_release": 16,
    }
    variants = [
        ("baseline-hhj-off", {}),
        ("empty-pids", {"pids": []}),
        ("hhj-defaults", {"hhj_enabled": True}),
        (
            "negative-hhj-delays-clamp-zero",
            {"hhj_enabled": True, "delay1": -5, "delay2": -8, "delay3": -100},
        ),
        (
            "shift-key-changes-before-release",
            {"hhj_enabled": True, "shift_key_action": 160, "shift_key_release": 161},
        ),
        (
            "zero-hhj-delays",
            {"hhj_enabled": True, "delay1": 0, "delay2": 0, "delay3": 0},
        ),
        (
            "large-hhj-delays",
            {"hhj_enabled": True, "delay1": 999, "delay2": 1001, "delay3": 1234},
        ),
    ]
    cases = []
    for name, changes in variants:
        spec = dict(base)
        spec.update(changes)
        observed = run_json(RUNTIME_ORACLE, "floorbounce-worker", compact(spec))
        cases.append({"name": name, "spec": spec, "events": observed["events"]})
    return cases


def runtime_bunnyhop_physical_cases() -> list[dict[str, Any]]:
    hotkeys = [
        0,
        1,
        2,
        3,
        4,
        5,
        6,
        7,
        32,
        90,
        256,
        257,
        0x10000 | 32,
        0x20000 | 1,
        0x40000 | 256,
        0xF0000 | 90,
    ]
    cases = []
    for hotkey in hotkeys:
        observed = run_json(RUNTIME_ORACLE, "bunnyhop-physical-plan", str(hotkey))
        cases.append(
            {
                "hotkey": hotkey,
                "key": observed["key"],
                "source": observed["source"],
            }
        )
    return cases


def runtime_bunnyhop_guard_cases() -> list[dict[str, Any]]:
    cases = []
    case_index = 0
    for running in (False, True):
        for keep_running in (False, True):
            for loop in (False, True):
                for macro in (False, True):
                    for notbinding in (False, True):
                        for section in (False, True):
                            observed = run_json(
                                RUNTIME_ORACLE,
                                "bunnyhop-guards",
                                str(int(running)),
                                str(int(keep_running)),
                                str(int(loop)),
                                str(int(macro)),
                                str(int(notbinding)),
                                str(int(section)),
                            )
                            cases.append(
                                {
                                    "name": f"matrix-{case_index}",
                                    "running": running,
                                    "keep_running": keep_running,
                                    "loop": loop,
                                    "macro": macro,
                                    "notbinding": notbinding,
                                    "section": section,
                                    "loop_head": observed["loop_head"],
                                    "after_half": observed["after_half"],
                                }
                            )
                            case_index += 1
    return cases


def runtime_bunnyhop_iteration_cases() -> list[dict[str, Any]]:
    cases = []
    case_index = 0
    for output_key in (1, 32, 90, 0x10000 | 32):
        for delay in (-99, -1, 0, 1, 2, 3, 4, 5, 99, 100, 101):
            for continue_after_half in (False, True):
                observed = run_json(
                    RUNTIME_ORACLE,
                    "bunnyhop-iteration",
                    str(output_key),
                    str(delay),
                    str(int(continue_after_half)),
                )
                cases.append(
                    {
                        "name": f"matrix-{case_index}",
                        "output_key": output_key,
                        "delay_ms": delay,
                        "continue_after_half": continue_after_half,
                        "half_delay_ms": observed["half_delay_ms"],
                        "completed": observed["completed"],
                        "events": observed["events"],
                    }
                )
                case_index += 1
    return cases


def runtime_anti_afk_timer_cases() -> list[dict[str, Any]]:
    cases = []
    case_index = 0
    for armed in (False, True):
        for foreground in (False, True):
            for serial_changed in (False, True):
                for configured_minutes in (-5, 0, 1, 15):
                    for elapsed_ms in (0, 59_999, 60_000, 899_999, 900_000, 959_999):
                        last_serial = 10
                        keyboard_serial = 11 if serial_changed else 10
                        observed = run_json(
                            RUNTIME_ORACLE,
                            "anti-afk-timer",
                            str(int(armed)),
                            str(last_serial),
                            str(keyboard_serial),
                            str(int(foreground)),
                            str(configured_minutes),
                            str(elapsed_ms),
                        )
                        cases.append(
                            {
                                "name": f"matrix-{case_index}",
                                "armed": armed,
                                "last_serial": last_serial,
                                "keyboard_serial": keyboard_serial,
                                "foreground": foreground,
                                "configured_minutes": configured_minutes,
                                "elapsed_ms": elapsed_ms,
                                "next_armed": observed["armed"],
                                "next_last_serial": observed["last_serial"],
                                "reset_activity": observed["reset_activity"],
                                "due": observed["due"],
                                "threshold_minutes": observed["threshold_minutes"],
                            }
                        )
                        case_index += 1
    return cases


def runtime_anti_afk_keypress_cases() -> list[dict[str, Any]]:
    cases = []
    for key in (112, 0x10000 | 112, 0xF0000 | 90):
        for double_press in (False, True):
            observed = run_json(
                RUNTIME_ORACLE,
                "anti-afk-keypress",
                str(key),
                str(int(double_press)),
            )
            cases.append(
                {
                    "name": f"key-{key}-double-{int(double_press)}",
                    "key": key,
                    "double_press": double_press,
                    "events": observed["events"],
                }
            )
    return cases


def runtime_anti_afk_tap_cases() -> list[dict[str, Any]]:
    cases = []
    for rect_available in (False, True):
        for has_mouse_pos in (False, True):
            observed = run_json(
                RUNTIME_ORACLE,
                "anti-afk-tap",
                str(int(rect_available)),
                str(int(has_mouse_pos)),
            )
            cases.append(
                {
                    "name": f"rect-{int(rect_available)}-mouse-{int(has_mouse_pos)}",
                    "rect_available": rect_available,
                    "has_mouse_pos": has_mouse_pos,
                    "success": observed["success"],
                    "events": observed["events"],
                }
            )
    return cases


def runtime_anti_afk_background_cases() -> list[dict[str, Any]]:
    cases = []
    case_index = 0
    for original_foreground in (False, True):
        for outer_mouse_pos in (False, True):
            for tap_rect in (False, True):
                for tap_mouse_pos in (False, True):
                    for double_press in (False, True):
                        observed = run_json(
                            RUNTIME_ORACLE,
                            "anti-afk-background",
                            str(int(original_foreground)),
                            str(int(outer_mouse_pos)),
                            str(int(tap_rect)),
                            str(int(tap_mouse_pos)),
                            str(int(double_press)),
                        )
                        cases.append(
                            {
                                "name": f"matrix-{case_index}",
                                "original_foreground": original_foreground,
                                "outer_mouse_pos": outer_mouse_pos,
                                "tap_rect": tap_rect,
                                "tap_mouse_pos": tap_mouse_pos,
                                "double_press": double_press,
                                "events": observed["events"],
                            }
                        )
                        case_index += 1
    return cases


def runtime_spam_controller_cases() -> list[dict[str, Any]]:
    cases = []
    case_index = 0
    for section in (False, True):
        for allowed in (False, True):
            for hotkey in (False, True):
                for was_pressed in (False, True):
                    for hold_mode in (False, True):
                        for thread_active in (False, True):
                            observed = run_json(
                                RUNTIME_ORACLE,
                                "spam-controller",
                                str(int(section)),
                                str(int(allowed)),
                                str(int(hotkey)),
                                str(int(was_pressed)),
                                str(int(hold_mode)),
                                str(int(thread_active)),
                            )
                            cases.append(
                                {
                                    "name": f"matrix-{case_index}",
                                    "section": section,
                                    "allowed_input": allowed,
                                    "hotkey": hotkey,
                                    "was_pressed": was_pressed,
                                    "hold_mode": hold_mode,
                                    "thread_active": thread_active,
                                    "allowed": observed["allowed"],
                                    "pressed": observed["pressed"],
                                    "next_was_pressed": observed["next_was_pressed"],
                                    "thread_action": observed["thread_action"],
                                    "next_thread_active": observed["next_thread_active"],
                                    "should_start_worker": observed["should_start_worker"],
                                }
                            )
                            case_index += 1
    return cases


def runtime_spam_guard_cases() -> list[dict[str, Any]]:
    cases = []
    case_index = 0
    for running in (False, True):
        for keep_running in (False, True):
            for thread_active in (False, True):
                for macro_toggled in (False, True):
                    for not_binding in (False, True):
                        for section in (False, True):
                            observed = run_json(
                                RUNTIME_ORACLE,
                                "spam-guard",
                                str(int(running)),
                                str(int(keep_running)),
                                str(int(thread_active)),
                                str(int(macro_toggled)),
                                str(int(not_binding)),
                                str(int(section)),
                            )
                            cases.append(
                                {
                                    "name": f"matrix-{case_index}",
                                    "running": running,
                                    "keep_running": keep_running,
                                    "thread_active": thread_active,
                                    "macro_toggled": macro_toggled,
                                    "not_binding": not_binding,
                                    "section": section,
                                    "should_run": observed["should_run"],
                                }
                            )
                            case_index += 1
    return cases


def runtime_spam_iteration_cases() -> list[dict[str, Any]]:
    cases = []
    case_index = 0
    for delay_ms in (-99, -1, 0, 1, 2, 17, 1000):
        for key_down, key_up in ((65, 65), (65, 66), (0x10000 | 112, 0x10000 | 113)):
            observed = run_json(
                RUNTIME_ORACLE,
                "spam-iteration",
                str(delay_ms),
                str(key_down),
                str(key_up),
            )
            cases.append(
                {
                    "name": f"case-{case_index}",
                    "delay_ms": delay_ms,
                    "key_down": key_down,
                    "key_up": key_up,
                    "events": observed["events"],
                }
            )
            case_index += 1
    return cases


def runtime_hhj_motion_cases() -> list[dict[str, Any]]:
    cases = []
    case_index = 0
    for section in (False, True):
        for is_hhj in (False, True):
            observed = run_json(
                RUNTIME_ORACLE,
                "hhj-motion",
                str(int(section)),
                str(int(is_hhj)),
                "144",
                "10",
                "-20",
            )
            cases.append(
                {
                    "name": f"matrix-{case_index}",
                    "section": section,
                    "is_hhj": is_hhj,
                    "fps": 144,
                    "strength_x": 10,
                    "strength_y": -20,
                    "execute": observed["execute"],
                    "events": observed["events"],
                }
            )
            case_index += 1
    return cases


def runtime_ledge_turn_cases() -> list[dict[str, Any]]:
    values = (
        "",
        "0",
        "-1",
        "0.009",
        "0.01",
        "0.1",
        "0.5",
        "1",
        "1.25junk",
        "  +2.5tail",
        "abc",
        "nan",
        "inf",
        "-inf",
        "1e2",
        "1e-3",
    )
    cases = []
    for sensitivity in values:
        for camfix in (False, True):
            observed = run_json(
                RUNTIME_ORACLE,
                "ledge-turn",
                sensitivity,
                str(int(camfix)),
            )
            cases.append(
                {
                    "sensitivity": sensitivity,
                    "camfix": camfix,
                    "turn90": observed["turn90"],
                }
            )
    return cases


def runtime_ledge_worker_cases() -> list[dict[str, Any]]:
    cases = []
    case_index = 0
    for camfix in (False, True):
        for side_left in (False, True):
            for realign in (False, True):
                for auto_hold in (False, True):
                    spec = {
                        "sensitivity": "0.5",
                        "camfix": camfix,
                        "side_left": side_left,
                        "realign": realign,
                        "auto_hold": auto_hold,
                    }
                    observed = run_json(RUNTIME_ORACLE, "ledge-worker", compact(spec))
                    cases.append(
                        {
                            "name": f"matrix-{case_index}",
                            "spec": spec,
                            "events": observed["events"],
                        }
                    )
                    case_index += 1
    return cases


def runtime_unequip_cases() -> list[dict[str, Any]]:
    base = {
        "custom_text": "", "emote_text": "/e wave", "paste_delay": 5,
        "use_old_paste": False, "enter_action": 13, "enter_release": 13,
        "dropdown_first": 0, "dropdown_zero": 0, "dropdown_one": 0,
        "speed_slot": 1, "unequip_toggle": False,
    }
    variants = [
        ("custom-text-short-circuit", {"custom_text": "hello", "emote_text": "/e unused"}),
        ("dropdown-zero", {}),
        ("dropdown-one", {"dropdown_first": 1, "dropdown_zero": 1, "dropdown_one": 1}),
        ("dropdown-two", {"dropdown_first": 2, "dropdown_zero": 2, "dropdown_one": 2}),
        ("dropdown-other", {"dropdown_first": 3, "dropdown_zero": 3, "dropdown_one": 3}),
        ("dropdown-live-reads-diverge", {"dropdown_first": 2, "dropdown_zero": 0, "dropdown_one": 1}),
        ("enter-key-and-slot-change", {"enter_action": 13, "enter_release": 14, "speed_slot": 12}),
        ("negative-paste-delay-and-no-reequip", {"paste_delay": -99, "use_old_paste": True, "speed_slot": -5, "unequip_toggle": True}),
    ]
    cases = []
    for name, changes in variants:
        spec = dict(base); spec.update(changes)
        observed = run_json(RUNTIME_ORACLE, "unequip-worker", compact(spec))
        cases.append({"name": name, "spec": spec, "events": observed["events"]})
    return cases


def runtime_laugh_cases() -> list[dict[str, Any]]:
    cases = []; case_index = 0
    for global_zoom_action in (False, True):
        for zoom_reverse in (False, True):
            for laugh_move_start in (False, True):
                for global_zoom_release in (False, True):
                    for laugh_move_release in (False, True):
                        spec = {
                            "paste_delay": -5 if case_index == 0 else 5,
                            "use_old_paste": case_index == 0,
                            "enter_action": 13, "enter_release": 14,
                            "global_zoom_action": global_zoom_action, "shift_action": 160,
                            "zoom_reverse": zoom_reverse, "laugh_move_start": laugh_move_start,
                            "global_zoom_release": global_zoom_release, "shift_release": 161,
                            "laugh_move_release": laugh_move_release,
                        }
                        observed = run_json(RUNTIME_ORACLE, "laugh-worker", compact(spec))
                        cases.append({"name": f"matrix-{case_index}", "spec": spec, "events": observed["events"]})
                        case_index += 1
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
        "freeze_threshold_cases": runtime_freeze_threshold_cases(),
        "freeze_cases": runtime_freeze_cases(),
        "target_suspend_cases": runtime_target_suspend_cases(),
        "item_desync_cases": runtime_item_desync_cases(),
        "itemclip_cases": runtime_itemclip_cases(),
        "wallwalk_delay_cases": runtime_wallwalk_delay_cases(),
        "wallwalk_cases": runtime_wallwalk_cases(),
        "wallhop_cases": runtime_wallhop_cases(),
        "walless_cases": runtime_walless_cases(),
        "hhj_cases": runtime_hhj_cases(),
        "floorbounce_cases": runtime_floorbounce_cases(),
        "bunnyhop_physical_cases": runtime_bunnyhop_physical_cases(),
        "bunnyhop_guard_cases": runtime_bunnyhop_guard_cases(),
        "bunnyhop_iteration_cases": runtime_bunnyhop_iteration_cases(),
        "anti_afk_timer_cases": runtime_anti_afk_timer_cases(),
        "anti_afk_keypress_cases": runtime_anti_afk_keypress_cases(),
        "anti_afk_tap_cases": runtime_anti_afk_tap_cases(),
        "anti_afk_background_cases": runtime_anti_afk_background_cases(),
        "spam_controller_cases": runtime_spam_controller_cases(),
        "spam_guard_cases": runtime_spam_guard_cases(),
        "spam_iteration_cases": runtime_spam_iteration_cases(),
        "hhj_motion_cases": runtime_hhj_motion_cases(),
        "ledge_turn_cases": runtime_ledge_turn_cases(),
        "ledge_worker_cases": runtime_ledge_worker_cases(),
        "unequip_cases": runtime_unequip_cases(),
        "laugh_cases": runtime_laugh_cases(),
        "presskey_cases": runtime_presskey_cases(),
        "speedglitch_cases": runtime_speedglitch_cases(),
    }
    write_json(ROOT / "compat" / "timing" / "runtime_cases.json", output)


def ensure_oracle(path: Path, target: str, parser: argparse.ArgumentParser) -> None:
    if not path.is_file():
        parser.error(f"C++ oracle not found at {path}; build target {target} first")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("fixture", choices=["hotkeys", "input-hotkeys", "linux-keymap", "move-degrees", "profiles", "roblox", "roblox-regex", "runtime", "script-metadata", "all"])
    args = parser.parse_args()

    if args.fixture in {"hotkeys", "all"}:
        ensure_oracle(ORACLE, "smu_compat_oracle", parser)
        write_hotkeys()
    if args.fixture in {"input-hotkeys", "all"}:
        ensure_oracle(ORACLE, "smu_compat_oracle", parser)
        write_input_hotkeys()
    if args.fixture in {"linux-keymap", "all"}:
        ensure_oracle(ORACLE, "smu_compat_oracle", parser)
        write_linux_keymap()
    if args.fixture in {"roblox-regex", "all"}:
        ensure_oracle(ORACLE, "smu_compat_oracle", parser)
        write_roblox_regex()
    if args.fixture in {"move-degrees", "all"}:
        ensure_oracle(ORACLE, "smu_compat_oracle", parser)
        write_move_degrees()
    if args.fixture in {"script-metadata", "all"}:
        ensure_oracle(ORACLE, "smu_compat_oracle", parser)
        write_script_metadata()
    if args.fixture in {"profiles", "all"}:
        ensure_oracle(PROFILE_ORACLE, "smu_profile_compat_oracle", parser)
        write_profiles()
    if args.fixture in {"roblox", "all"}:
        ensure_oracle(ORACLE, "smu_compat_oracle", parser)
        write_roblox()
    if args.fixture in {"runtime", "all"}:
        ensure_oracle(RUNTIME_ORACLE, "smu_runtime_compat_oracle", parser)
        write_runtime()
    return 0


if __name__ == "__main__":
    sys.exit(main())

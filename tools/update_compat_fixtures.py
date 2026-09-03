#!/usr/bin/env python3
"""Regenerate checked-in rewrite fixtures from the production C++ compatibility oracle."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
ORACLE = ROOT / "build" / "smu_compat_oracle"
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


def oracle(operation: str, value: str) -> dict[str, object]:
    completed = subprocess.run(
        [str(ORACLE), operation, value],
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(completed.stdout)


def write_hotkeys() -> None:
    output = {
        "oracle": "smu_compat_oracle hotkey",
        "cases": [oracle("hotkey", case) for case in HOTKEY_CASES],
    }
    path = ROOT / "compat" / "hotkeys" / "cases.json"
    path.write_text(json.dumps(output, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(path.relative_to(ROOT))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("fixture", choices=["hotkeys"])
    args = parser.parse_args()

    if not ORACLE.is_file():
        parser.error(f"C++ oracle not found at {ORACLE}; build target smu_compat_oracle first")
    if args.fixture == "hotkeys":
        write_hotkeys()
    return 0


if __name__ == "__main__":
    sys.exit(main())

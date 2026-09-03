#!/usr/bin/env python3
"""Capture one deterministic C++/Rust UI parity case and run the gated image diff."""

from __future__ import annotations

import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "compat" / "ui" / "cases.toml"
GENERATED = ROOT / "compat" / "screenshots" / "generated"
CPP_BINARY = ROOT / "build" / "Spencer-Macro-Utilities"
RUST_MANIFEST = ROOT / "rust" / "Cargo.toml"


def run(command: list[str], *, env: dict[str, str] | None = None) -> None:
    print("+", " ".join(command), flush=True)
    subprocess.run(command, cwd=ROOT, env=env, check=True)


def load_case(name: str) -> dict[str, object]:
    with MANIFEST.open("rb") as handle:
        data = tomllib.load(handle)
    for case in data.get("case", []):
        if case.get("name") == name:
            return case
    raise SystemExit(f"UI parity case not found: {name}")


def xvfb_command(width: int, height: int, command: list[str]) -> list[str]:
    screen_width = max(width + 400, 1600)
    screen_height = max(height + 300, 1000)
    return [
        "xvfb-run",
        "-a",
        "-s",
        f"-screen 0 {screen_width}x{screen_height}x24",
        *command,
    ]


def settings_file_from_env(environment: dict[str, str]) -> Path:
    xdg_config_home = environment.get("XDG_CONFIG_HOME")
    if xdg_config_home:
        return Path(xdg_config_home) / "SpencerMacroUtilities" / "SMCSettings.json"
    home = Path(environment.get("HOME", str(Path.home())))
    return home / ".config" / "SpencerMacroUtilities" / "SMCSettings.json"


def file_fingerprint(path: Path) -> tuple[str, int] | None:
    if not path.is_file():
        return None
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    return digest, path.stat().st_mtime_ns


def assert_settings_unchanged(path: Path, before: tuple[str, int] | None) -> None:
    after = file_fingerprint(path)
    if before != after:
        raise RuntimeError(
            f"UI parity capture modified user settings: {path}\n"
            f"before={before!r}\nafter={after!r}"
        )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("case", nargs="?", default="lag-switch-linux-minimum")
    parser.add_argument("--skip-build", action="store_true")
    args = parser.parse_args()

    if sys.platform != "linux":
        parser.error("the current headless capture driver is Linux/Xvfb-only")
    if shutil.which("xvfb-run") is None:
        parser.error("xvfb-run is required (Fedora package: xorg-x11-server-Xvfb)")

    case = load_case(args.case)
    if not bool(case.get("automated", False)):
        parser.error(f"case is a planned matrix entry, not automated yet: {args.case}")
    platform = case.get("platform")
    if platform not in (None, "linux"):
        parser.error(f"case requires platform {platform!r}, current platform is Linux")

    width = int(case["logical_width"])
    height = int(case["logical_height"])
    parity_state = str(case.get("parity_state") or args.case)

    GENERATED.mkdir(parents=True, exist_ok=True)
    cpp_capture = GENERATED / f"{args.case}-cpp.ppm"
    rust_capture = GENERATED / f"{args.case}-rust.png"
    gate_diff = GENERATED / f"{args.case}-gate-diff.png"

    if not args.skip_build:
        run(["cmake", "-S", ".", "-B", "build"])
        run(["cmake", "--build", "build", "--target", "spencer_macro_utilities", "-j2"])
        run(["cargo", "build", "--manifest-path", str(RUST_MANIFEST), "-p", "smu-app"])
        run(["cargo", "build", "--manifest-path", str(RUST_MANIFEST), "-p", "smu-compat"])

    common_env = os.environ.copy()
    common_env.update(
        {
            "LIBGL_ALWAYS_SOFTWARE": "1",
            "SDL_VIDEODRIVER": "x11",
            "SMU_UI_PARITY_CASE": parity_state,
            "SMU_UI_CAPTURE_WIDTH": str(width),
            "SMU_UI_CAPTURE_HEIGHT": str(height),
        }
    )

    settings_path = settings_file_from_env(common_env)
    settings_before = file_fingerprint(settings_path)

    cpp_env = common_env.copy()
    cpp_env["SMU_UI_CAPTURE_PATH"] = str(cpp_capture)
    run(xvfb_command(width, height, [str(CPP_BINARY)]), env=cpp_env)
    assert_settings_unchanged(settings_path, settings_before)

    rust_env = common_env.copy()
    rust_env["SMU_UI_CAPTURE_PATH"] = str(rust_capture)
    run(
        xvfb_command(
            width,
            height,
            [
                "cargo",
                "run",
                "--quiet",
                "--manifest-path",
                str(RUST_MANIFEST),
                "-p",
                "smu-app",
            ],
        ),
        env=rust_env,
    )

    run(
        [
            "cargo",
            "run",
            "--quiet",
            "--manifest-path",
            str(RUST_MANIFEST),
            "-p",
            "smu-compat",
            "--",
            "case-diff",
            str(MANIFEST),
            args.case,
            str(cpp_capture),
            str(rust_capture),
            str(gate_diff),
        ]
    )
    print(f"C++ capture: {cpp_capture.relative_to(ROOT)}")
    print(f"Rust capture: {rust_capture.relative_to(ROOT)}")
    print(f"Gate diff:    {gate_diff.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

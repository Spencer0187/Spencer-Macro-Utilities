#!/usr/bin/env python3
"""Build and stage an experimental Rust Windows SMU runtime directory.

This intentionally does not replace the legacy C++ Windows release/signing path.
"""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys

APP_OUTPUT_NAME = "Spencer-Macro-Utilities.exe"
CARGO_BINARY_NAME = "smu-app.exe"
HELPER_OUTPUT_NAME = "SMU-Update-Helper.exe"
HELPER_CARGO_BINARY_NAME = "smu-windows-updater-helper.exe"
SDL_DLL_NAME = "SDL3.dll"
WINDIVERT_DLL_NAME = "SMCWinDivert.dll"
WINDIVERT_DRIVER_NAME = "SMCWinDivert64.sys"
WINDIVERT_DLL_DRIVER_NAME = "WinDivert64.sys"
WINDOWS_GUI_SUBSYSTEM = 2


def run(command: list[str], *, cwd: Path, env: dict[str, str]) -> None:
    print("+", " ".join(command), flush=True)
    subprocess.run(command, cwd=cwd, env=env, check=True)


def pe_subsystem(executable: Path) -> int:
    with executable.open("rb") as stream:
        dos_header = stream.read(64)
        if len(dos_header) < 64 or dos_header[:2] != b"MZ":
            raise RuntimeError(f"not a Windows PE executable: {executable}")
        pe_offset = int.from_bytes(dos_header[0x3C:0x40], "little")
        stream.seek(pe_offset)
        signature = stream.read(4)
        if signature != b"PE\0\0":
            raise RuntimeError(f"missing PE signature: {executable}")
        coff_header = stream.read(20)
        if len(coff_header) != 20:
            raise RuntimeError(f"truncated PE COFF header: {executable}")
        optional_size = int.from_bytes(coff_header[16:18], "little")
        optional_header = stream.read(optional_size)
        if len(optional_header) != optional_size or optional_size < 70:
            raise RuntimeError(f"truncated PE optional header: {executable}")
        magic = int.from_bytes(optional_header[:2], "little")
        if magic not in {0x10B, 0x20B}:
            raise RuntimeError(f"unsupported PE optional-header magic 0x{magic:04X}: {executable}")
        return int.from_bytes(optional_header[68:70], "little")


def resolve_sdl_dll(target_dir: Path, profile: str) -> Path:
    copied = target_dir / profile / SDL_DLL_NAME
    if copied.is_file():
        return copied

    candidates = {
        path.resolve()
        for path in target_dir.glob(
            f"{profile}/build/sdl3-sys-*/out/bin/{SDL_DLL_NAME}"
        )
        if path.is_file()
    }
    if len(candidates) != 1:
        rendered = ", ".join(str(path) for path in sorted(candidates)) or "none"
        raise RuntimeError(
            "expected Cargo-built SDL3.dll at the profile root or exactly one "
            f"sdl3-sys build output under {target_dir / profile}, found: {rendered}"
        )
    return next(iter(candidates))


def required_runtime_sources(root: Path) -> tuple[Path, Path, Path]:
    assets = root / "assets"
    windivert = root / "third_party" / "windivert"
    dll = windivert / WINDIVERT_DLL_NAME
    driver = windivert / WINDIVERT_DRIVER_NAME

    required = (
        assets / "LSANS.TTF",
        assets / "macro_tutorials",
        dll,
        driver,
    )
    missing = [path for path in required if not path.exists()]
    if missing:
        rendered = ", ".join(str(path) for path in missing)
        raise RuntimeError(f"required Windows Rust runtime assets are missing: {rendered}")
    if not (assets / "macro_tutorials").is_dir():
        raise RuntimeError(
            f"macro tutorial asset path is not a directory: {assets / 'macro_tutorials'}"
        )
    return assets, dll, driver


def stage_runtime(
    binary: Path,
    helper_binary: Path,
    sdl_dll: Path,
    assets: Path,
    windivert_dll: Path,
    windivert_driver: Path,
    stage_dir: Path,
) -> tuple[Path, Path, Path, Path, Path]:
    for label, path in (
        ("Rust app binary", binary),
        ("Rust update helper binary", helper_binary),
        ("Cargo-built SDL3.dll", sdl_dll),
        (WINDIVERT_DLL_NAME, windivert_dll),
        (WINDIVERT_DRIVER_NAME, windivert_driver),
    ):
        if not path.is_file():
            raise RuntimeError(f"{label} was not produced/found: {path}")
    if not assets.is_dir():
        raise RuntimeError(f"Rust app assets directory is missing: {assets}")

    if stage_dir.exists():
        shutil.rmtree(stage_dir)
    stage_dir.mkdir(parents=True)

    staged_binary = stage_dir / APP_OUTPUT_NAME
    staged_helper = stage_dir / HELPER_OUTPUT_NAME
    staged_sdl = stage_dir / SDL_DLL_NAME
    staged_windivert_dll = stage_dir / WINDIVERT_DLL_NAME
    staged_windivert_driver = stage_dir / WINDIVERT_DRIVER_NAME

    shutil.copy2(binary, staged_binary)
    shutil.copy2(helper_binary, staged_helper)
    shutil.copy2(sdl_dll, staged_sdl)
    shutil.copy2(windivert_dll, staged_windivert_dll)
    shutil.copy2(windivert_driver, staged_windivert_driver)
    # The unmodified SMCWinDivert.dll creates the WinDivert service with this exact image path.
    shutil.copy2(windivert_driver, stage_dir / WINDIVERT_DLL_DRIVER_NAME)
    shutil.copytree(assets, stage_dir / "assets")

    return (
        staged_binary,
        staged_helper,
        staged_sdl,
        staged_windivert_dll,
        staged_windivert_driver,
    )


def build_and_stage(
    root: Path,
    target_dir: Path,
    stage_dir: Path,
    profile: str,
    cargo: str,
) -> tuple[Path, Path, Path, Path, Path]:
    if profile not in {"debug", "release"}:
        raise ValueError(f"unsupported Cargo profile: {profile}")

    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(target_dir)
    command = [
        cargo,
        "build",
        "--locked",
        "--manifest-path",
        str(root / "rust" / "Cargo.toml"),
        "-p",
        "smu-app",
        "-p",
        "smu-windows-updater-helper",
    ]
    if profile == "release":
        command.append("--release")
    run(command, cwd=root / "rust", env=env)

    binary = target_dir / profile / CARGO_BINARY_NAME
    helper_binary = target_dir / profile / HELPER_CARGO_BINARY_NAME
    for label, executable in (("app", binary), ("update helper", helper_binary)):
        subsystem = pe_subsystem(executable)
        if subsystem != WINDOWS_GUI_SUBSYSTEM:
            raise RuntimeError(
                f"Rust Windows {label} uses PE subsystem {subsystem}, expected GUI subsystem {WINDOWS_GUI_SUBSYSTEM}"
            )
    sdl_dll = resolve_sdl_dll(target_dir, profile)
    assets, windivert_dll, windivert_driver = required_runtime_sources(root)
    return stage_runtime(
        binary,
        helper_binary,
        sdl_dll,
        assets,
        windivert_dll,
        windivert_driver,
        stage_dir,
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--target-dir", type=Path, required=True)
    parser.add_argument("--stage-dir", type=Path, required=True)
    parser.add_argument("--profile", choices=("debug", "release"), default="release")
    parser.add_argument("--cargo", default="cargo")
    return parser.parse_args()


def main() -> int:
    if sys.platform != "win32":
        raise SystemExit("Rust Windows staging can only run on Windows")
    args = parse_args()
    staged = build_and_stage(
        args.root.resolve(),
        args.target_dir.resolve(),
        args.stage_dir.resolve(),
        args.profile,
        args.cargo,
    )
    print(f"Staged experimental Rust Windows app: {staged[0]}")
    print(f"Staged Rust update helper: {staged[1]}")
    print(f"Staged Cargo SDL3: {staged[2]}")
    print(f"Staged WinDivert DLL: {staged[3]}")
    print(f"Staged WinDivert driver: {staged[4]}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

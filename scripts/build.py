#!/usr/bin/env python3
"""Build the useful local SMU package for the current platform."""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def run(command: list[str], *, env: dict[str, str] | None = None) -> None:
    print("+", " ".join(command), flush=True)
    subprocess.run(command, cwd=ROOT, env=env, check=True)


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(
        description=(
            "Build SMU locally. The default is an unsigned Windows executable, "
            "a portable Linux folder, or a universal macOS package."
        )
    )
    linux_format = result.add_mutually_exclusive_group()
    linux_format.add_argument(
        "--appimage",
        action="store_true",
        help="build a Linux AppImage (appimagetool is downloaded and cached if needed)",
    )
    linux_format.add_argument(
        "--linux-release",
        action="store_true",
        help="build the all-in-one Linux ZIP (requires nfpm and zip)",
    )
    result.add_argument(
        "--config",
        choices=("Debug", "Release"),
        default="Release",
        help="CMake configuration (default: Release)",
    )
    result.add_argument(
        "--cmake-arg",
        action="append",
        default=[],
        metavar="ARG",
        help="additional CMake configure argument; may be repeated",
    )
    return result


def build_windows(config: str, cmake_args: list[str]) -> None:
    build_dir = ROOT / "build" / "windows-local"
    run(
        [
            "cmake",
            "-S",
            str(ROOT),
            "-B",
            str(build_dir),
            "-G",
            "Visual Studio 17 2022",
            "-A",
            "x64",
            *cmake_args,
        ]
    )
    run(
        [
            "cmake",
            "--build",
            str(build_dir),
            "--config",
            config,
            "--target",
            "suspend",
            "--parallel",
        ]
    )
    print(f"\nBuilt unsigned executable: {build_dir / config / 'suspend.exe'}")


def build_linux(
    config: str,
    cmake_args: list[str],
    *,
    appimage: bool,
    release: bool,
) -> None:
    env = os.environ.copy()
    env["CMAKE_BUILD_TYPE"] = config
    script = "package_linux.sh"
    if appimage:
        script = "package_appimage.sh"
    elif release:
        script = "package_linux_release.sh"
    run(["bash", str(ROOT / "scripts" / script), *cmake_args], env=env)


def build_macos(config: str, cmake_args: list[str]) -> None:
    if config != "Release":
        raise SystemExit("The universal macOS package currently supports Release builds only.")
    run(["bash", str(ROOT / "scripts" / "package_macos.sh"), *cmake_args])


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    if sys.platform == "win32":
        if args.appimage or args.linux_release:
            raise SystemExit("Linux package options can only be used on Linux.")
        build_windows(args.config, args.cmake_arg)
    elif sys.platform == "darwin":
        if args.appimage or args.linux_release:
            raise SystemExit("Linux package options can only be used on Linux.")
        build_macos(args.config, args.cmake_arg)
    elif sys.platform.startswith("linux"):
        build_linux(
            args.config,
            args.cmake_arg,
            appimage=args.appimage,
            release=args.linux_release,
        )
    else:
        raise SystemExit(f"Unsupported build host: {sys.platform}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

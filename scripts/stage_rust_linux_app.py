#!/usr/bin/env python3
"""Build and stage the Rust Linux SMU executable with its exact Cargo-built SDL3."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys

APP_OUTPUT_NAME = "Spencer-Macro-Utilities"
CARGO_BINARY_NAME = "smu-app"
ORIGIN_RUNPATH_FLAG = r"-C link-arg=-Wl,-rpath,$ORIGIN/lib"


def rustflags_with_origin_runpath(existing: str | None) -> str:
    parts = [existing.strip()] if existing and existing.strip() else []
    if ORIGIN_RUNPATH_FLAG not in parts:
        parts.append(ORIGIN_RUNPATH_FLAG)
    return " ".join(parts)


def run(command: list[str], *, cwd: Path, env: dict[str, str]) -> None:
    print("+", " ".join(command), flush=True)
    subprocess.run(command, cwd=cwd, env=env, check=True)


def resolve_sdl_library(target_dir: Path, profile: str) -> Path:
    candidates: set[Path] = set()
    pattern = f"{profile}/build/sdl3-sys-*/out/lib64/libSDL3.so.0"
    for soname_link in target_dir.glob(pattern):
        try:
            resolved = soname_link.resolve(strict=True)
        except OSError:
            continue
        if resolved.is_file():
            candidates.add(resolved)
    if len(candidates) != 1:
        rendered = ", ".join(str(path) for path in sorted(candidates)) or "none"
        raise RuntimeError(
            "expected exactly one Cargo-built SDL3 soname target under "
            f"{target_dir / profile}, found {len(candidates)}: {rendered}"
        )
    return next(iter(candidates))


def stage_runtime(binary: Path, sdl_library: Path, stage_dir: Path) -> tuple[Path, Path]:
    if not binary.is_file():
        raise RuntimeError(f"Rust app binary was not produced: {binary}")
    if not sdl_library.is_file():
        raise RuntimeError(f"Cargo-built SDL3 library was not produced: {sdl_library}")

    stage_dir.mkdir(parents=True, exist_ok=True)
    lib_dir = stage_dir / "lib"
    if lib_dir.exists():
        shutil.rmtree(lib_dir)
    lib_dir.mkdir(parents=True)
    staged_binary = stage_dir / APP_OUTPUT_NAME
    shutil.copy2(binary, staged_binary)
    staged_binary.chmod(staged_binary.stat().st_mode | 0o111)

    staged_real_sdl = lib_dir / sdl_library.name
    shutil.copy2(sdl_library, staged_real_sdl)
    for link_name in ("libSDL3.so.0", "libSDL3.so"):
        link = lib_dir / link_name
        if link.exists() or link.is_symlink():
            link.unlink()
        link.symlink_to(staged_real_sdl.name)
    return staged_binary, staged_real_sdl


def build_and_stage(
    root: Path,
    target_dir: Path,
    stage_dir: Path,
    profile: str,
    cargo: str,
) -> tuple[Path, Path]:
    if profile not in {"debug", "release"}:
        raise ValueError(f"unsupported Cargo profile: {profile}")
    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(target_dir)
    env["RUSTFLAGS"] = rustflags_with_origin_runpath(env.get("RUSTFLAGS"))
    command = [
        cargo,
        "build",
        "--locked",
        "--manifest-path",
        str(root / "rust" / "Cargo.toml"),
        "-p",
        "smu-app",
    ]
    if profile == "release":
        command.append("--release")
    run(command, cwd=root / "rust", env=env)

    binary = target_dir / profile / CARGO_BINARY_NAME
    sdl_library = resolve_sdl_library(target_dir, profile)
    return stage_runtime(binary, sdl_library, stage_dir)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--target-dir", type=Path, required=True)
    parser.add_argument("--stage-dir", type=Path, required=True)
    parser.add_argument("--profile", choices=("debug", "release"), default="release")
    parser.add_argument("--cargo", default="cargo")
    return parser.parse_args()


def main() -> int:
    if not sys.platform.startswith("linux"):
        raise SystemExit("Rust Linux staging can only run on Linux")
    args = parse_args()
    binary, sdl_library = build_and_stage(
        args.root.resolve(),
        args.target_dir.resolve(),
        args.stage_dir.resolve(),
        args.profile,
        args.cargo,
    )
    print(f"Staged Rust app: {binary}")
    print(f"Staged Cargo SDL3: {sdl_library}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

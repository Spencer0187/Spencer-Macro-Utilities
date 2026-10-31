#!/usr/bin/env python3
"""Build and stage a universal Rust macOS SMU app bundle.

Ad-hoc signing (identity "-") is the certificate-free release default.
It satisfies macOS code integrity, but does not establish release-signer trust:
updater automatic replacement must remain disabled for ad-hoc releases.
"""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys

APP_BUNDLE_NAME = "Spencer Macro Utilities.app"
APP_EXECUTABLE_NAME = "Spencer-Macro-Utilities"
APP_CARGO_BINARY_NAME = "smu-app"
HELPER_EXECUTABLE_NAME = "SMU-Update-Helper"
HELPER_CARGO_BINARY_NAME = "smu-macos-updater-helper"
BUNDLE_IDENTIFIER = "com.spencer0187.smu"
SDL_PREFIX = "libSDL3"
FRAMEWORK_RPATH = "@executable_path/../Frameworks"
TARGET_ARCHITECTURES = {
    "aarch64-apple-darwin": "arm64",
    "x86_64-apple-darwin": "x86_64",
}


def run(
    command: list[str],
    *,
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
    capture: bool = False,
) -> subprocess.CompletedProcess[str]:
    print("+", " ".join(command), flush=True)
    return subprocess.run(
        command,
        cwd=cwd,
        env=env,
        check=True,
        text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.STDOUT if capture else None,
    )


def profile_args(profile: str) -> list[str]:
    if profile == "release":
        return ["--release"]
    if profile == "debug":
        return []
    raise ValueError(f"unsupported Cargo profile: {profile}")


def build_target_binaries(
    root: Path,
    target_dir: Path,
    target: str,
    profile: str,
    cargo: str,
) -> tuple[Path, Path, Path]:
    if target not in TARGET_ARCHITECTURES:
        raise ValueError(f"unsupported macOS Rust target: {target}")
    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(target_dir)
    env.setdefault("MACOSX_DEPLOYMENT_TARGET", "10.15")
    command = [
        cargo,
        "build",
        "--locked",
        "--manifest-path",
        str(root / "rust" / "Cargo.toml"),
        "-p",
        "smu-app",
        "-p",
        "smu-macos-updater-helper",
        "--target",
        target,
        *profile_args(profile),
    ]
    run(command, cwd=root / "rust", env=env)
    profile_dir = target_dir / target / profile
    app = profile_dir / APP_CARGO_BINARY_NAME
    helper = profile_dir / HELPER_CARGO_BINARY_NAME
    for label, path in (("Rust macOS app", app), ("Rust macOS update helper", helper)):
        if not path.is_file():
            raise RuntimeError(f"{label} was not produced for {target}: {path}")
    sdl = resolve_sdl_dylib(target_dir / target, profile)
    return app, helper, sdl


def create_universal_binary(inputs: list[Path], output: Path) -> None:
    if len(inputs) != len(TARGET_ARCHITECTURES):
        raise RuntimeError(
            f"expected {len(TARGET_ARCHITECTURES)} architecture inputs for {output}, got {len(inputs)}"
        )
    output.parent.mkdir(parents=True, exist_ok=True)
    run(["/usr/bin/lipo", "-create", *(str(path) for path in inputs), "-output", str(output)])


def binary_architectures(binary: Path) -> set[str]:
    output = run(["/usr/bin/lipo", "-archs", str(binary)], capture=True).stdout or ""
    return set(output.split())


def require_universal_binary(binary: Path) -> None:
    expected = set(TARGET_ARCHITECTURES.values())
    actual = binary_architectures(binary)
    if actual != expected:
        raise RuntimeError(
            f"{binary} architectures are {sorted(actual)}, expected {sorted(expected)}"
        )


def resolve_sdl_dylib(target_dir: Path, profile: str) -> Path:
    candidates: set[Path] = set()
    profile_dir = target_dir / profile
    for path in profile_dir.glob(f"{SDL_PREFIX}*.dylib"):
        if path.is_file() and not path.is_symlink():
            candidates.add(path.resolve())
    if not candidates:
        for path in target_dir.glob(f"{profile}/build/sdl3-sys-*/out/lib/{SDL_PREFIX}*.dylib"):
            if path.is_file() and not path.is_symlink():
                candidates.add(path.resolve())
    preferred = sorted(
        (path for path in candidates if path.name != "libSDL3.dylib"),
        key=lambda path: (len(path.name), path.name),
    )
    if len(preferred) == 1:
        return preferred[0]
    if len(candidates) == 1:
        return next(iter(candidates))
    rendered = ", ".join(str(path) for path in sorted(candidates)) or "none"
    raise RuntimeError(
        "expected exactly one Cargo-built SDL3 dylib candidate under "
        f"{profile_dir}, found: {rendered}"
    )


def linked_libraries(binary: Path) -> list[str]:
    output = run(["/usr/bin/otool", "-L", str(binary)], capture=True).stdout or ""
    libraries: list[str] = []
    for line in output.splitlines():
        if not line[:1].isspace():
            # Thin/universal image headers are not dependency entries.
            continue
        stripped = line.strip()
        if not stripped:
            continue
        library = stripped.split(" (", 1)[0]
        if library not in libraries:
            libraries.append(library)
    return libraries


def rpaths(binary: Path) -> list[str]:
    output = run(["/usr/bin/otool", "-l", str(binary)], capture=True).stdout or ""
    lines = output.splitlines()
    result = []
    for index, line in enumerate(lines):
        if line.strip() != "cmd LC_RPATH":
            continue
        for following in lines[index + 1 : index + 6]:
            stripped = following.strip()
            if stripped.startswith("path "):
                result.append(stripped[5:].split(" (offset", 1)[0])
                break
    return result


def rewrite_sdl_dependency(binary: Path, staged_sdl: Path) -> None:
    sdl_dependencies = [
        library for library in linked_libraries(binary) if Path(library).name.startswith(SDL_PREFIX)
    ]
    if len(sdl_dependencies) != 1:
        rendered = ", ".join(sdl_dependencies) or "none"
        raise RuntimeError(
            f"expected exactly one SDL3 dependency in {binary}, found: {rendered}"
        )
    desired = f"@rpath/{staged_sdl.name}"
    if sdl_dependencies[0] != desired:
        run(
            [
                "/usr/bin/install_name_tool",
                "-change",
                sdl_dependencies[0],
                desired,
                str(binary),
            ]
        )
    if FRAMEWORK_RPATH not in rpaths(binary):
        run(["/usr/bin/install_name_tool", "-add_rpath", FRAMEWORK_RPATH, str(binary)])
    run(["/usr/bin/install_name_tool", "-id", desired, str(staged_sdl)])


def write_info_plist(bundle: Path, version: str) -> Path:
    plist = {
        "CFBundleDevelopmentRegion": "en",
        "CFBundleDisplayName": "Spencer Macro Utilities",
        "CFBundleExecutable": APP_EXECUTABLE_NAME,
        "CFBundleIconFile": "smu_icon.icns",
        "CFBundleIdentifier": BUNDLE_IDENTIFIER,
        "CFBundleInfoDictionaryVersion": "6.0",
        "CFBundleName": "Spencer Macro Utilities",
        "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": version,
        "CFBundleVersion": version,
        "LSMinimumSystemVersion": "10.15",
        "NSHighResolutionCapable": True,
        "NSPrincipalClass": "NSApplication",
    }
    info = bundle / "Contents" / "Info.plist"
    with info.open("wb") as stream:
        plistlib.dump(plist, stream, sort_keys=True)
    return info


def required_notice_files(root: Path) -> tuple[tuple[Path, str], ...]:
    return (
        (root / "LICENSE", "LICENSE"),
        (root / "PRIVACY.md", "PRIVACY.md"),
        (root / "THIRD_PARTY_NOTICES.md", "THIRD_PARTY_NOTICES.md"),
        (root / "third_party/SDL/LICENSE.txt", "licenses/SDL.txt"),
        (root / "third_party/appimage-runtime/LICENSE", "licenses/AppImage-type2-runtime.txt"),
        (
            root / "platform/linux/nethelper/vendor/github.com/coreos/go-iptables/LICENSE",
            "licenses/go-iptables-LICENSE.txt",
        ),
        (
            root / "platform/linux/nethelper/vendor/github.com/coreos/go-iptables/NOTICE",
            "licenses/go-iptables-NOTICE.txt",
        ),
        (root / "third_party/windivert/LICENSE-LGPL-3.0.txt", "licenses/WinDivert-LGPL-3.0.txt"),
    )


def stage_bundle(
    root: Path,
    app_binaries: list[Path],
    helper_binaries: list[Path],
    sdl_dylibs: list[Path],
    bundle: Path,
    version: str,
) -> tuple[Path, Path, Path]:
    if bundle.exists():
        shutil.rmtree(bundle)
    macos = bundle / "Contents" / "MacOS"
    frameworks = bundle / "Contents" / "Frameworks"
    resources = bundle / "Contents" / "Resources"
    macos.mkdir(parents=True)
    frameworks.mkdir(parents=True)
    resources.mkdir(parents=True)

    sdl_names = {path.name for path in sdl_dylibs}
    if len(sdl_names) != 1:
        raise RuntimeError(
            "Cargo-built SDL3 dylib names differ by architecture: " + ", ".join(sorted(sdl_names))
        )
    staged_app = macos / APP_EXECUTABLE_NAME
    staged_helper = macos / HELPER_EXECUTABLE_NAME
    staged_sdl = frameworks / next(iter(sdl_names))
    create_universal_binary(app_binaries, staged_app)
    create_universal_binary(helper_binaries, staged_helper)
    create_universal_binary(sdl_dylibs, staged_sdl)
    staged_app.chmod(staged_app.stat().st_mode | 0o111)
    staged_helper.chmod(staged_helper.stat().st_mode | 0o111)

    assets = root / "assets"
    if not assets.is_dir():
        raise RuntimeError(f"required Rust app assets are missing: {assets}")
    # Bundle resources belong under Contents/Resources for macOS code signing.
    # Keep the executable-relative lookup contract via an internal, relative symlink:
    # copying assets into Contents/MacOS causes codesign to treat PNGs as nested code.
    shutil.copytree(assets, resources / "assets")
    (macos / "assets").symlink_to("../Resources/assets", target_is_directory=True)
    icon = assets / "smu_icon.icns"
    if not icon.is_file():
        raise RuntimeError(f"required macOS app icon is missing: {icon}")
    shutil.copy2(icon, resources / "smu_icon.icns")

    for source, relative in required_notice_files(root):
        if not source.is_file():
            raise RuntimeError(f"required macOS release notice is missing: {source}")
        destination = resources / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)

    write_info_plist(bundle, version)
    rewrite_sdl_dependency(staged_app, staged_sdl)
    for binary in (staged_app, staged_helper, staged_sdl):
        require_universal_binary(binary)
    return staged_app, staged_helper, staged_sdl


def staged_sdl_dylib(bundle: Path) -> Path:
    dylibs = list((bundle / "Contents" / "Frameworks").glob(f"{SDL_PREFIX}*.dylib"))
    if len(dylibs) != 1:
        raise RuntimeError(f"expected one staged SDL3 dylib, found {len(dylibs)}")
    return dylibs[0]


def sign_bundle(bundle: Path, identity: str) -> None:
    if not identity:
        raise ValueError("macOS Rust staging requires a nonempty signing identity (use - for ad-hoc)")
    staged_sdl = staged_sdl_dylib(bundle)
    helper = bundle / "Contents" / "MacOS" / HELPER_EXECUTABLE_NAME
    app = bundle / "Contents" / "MacOS" / APP_EXECUTABLE_NAME
    for code in (staged_sdl, helper, app):
        run(["/usr/bin/codesign", "--force", "--sign", identity, str(code)])
    run(["/usr/bin/codesign", "--force", "--deep", "--sign", identity, str(bundle)])
    run(["/usr/bin/codesign", "--verify", "--deep", "--strict", "--verbose=2", str(bundle)])


def validate_bundle(bundle: Path, version: str) -> None:
    app = bundle / "Contents" / "MacOS" / APP_EXECUTABLE_NAME
    helper = bundle / "Contents" / "MacOS" / HELPER_EXECUTABLE_NAME
    assets = bundle / "Contents" / "MacOS" / "assets"
    frameworks = bundle / "Contents" / "Frameworks"
    required = (
        app,
        helper,
        assets / "LSANS.TTF",
        assets / "macro_tutorials",
        bundle / "Contents" / "Info.plist",
        bundle / "Contents" / "Resources" / "smu_icon.icns",
        bundle / "Contents" / "Resources" / "LICENSE",
        bundle / "Contents" / "Resources" / "PRIVACY.md",
        bundle / "Contents" / "Resources" / "THIRD_PARTY_NOTICES.md",
    )
    if not assets.is_symlink() or os.readlink(assets) != "../Resources/assets":
        raise RuntimeError("macOS executable asset path must link to bundled Resources/assets")
    missing = [path for path in required if not path.exists()]
    if missing:
        raise RuntimeError(
            "staged Rust macOS app is incomplete: " + ", ".join(str(path) for path in missing)
        )
    staged_sdl = staged_sdl_dylib(bundle)
    dependency = f"@rpath/{staged_sdl.name}"
    if dependency not in linked_libraries(app):
        raise RuntimeError(f"Rust macOS app does not reference staged SDL via {dependency}")
    if FRAMEWORK_RPATH not in rpaths(app):
        raise RuntimeError(f"Rust macOS app is missing bundle Frameworks rpath {FRAMEWORK_RPATH}")
    if any(Path(library).name.startswith(SDL_PREFIX) for library in linked_libraries(helper)):
        raise RuntimeError("Rust macOS update helper must remain SDL-free")
    for binary in (app, helper, staged_sdl):
        require_universal_binary(binary)
    with (bundle / "Contents" / "Info.plist").open("rb") as stream:
        plist = plistlib.load(stream)
    expected_plist = {
        "CFBundleExecutable": APP_EXECUTABLE_NAME,
        "CFBundleIdentifier": BUNDLE_IDENTIFIER,
        "CFBundleIconFile": "smu_icon.icns",
        "CFBundleShortVersionString": version,
        "CFBundleVersion": version,
        "LSMinimumSystemVersion": "10.15",
    }
    mismatched = {
        key: plist.get(key)
        for key, expected in expected_plist.items()
        if plist.get(key) != expected
    }
    if mismatched:
        raise RuntimeError(f"Rust macOS Info.plist metadata is incorrect: {mismatched}")


def build_and_stage(
    root: Path,
    target_dir: Path,
    bundle: Path,
    profile: str,
    cargo: str,
    sign_identity: str,
) -> tuple[Path, Path, Path]:
    version = (root / "version").read_text(encoding="utf-8").strip()
    if not version:
        raise RuntimeError("repository version file is empty")
    artifacts = [
        build_target_binaries(root, target_dir, target, profile, cargo)
        for target in TARGET_ARCHITECTURES
    ]
    staged = stage_bundle(
        root,
        [artifact[0] for artifact in artifacts],
        [artifact[1] for artifact in artifacts],
        [artifact[2] for artifact in artifacts],
        bundle,
        version,
    )
    sign_bundle(bundle, sign_identity)
    validate_bundle(bundle, version)
    return staged


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--target-dir", type=Path, required=True)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--profile", choices=("debug", "release"), default="release")
    parser.add_argument("--cargo", default="cargo")
    parser.add_argument("--sign-identity", default="-", help="codesign identity; - uses certificate-free ad-hoc signing")
    return parser.parse_args()


def main() -> int:
    if sys.platform != "darwin":
        raise SystemExit("Rust macOS staging can only run on macOS")
    args = parse_args()
    staged = build_and_stage(
        args.root.resolve(),
        args.target_dir.resolve(),
        args.bundle.resolve(),
        args.profile,
        args.cargo,
        args.sign_identity,
    )
    print(f"Staged Rust macOS app: {staged[0]}")
    print(f"Staged Rust macOS update helper: {staged[1]}")
    print(f"Staged Cargo SDL3 dylib: {staged[2]}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

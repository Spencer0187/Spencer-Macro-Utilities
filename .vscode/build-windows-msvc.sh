#!/usr/bin/env bash
set -euo pipefail

CONFIG="Debug"
RUN_AFTER_BUILD=0
FORCE_SYNC="${SMU_FORCE_SYNC:-0}"

for arg in "$@"; do
  case "$arg" in
    Debug|Release|RelWithDebInfo)
      CONFIG="$arg"
      ;;
    --run)
      RUN_AFTER_BUILD=1
      ;;
    --force-sync)
      FORCE_SYNC=1
      ;;
    *)
      echo "Unknown argument: $arg" >&2
      echo "Usage: $0 [Debug|Release|RelWithDebInfo] [--run] [--force-sync]" >&2
      exit 2
      ;;
  esac
done

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PS_EXE="/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe"

if ! command -v rsync >/dev/null 2>&1; then
  echo "rsync is not installed. Run: sudo apt install -y rsync" >&2
  exit 127
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "python3 is not installed. It is required for fast dirty-file sync." >&2
  exit 127
fi

if ! git -C "$REPO_ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "This build script expects to run inside a Git work tree: $REPO_ROOT" >&2
  exit 2
fi

WIN_USERPROFILE_WIN="$("$PS_EXE" -NoProfile -ExecutionPolicy Bypass -Command '[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new(); Write-Output $env:USERPROFILE' | tr -d '\r')"
WIN_USERPROFILE_WSL="$(wslpath -u "$WIN_USERPROFILE_WIN")"

CACHE_ROOT="$WIN_USERPROFILE_WSL/AppData/Local/PublicSuspend/MSVC"
SRC_COPY="$CACHE_ROOT/src"
BUILD_DIR="$CACHE_ROOT/build-$CONFIG"
PS1_FILE="$CACHE_ROOT/build-publicsuspend.ps1"
LAUNCH_PS1_FILE="$CACHE_ROOT/run-publicsuspend.ps1"
STATE_DIR="$CACHE_ROOT/.sync-state"
HEAD_STAMP="$STATE_DIR/last-head"
SYNC_VERSION_STAMP="$STATE_DIR/sync-version"
DIRTY_PATHS_STAMP="$STATE_DIR/last-dirty-paths"
SYNC_VERSION="git-dirty-fast-sync-v2"

mkdir -p "$SRC_COPY" "$BUILD_DIR" "$STATE_DIR"

CURRENT_HEAD="$(git -C "$REPO_ROOT" rev-parse HEAD 2>/dev/null || true)"
PREVIOUS_HEAD="$(cat "$HEAD_STAMP" 2>/dev/null || true)"
PREVIOUS_SYNC_VERSION="$(cat "$SYNC_VERSION_STAMP" 2>/dev/null || true)"

full_sync() {
  echo "Full-syncing WSL repo to Windows build copy..."
  echo "From: $REPO_ROOT"
  echo "To:   $SRC_COPY"

  rsync -a --delete \
    --exclude='.git/' \
    --exclude='.vs/' \
    --exclude='.vscode/' \
    --exclude='build*/' \
    --exclude='out/' \
    --exclude='artifacts/' \
    "$REPO_ROOT/" "$SRC_COPY/"

  printf '%s\n' "$CURRENT_HEAD" > "$HEAD_STAMP"
  printf '%s\n' "$SYNC_VERSION" > "$SYNC_VERSION_STAMP"
}

fast_dirty_sync() {
  echo "Fast-syncing Git dirty paths to Windows build copy..."
  echo "From: $REPO_ROOT"
  echo "To:   $SRC_COPY"

  REPO_ROOT="$REPO_ROOT" SRC_COPY="$SRC_COPY" DIRTY_PATHS_STAMP="$DIRTY_PATHS_STAMP" python3 <<'PY'
from __future__ import annotations

import os
import shutil
import subprocess
from pathlib import Path

repo = Path(os.environ["REPO_ROOT"]).resolve()
dst_root = Path(os.environ["SRC_COPY"]).resolve()
dirty_stamp = Path(os.environ["DIRTY_PATHS_STAMP"]).resolve()

excluded_top = {".git", ".vs", ".vscode", "out", "artifacts"}

def is_excluded(rel: str) -> bool:
    parts = Path(rel).parts
    if not parts:
        return True
    if parts[0] in excluded_top:
        return True
    return any(part.startswith("build") for part in parts)

def run_git(args: list[str]) -> bytes:
    return subprocess.check_output(["git", "-C", str(repo), *args])

def nul_paths(args: list[str]) -> list[str]:
    data = run_git(args)
    return [p.decode("utf-8", "surrogateescape") for p in data.split(b"\0") if p]

def read_previous_dirty_paths() -> set[str]:
    try:
        data = dirty_stamp.read_bytes()
    except FileNotFoundError:
        return set()
    return {p.decode("utf-8", "surrogateescape") for p in data.split(b"\0") if p}

def write_current_dirty_paths(paths: set[str]) -> None:
    dirty_stamp.parent.mkdir(parents=True, exist_ok=True)
    payload = b"\0".join(p.encode("utf-8", "surrogateescape") for p in sorted(paths))
    if payload:
        payload += b"\0"
    dirty_stamp.write_bytes(payload)

def remove_dest(rel: str) -> None:
    if is_excluded(rel):
        return
    dst = dst_root / rel
    if dst.is_dir() and not dst.is_symlink():
        shutil.rmtree(dst)
        print(f"deleted dir:  {rel}")
    elif dst.exists() or dst.is_symlink():
        dst.unlink()
        print(f"deleted file: {rel}")

def copy_path(rel: str) -> None:
    if is_excluded(rel):
        return
    src = repo / rel
    dst = dst_root / rel
    if not src.exists() and not src.is_symlink():
        remove_dest(rel)
        return
    dst.parent.mkdir(parents=True, exist_ok=True)
    if src.is_dir() and not src.is_symlink():
        if dst.exists() and not dst.is_dir():
            dst.unlink()
        shutil.copytree(src, dst, dirs_exist_ok=True, ignore=shutil.ignore_patterns(".git", ".vs", ".vscode", "out", "artifacts", "build*"))
        print(f"copied dir:   {rel}")
    else:
        if dst.exists() and dst.is_dir():
            shutil.rmtree(dst)
        shutil.copy2(src, dst, follow_symlinks=True)
        print(f"copied file:  {rel}")

changed = nul_paths(["diff", "--name-only", "-z", "--diff-filter=ACMRTUXB", "HEAD", "--"])
deleted = nul_paths(["diff", "--name-only", "-z", "--diff-filter=D", "HEAD", "--"])
untracked = nul_paths(["ls-files", "--others", "--exclude-standard", "-z"])

previous_dirty = read_previous_dirty_paths()
current_dirty = set(changed) | set(deleted) | set(untracked)

status = run_git(["status", "--porcelain=v1", "-z", "--untracked-files=all"])
records = [r.decode("utf-8", "surrogateescape") for r in status.split(b"\0") if r]
i = 0
while i < len(records):
    rec = records[i]
    if len(rec) >= 4:
        xy = rec[:2]
        if "R" in xy or "C" in xy:
            if i + 1 < len(records):
                old_path = records[i + 1]
                remove_dest(old_path)
                previous_dirty.add(old_path)
                i += 1
    i += 1

# Include previous dirty paths so reverting a file back to HEAD also updates the Windows cache.
paths_to_apply = previous_dirty | current_dirty

for rel in sorted(paths_to_apply):
    if rel in deleted or not (repo / rel).exists():
        remove_dest(rel)
    else:
        copy_path(rel)

write_current_dirty_paths(current_dirty)

print(
    f"fast sync summary: {len(set(changed))} changed, "
    f"{len(set(deleted))} deleted, {len(set(untracked))} untracked, "
    f"{len(previous_dirty)} previously dirty"
)
PY
}

if [[ "$FORCE_SYNC" == "1" ]]; then
  full_sync
elif [[ ! -f "$SRC_COPY/CMakeLists.txt" ]]; then
  full_sync
elif [[ "$PREVIOUS_SYNC_VERSION" != "$SYNC_VERSION" ]]; then
  full_sync
elif [[ -z "$CURRENT_HEAD" || "$CURRENT_HEAD" != "$PREVIOUS_HEAD" ]]; then
  full_sync
else
  fast_dirty_sync
fi

cat > "$PS1_FILE" <<'POWERSHELL'
param(
    [Parameter(Mandatory=$true)][string]$Source,
    [Parameter(Mandatory=$true)][string]$Build,
    [Parameter(Mandatory=$true)][ValidateSet("Debug","Release","RelWithDebInfo")][string]$Config
)

$ErrorActionPreference = "Stop"

Set-Location $env:TEMP

$cmakeLists = Join-Path $Source "CMakeLists.txt"
if (-not (Test-Path $cmakeLists)) {
    throw "Missing CMakeLists.txt at $cmakeLists"
}

Write-Host "Configuring Windows MSVC build..."
Write-Host "Source: $Source"
Write-Host "Build:  $Build"

& cmake.exe -S $Source -B $Build -G "Visual Studio 17 2022" -A x64 -DCMAKE_SUPPRESS_REGENERATION=ON
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

Write-Host "Building $Config..."
& cmake.exe --build $Build --config $Config
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

$exe = Join-Path $Build "$Config\suspend.exe"
if (Test-Path $exe) {
    Write-Host "Built: $exe"
}

exit 0
POWERSHELL

SRC_WIN="$(wslpath -w "$SRC_COPY")"
BUILD_WIN="$(wslpath -w "$BUILD_DIR")"
PS1_WIN="$(wslpath -w "$PS1_FILE")"

"$PS_EXE" \
  -NoProfile \
  -ExecutionPolicy Bypass \
  -File "$PS1_WIN" \
  -Source "$SRC_WIN" \
  -Build "$BUILD_WIN" \
  -Config "$CONFIG"

OUT_DIR="$REPO_ROOT/artifacts/windows/$CONFIG"
EXE_WSL="$BUILD_DIR/$CONFIG/suspend.exe"
PDB_WSL="$BUILD_DIR/$CONFIG/suspend.pdb"

if [[ -f "$EXE_WSL" ]]; then
  mkdir -p "$OUT_DIR"
  cp -f "$EXE_WSL" "$OUT_DIR/"
  echo "Copied executable to: $OUT_DIR/suspend.exe"
fi

if [[ -f "$PDB_WSL" ]]; then
  mkdir -p "$OUT_DIR"
  cp -f "$PDB_WSL" "$OUT_DIR/"
  echo "Copied debug symbols to: $OUT_DIR/suspend.pdb"
fi

if [[ "$RUN_AFTER_BUILD" == "1" ]]; then
  if [[ ! -f "$EXE_WSL" ]]; then
    echo "Cannot run: executable was not found at $EXE_WSL" >&2
    exit 1
  fi

  EXE_WIN="$(wslpath -w "$EXE_WSL")"
  WORKDIR_WIN="$(wslpath -w "$(dirname "$EXE_WSL")")"
  LAUNCH_PS1_WIN="$(wslpath -w "$LAUNCH_PS1_FILE")"

  cat > "$LAUNCH_PS1_FILE" <<'POWERSHELL'
param(
    [Parameter(Mandatory=$true)][string]$Exe,
    [Parameter(Mandatory=$true)][string]$WorkingDirectory
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path -LiteralPath $Exe)) {
    throw "Executable not found: $Exe"
}

if (-not (Test-Path -LiteralPath $WorkingDirectory)) {
    throw "Working directory not found: $WorkingDirectory"
}

Start-Process -FilePath $Exe -WorkingDirectory $WorkingDirectory
POWERSHELL

  echo "Launching: $EXE_WIN"
  "$PS_EXE" \
    -NoProfile \
    -ExecutionPolicy Bypass \
    -File "$LAUNCH_PS1_WIN" \
    -Exe "$EXE_WIN" \
    -WorkingDirectory "$WORKDIR_WIN"
fi

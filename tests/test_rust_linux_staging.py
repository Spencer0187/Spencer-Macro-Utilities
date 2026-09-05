import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "stage_rust_linux_app.py"
SPEC = importlib.util.spec_from_file_location("stage_rust_linux_app", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class RustLinuxStagingTest(unittest.TestCase):
    def test_rustflags_add_origin_runpath_without_dropping_existing_flags(self) -> None:
        self.assertEqual(
            MODULE.rustflags_with_origin_runpath("-C target-cpu=x86-64-v2"),
            "-C target-cpu=x86-64-v2 -C link-arg=-Wl,-rpath,$ORIGIN/lib",
        )
        self.assertEqual(
            MODULE.rustflags_with_origin_runpath(None),
            "-C link-arg=-Wl,-rpath,$ORIGIN/lib",
        )

    def test_resolve_sdl_library_requires_one_exact_cargo_build(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            out = target / "release" / "build" / "sdl3-sys-abc" / "out" / "lib64"
            out.mkdir(parents=True)
            real = out / "libSDL3.so.0.4.14"
            real.write_bytes(b"sdl")
            (out / "libSDL3.so.0").symlink_to(real.name)
            self.assertEqual(MODULE.resolve_sdl_library(target, "release"), real)

    def test_stage_runtime_renames_binary_and_creates_sdl_soname_links(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "smu-app"
            binary.write_bytes(b"rust-app")
            sdl = root / "libSDL3.so.0.4.14"
            sdl.write_bytes(b"sdl")
            stage = root / "stage"
            stale_lib = stage / "lib"
            stale_lib.mkdir(parents=True)
            (stale_lib / "libSDL3.so.0.old").write_bytes(b"old")

            staged_binary, staged_sdl = MODULE.stage_runtime(binary, sdl, stage)

            self.assertEqual(staged_binary.name, "Spencer-Macro-Utilities")
            self.assertEqual(staged_binary.read_bytes(), b"rust-app")
            self.assertTrue(staged_binary.stat().st_mode & 0o111)
            self.assertEqual(staged_sdl.read_bytes(), b"sdl")
            self.assertEqual((stage / "lib" / "libSDL3.so.0").resolve(), staged_sdl)
            self.assertEqual((stage / "lib" / "libSDL3.so").resolve(), staged_sdl)
            self.assertFalse((stage / "lib" / "libSDL3.so.0.old").exists())

    def test_ambiguous_sdl_builds_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            for suffix in ("one", "two"):
                out = target / "release" / "build" / f"sdl3-sys-{suffix}" / "out" / "lib64"
                out.mkdir(parents=True)
                real = out / f"libSDL3.so.0.4.{suffix}"
                real.write_bytes(suffix.encode())
                (out / "libSDL3.so.0").symlink_to(real.name)
            with self.assertRaises(RuntimeError):
                MODULE.resolve_sdl_library(target, "release")


if __name__ == "__main__":
    unittest.main()

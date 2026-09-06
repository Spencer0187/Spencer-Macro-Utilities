import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "stage_rust_windows_app.py"
SPEC = importlib.util.spec_from_file_location("stage_rust_windows_app", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class RustWindowsStagingTest(unittest.TestCase):
    def test_pe_subsystem_reads_gui_header(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            executable = Path(directory) / "app.exe"
            payload = bytearray(0x200)
            payload[:2] = b"MZ"
            payload[0x3C:0x40] = (0x80).to_bytes(4, "little")
            payload[0x80:0x84] = b"PE\0\0"
            payload[0x94:0x96] = (0xF0).to_bytes(2, "little")
            optional = 0x98
            payload[optional : optional + 2] = (0x20B).to_bytes(2, "little")
            payload[optional + 68 : optional + 70] = MODULE.WINDOWS_GUI_SUBSYSTEM.to_bytes(
                2, "little"
            )
            executable.write_bytes(payload)

            self.assertEqual(MODULE.pe_subsystem(executable), MODULE.WINDOWS_GUI_SUBSYSTEM)

    def test_repository_windivert_payloads_match_expected_port_inputs(self) -> None:
        expected = {
            "SMCWinDivert.dll": "c1e060ee19444a259b2162f8af0f3fe8c4428a1c6f694dce20de194ac8d7d9a2",
            "SMCWinDivert64.sys": "8da085332782708d8767bcace5327a6ec7283c17cfb85e40b03cd2323a90ddc2",
        }
        for name, digest in expected.items():
            payload = ROOT / "third_party" / "windivert" / name
            self.assertEqual(hashlib.sha256(payload.read_bytes()).hexdigest(), digest)

    def test_resolve_sdl_dll_prefers_profile_root_copy(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            copied = target / "release" / "SDL3.dll"
            copied.parent.mkdir(parents=True)
            copied.write_bytes(b"profile-copy")
            fallback = (
                target
                / "release"
                / "build"
                / "sdl3-sys-abc"
                / "out"
                / "bin"
                / "SDL3.dll"
            )
            fallback.parent.mkdir(parents=True)
            fallback.write_bytes(b"build-copy")

            self.assertEqual(MODULE.resolve_sdl_dll(target, "release"), copied)

    def test_resolve_sdl_dll_accepts_one_build_output_fallback(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            fallback = (
                target
                / "debug"
                / "build"
                / "sdl3-sys-abc"
                / "out"
                / "bin"
                / "SDL3.dll"
            )
            fallback.parent.mkdir(parents=True)
            fallback.write_bytes(b"build-copy")

            self.assertEqual(MODULE.resolve_sdl_dll(target, "debug"), fallback.resolve())

    def test_resolve_sdl_dll_rejects_ambiguous_build_outputs(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            for suffix in ("one", "two"):
                dll = (
                    target
                    / "release"
                    / "build"
                    / f"sdl3-sys-{suffix}"
                    / "out"
                    / "bin"
                    / "SDL3.dll"
                )
                dll.parent.mkdir(parents=True)
                dll.write_bytes(suffix.encode())

            with self.assertRaises(RuntimeError):
                MODULE.resolve_sdl_dll(target, "release")

    def test_required_runtime_sources_find_assets_and_windivert(self) -> None:
        assets, dll, driver = MODULE.required_runtime_sources(ROOT)
        self.assertEqual(assets, ROOT / "assets")
        self.assertEqual(dll.name, "SMCWinDivert.dll")
        self.assertEqual(driver.name, "SMCWinDivert64.sys")
        self.assertTrue((assets / "LSANS.TTF").is_file())
        self.assertTrue((assets / "macro_tutorials").is_dir())

    def test_stage_runtime_creates_clean_runnable_directory_contract(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "smu-app.exe"
            sdl = root / "SDL3.dll"
            windivert_dll = root / "SMCWinDivert.dll"
            windivert_driver = root / "SMCWinDivert64.sys"
            assets = root / "source-assets"
            tutorials = assets / "macro_tutorials"
            tutorials.mkdir(parents=True)

            binary.write_bytes(b"rust-app")
            sdl.write_bytes(b"sdl")
            windivert_dll.write_bytes(b"windivert-dll")
            windivert_driver.write_bytes(b"windivert-driver")
            (assets / "LSANS.TTF").write_bytes(b"font")
            (tutorials / "wallhop.jpg").write_bytes(b"tutorial")
            (assets / "smu_icon.bmp").write_bytes(b"icon")

            stage = root / "stage"
            stage.mkdir()
            (stage / "stale.txt").write_text("stale", encoding="utf-8")

            staged = MODULE.stage_runtime(
                binary,
                sdl,
                assets,
                windivert_dll,
                windivert_driver,
                stage,
            )

            self.assertEqual(staged[0], stage / "Spencer-Macro-Utilities.exe")
            self.assertEqual((stage / "Spencer-Macro-Utilities.exe").read_bytes(), b"rust-app")
            self.assertEqual((stage / "SDL3.dll").read_bytes(), b"sdl")
            self.assertEqual(
                (stage / "SMCWinDivert.dll").read_bytes(), b"windivert-dll"
            )
            self.assertEqual(
                (stage / "SMCWinDivert64.sys").read_bytes(), b"windivert-driver"
            )
            self.assertEqual((stage / "assets" / "LSANS.TTF").read_bytes(), b"font")
            self.assertEqual(
                (stage / "assets" / "macro_tutorials" / "wallhop.jpg").read_bytes(),
                b"tutorial",
            )
            self.assertEqual((stage / "assets" / "smu_icon.bmp").read_bytes(), b"icon")
            self.assertFalse((stage / "stale.txt").exists())

    def test_stage_runtime_rejects_missing_required_file(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            assets = root / "assets"
            assets.mkdir()
            existing = root / "present"
            existing.write_bytes(b"x")
            with self.assertRaises(RuntimeError):
                MODULE.stage_runtime(
                    existing,
                    root / "missing-sdl.dll",
                    assets,
                    existing,
                    existing,
                    root / "stage",
                )

    def test_rust_windows_binary_uses_gui_subsystem_and_product_stage_name(self) -> None:
        main = (ROOT / "rust" / "crates" / "smu-app" / "src" / "main.rs").read_text(
            encoding="utf-8"
        )
        self.assertIn(
            '#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]',
            main,
        )
        self.assertEqual(MODULE.APP_OUTPUT_NAME, "Spencer-Macro-Utilities.exe")
        self.assertEqual(MODULE.CARGO_BINARY_NAME, "smu-app.exe")

    def test_rewrite_workflow_has_isolated_unsigned_windows_rust_validation(self) -> None:
        workflow = (ROOT / ".github" / "workflows" / "rust-rewrite.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn("windows-rust-app:", workflow)
        self.assertIn("runs-on: windows-latest", workflow)
        self.assertIn("scripts/stage_rust_windows_app.py", workflow)
        self.assertIn("rust-windows-app-unsigned", workflow)
        self.assertNotIn("SignPath", workflow)


if __name__ == "__main__":
    unittest.main()

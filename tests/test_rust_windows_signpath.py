"""SignPath candidate artifact contract tests runnable without Windows."""

import importlib.util
from pathlib import Path
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts/prepare_rust_windows_signpath.py"
SPEC = importlib.util.spec_from_file_location("prepare_rust_windows_signpath", SCRIPT)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class WindowsSignPathInputTest(unittest.TestCase):
    def test_exact_signer_set_matches_updater_and_excludes_kernel_driver(self) -> None:
        MODULE.verify_signpath_config()
        source = (ROOT / "rust/crates/smu-updater/src/windows_bundle.rs").read_text()
        for name in MODULE.RELEASE_SIGNER_FILES:
            self.assertIn(f'"{name}"', source)
        self.assertNotIn("SMCWinDivert64.sys", MODULE.RELEASE_SIGNER_FILES)

    def test_prepares_four_pe_files_only(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            stage = Path(directory) / "stage"
            stage.mkdir()
            for name in MODULE.RELEASE_SIGNER_FILES:
                (stage / name).write_bytes(b"MZ" + bytes(100))
            (stage / "SMCWinDivert64.sys").write_bytes(b"MZ" + bytes(100))
            output = Path(directory) / "request.zip"
            MODULE.prepare(stage, output)
            with zipfile.ZipFile(output) as archive:
                self.assertEqual(tuple(archive.namelist()), MODULE.RELEASE_SIGNER_FILES)
                self.assertEqual(archive.read("SDL3.dll"), b"MZ" + bytes(100))

    def test_rejects_missing_invalid_and_symlinked_inputs(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            stage = Path(directory) / "stage"
            stage.mkdir()
            for name in MODULE.RELEASE_SIGNER_FILES:
                (stage / name).write_bytes(b"MZ" + bytes(100))
            output = Path(directory) / "request.zip"
            victim = stage / MODULE.RELEASE_SIGNER_FILES[0]
            victim.unlink()
            with self.assertRaisesRegex(ValueError, "missing or unsafe"):
                MODULE.prepare(stage, output)
            victim.write_bytes(b"not a PE" + bytes(100))
            with self.assertRaisesRegex(ValueError, "not a Windows PE"):
                MODULE.prepare(stage, output)
            victim.unlink()
            victim.symlink_to(stage / MODULE.RELEASE_SIGNER_FILES[1])
            with self.assertRaisesRegex(ValueError, "missing or unsafe"):
                MODULE.prepare(stage, output)


if __name__ == "__main__":
    unittest.main()

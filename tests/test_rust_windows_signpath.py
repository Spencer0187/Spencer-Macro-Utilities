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
    def test_only_project_owned_pe_files_are_requested_for_signing(self) -> None:
        MODULE.verify_signpath_config()
        source = (ROOT / "rust/crates/smu-updater/src/windows_bundle.rs").read_text()
        self.assertEqual(MODULE.RELEASE_SIGNER_FILES,
                         ("Spencer-Macro-Utilities.exe", "SMU-Update-Helper.exe"))
        self.assertTrue(set(MODULE.RELEASE_SIGNER_FILES).isdisjoint(
            MODULE.THIRD_PARTY_WINDOWS_FILES))
        # The release signer covers only owned code; unsigned upstream DLLs
        # must match the hashes of their already-installed counterparts.
        self.assertIn('WINDOWS_UNCHANGED_UPSTREAM_LIBRARY_FILES', source)
        self.assertIn('require_unchanged_windows_upstream_library', source)
        for name in (*MODULE.RELEASE_SIGNER_FILES, "SDL3.dll", "SMCWinDivert.dll"):
            self.assertIn(f'"{name}"', source)

    def test_prepares_two_owned_pe_files_only(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            stage = Path(directory) / "stage"
            stage.mkdir()
            for name in MODULE.RELEASE_SIGNER_FILES:
                (stage / name).write_bytes(b"MZ" + bytes(100))
            for third_party in MODULE.THIRD_PARTY_WINDOWS_FILES:
                (stage / third_party).write_bytes(b"MZ" + bytes(100))
            output = Path(directory) / "request.zip"
            MODULE.prepare(stage, output)
            with zipfile.ZipFile(output) as archive:
                self.assertEqual(tuple(archive.namelist()), MODULE.RELEASE_SIGNER_FILES)
                self.assertEqual(archive.read("Spencer-Macro-Utilities.exe"), b"MZ" + bytes(100))
                self.assertTrue(set(archive.namelist()).isdisjoint(
                    MODULE.THIRD_PARTY_WINDOWS_FILES))

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

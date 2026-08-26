from __future__ import annotations

import hashlib
import importlib.util
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "smu_update_manifest", ROOT / "scripts" / "generate_update_manifest.py"
)
assert SPEC is not None and SPEC.loader is not None
MANIFEST = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MANIFEST)


class UpdateManifestTests(unittest.TestCase):
    def test_v340_legacy_asset_casing_and_v341_cleanup(self) -> None:
        self.assertEqual(
            MANIFEST.official_update_assets("3.4.0"),
            {
                "windows-x64": "Spencer-Macro-Utilities-Windows.zip",
                "linux-x86_64": "Spencer-Macro-Utilities-V3.4.0-Linux-x86_64.ZIP",
                "macos-universal": "Spencer-Macro-Utilities-V3.4.0-macOS-universal.ZIP",
            },
        )
        v340_assets = MANIFEST.official_update_assets("3.4.0")
        self.assertEqual(
            [name for name in v340_assets.values() if name.endswith(".zip")],
            ["Spencer-Macro-Utilities-Windows.zip"],
        )
        self.assertEqual(
            MANIFEST.official_update_assets("3.4.1"),
            {
                "windows-x64": "Spencer-Macro-Utilities-Windows.zip",
                "linux-x86_64": "Spencer-Macro-Utilities-V3.4.1-Linux-x86_64.zip",
                "macos-universal": "Spencer-Macro-Utilities-V3.4.1-macOS-universal.zip",
            },
        )

    def test_build_manifest_uses_only_official_updater_assets(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            release_dir = Path(temp_dir)
            version = "3.4.0"
            expected_names = MANIFEST.official_update_assets(version)
            payloads: dict[str, bytes] = {}
            for index, filename in enumerate(expected_names.values(), start=1):
                payload = f"official-{index}".encode()
                payloads[filename] = payload
                (release_dir / filename).write_bytes(payload)

            # Contributor-added assets must never enter the updater contract.
            (release_dir / "Spencer-Macro-Utilities-V3.4.0-Windows-debug.zip").write_bytes(b"debug")
            (release_dir / "someone-added-this-later.rpm").write_bytes(b"rpm")

            manifest = MANIFEST.build_manifest(release_dir, version)
            self.assertEqual(manifest["schema_version"], 1)
            self.assertEqual(manifest["release_version"], version)
            self.assertEqual(manifest["minimum_updater_version"], "3.4.0")
            self.assertEqual(set(manifest["artifacts"]), set(expected_names))

            for key, filename in expected_names.items():
                contract = manifest["artifacts"][key]
                payload = payloads[filename]
                self.assertEqual(contract["asset"], filename)
                self.assertEqual(contract["size"], len(payload))
                self.assertEqual(contract["sha256"], hashlib.sha256(payload).hexdigest())

    def test_missing_official_asset_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            release_dir = Path(temp_dir)
            for filename in list(MANIFEST.official_update_assets("3.4.0").values())[:-1]:
                (release_dir / filename).write_bytes(b"payload")
            with self.assertRaises(FileNotFoundError):
                MANIFEST.build_manifest(release_dir, "3.4.0")

    def test_rejects_noncanonical_version(self) -> None:
        for invalid in ("V3.4.0", "3.4", "03.4.0", "3.4.0-beta"):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                MANIFEST.official_update_assets(invalid)


if __name__ == "__main__":
    unittest.main()

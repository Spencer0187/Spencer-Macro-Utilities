from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("smu_version", ROOT / "scripts" / "version.py")
assert SPEC is not None and SPEC.loader is not None
VERSION = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERSION)


class VersionTests(unittest.TestCase):
    def test_bump_version(self) -> None:
        self.assertEqual(VERSION.bump_version("3.2.1", "patch"), "3.2.2")
        self.assertEqual(VERSION.bump_version("3.2.1", "minor"), "3.3.0")
        self.assertEqual(VERSION.bump_version("3.2.1", "major"), "4.0.0")

    def test_rejects_non_release_versions(self) -> None:
        for invalid in ("V3.3.0", "3.3", "3.3.0-beta", "03.3.0"):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                VERSION.parse_version(invalid)

    def test_repository_frozen_legacy_url_contract(self) -> None:
        url = (ROOT / ".github" / "autoupdaterurl").read_text(encoding="utf-8").strip()
        self.assertEqual(
            url,
            "https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/download/"
            "V3.4.0/Spencer-Macro-Utilities-Windows.zip?legacy={VERSION}",
        )
        self.assertEqual(url.count("{VERSION}"), 1)

    def test_synchronizes_all_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            (root / "AppImage").mkdir()
            (root / ".github").mkdir()
            (root / "version").write_text("3.2.1\n", encoding="utf-8")
            frozen_legacy_url = (
                "https://github.com/example/releases/download/V3.4.0/"
                "Spencer-Macro-Utilities-Windows.zip?legacy={VERSION}\n"
            )
            (root / ".github" / "autoupdaterurl").write_text(
                frozen_legacy_url, encoding="utf-8"
            )
            (root / "AppImage" / "nfpm.yaml").write_text(
                "name: smu\nversion: 3.2.1\n", encoding="utf-8"
            )
            (root / "package.json").write_text(
                json.dumps({"name": "smu", "version": "1.0.0"}) + "\n",
                encoding="utf-8",
            )

            updates = VERSION.synchronized_contents(root, "3.3.0")
            for path, content in updates.items():
                path.write_text(content, encoding="utf-8")

            self.assertEqual(VERSION.check_synchronized(root), [])
            self.assertEqual((root / "version").read_text(encoding="utf-8"), "3.3.0\n")
            self.assertEqual(
                (root / ".github" / "autoupdaterurl").read_text(encoding="utf-8"),
                frozen_legacy_url,
            )


if __name__ == "__main__":
    unittest.main()

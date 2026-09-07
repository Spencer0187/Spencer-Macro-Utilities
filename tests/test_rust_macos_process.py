from __future__ import annotations

import pathlib
import tomllib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]


class RustMacosProcessTests(unittest.TestCase):
    def test_process_manifest_has_target_scoped_appkit_dependencies(self) -> None:
        manifest = tomllib.loads(
            (ROOT / "rust" / "crates" / "smu-process" / "Cargo.toml").read_text(
                encoding="utf-8"
            )
        )
        dependencies = manifest["target"]['cfg(target_os = "macos")']["dependencies"]
        self.assertIn("libc", dependencies)
        self.assertIn("objc2", dependencies)
        self.assertIn("objc2-app-kit", dependencies)

    def test_native_process_adapter_preserves_cpp_contract(self) -> None:
        source = (
            ROOT / "rust" / "crates" / "smu-process" / "src" / "macos.rs"
        ).read_text(encoding="utf-8")
        for expected in (
            "Duration::from_millis(250)",
            "autoreleasepool",
            "NSWorkspace::sharedWorkspace()",
            ".runningApplications()",
            "NSRunningApplication::runningApplicationWithProcessIdentifier",
            ".localizedName()",
            ".bundleIdentifier()",
            ".executableURL()",
            ".bundleURL()",
            ".frontmostApplication()",
            "macos_process_candidate_matches",
            "macos_select_main_pid",
            "libc::SIGSTOP",
            "libc::SIGCONT",
            "libc::getpid()",
            "libc::kill",
        ):
            self.assertIn(expected, source)

    def test_rewrite_workflow_has_native_macos_process_gate(self) -> None:
        workflow = (ROOT / ".github" / "workflows" / "rust-rewrite.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn("macos-process:", workflow)
        self.assertIn("runs-on: macos-latest", workflow)
        self.assertIn("cargo test -p smu-process", workflow)
        self.assertIn(
            "cargo clippy -p smu-process --all-targets -- -D warnings", workflow
        )


if __name__ == "__main__":
    unittest.main()

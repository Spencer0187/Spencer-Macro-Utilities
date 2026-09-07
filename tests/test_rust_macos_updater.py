from __future__ import annotations

import pathlib
import tomllib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]


class RustMacosUpdaterTests(unittest.TestCase):
    def test_macos_app_depends_on_rust_updater(self) -> None:
        manifest = tomllib.loads(
            (ROOT / "rust" / "crates" / "smu-app" / "Cargo.toml").read_text(
                encoding="utf-8"
            )
        )
        dependencies = manifest["target"]['cfg(target_os = "macos")']["dependencies"]
        self.assertIn("smu-updater", dependencies)

    def test_macos_update_checks_use_hardened_curl_but_keep_auto_apply_off(self) -> None:
        service = (
            ROOT / "rust" / "crates" / "smu-app" / "src" / "updater_service.rs"
        ).read_text(encoding="utf-8")
        self.assertIn('#[cfg(target_os = "macos")]\nfn check_for_updates', service)
        self.assertIn("CurlUpdateTransport::macos()", service)
        self.assertIn("UpdateChecker::new(CurlUpdateTransport::macos(), current_platform(), false)", service)
        self.assertIn(
            '#[cfg(any(target_os = "windows", target_os = "macos"))]\n    pub fn confirm_apply',
            service,
        )
        self.assertIn(
            "automatic installation is not supported in this launch mode", service
        )

    def test_macos_curl_profile_matches_cpp_limits(self) -> None:
        source = (
            ROOT / "rust" / "crates" / "smu-updater" / "src" / "linux.rs"
        ).read_text(encoding="utf-8")
        for expected in (
            'const MACOS_CURL_TIMEOUT_SECONDS: &str = "15"',
            'const MACOS_CURL_FILE_TIMEOUT_SECONDS: &str = "120"',
            'program: OsString::from("/usr/bin/curl")',
            "get_timeout_seconds: MACOS_CURL_TIMEOUT_SECONDS",
            "file_timeout_seconds: MACOS_CURL_FILE_TIMEOUT_SECONDS",
            'OsStr::new("=https")',
            "User-Agent: Spencer-Macro-Utilities-Updater",
        ):
            self.assertIn(expected, source)

    def test_macos_eligibility_matches_signed_installed_bundle_contract(self) -> None:
        source = (
            ROOT / "rust" / "crates" / "smu-updater" / "src" / "macos.rs"
        ).read_text(encoding="utf-8")
        for expected in (
            "macos_update_bundle_if_replaceable",
            "DirBuilderExt as _",
            "builder.mode(0o700)",
            'Some("app")',
            'Path::new("/Volumes")',
            '"/usr/bin/codesign"',
            '"--verify", "--deep", "--strict"',
            '"--extract-certificates=',
            'signer_dir.join("signer-0")',
            "libc::geteuid()",
            "libc::S_ISVTX",
            'unique_directory(parent, ".smu-update-write-test")',
            "macos_auto_apply_supported",
            "macos_auto_apply_unavailable_reason",
        ):
            self.assertIn(expected, source)
        self.assertNotIn('Command::new("/bin/mv")', source)
        self.assertNotIn("apply-update.sh", source)

    def test_macos_ci_runs_updater_and_source_contract(self) -> None:
        workflow = (ROOT / ".github" / "workflows" / "rust-rewrite.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn("tests.test_rust_macos_updater", workflow)
        self.assertIn(
            "cargo test -p smu-process -p smu-input -p smu-display -p smu-updater -p smu-app",
            workflow,
        )
        self.assertIn(
            "cargo clippy -p smu-process -p smu-input -p smu-display -p smu-updater -p smu-app --all-targets -- -D warnings",
            workflow,
        )


if __name__ == "__main__":
    unittest.main()

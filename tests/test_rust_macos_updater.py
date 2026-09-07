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
            "effective_user != parent_metadata.uid()",
            "parent_metadata.mode() & 0o022 != 0",
            "parent_has_extended_acl(parent)?",
            'Command::new("/bin/ls")',
            '.env("LC_ALL", "C")',
            'args(["-l", "-d", "-e"])',
            "containing folder with an extended ACL",
            "libc::S_ISVTX",
            'unique_directory(parent, ".smu-update-write-test")',
            "macos_auto_apply_supported",
            "macos_auto_apply_unavailable_reason",
        ):
            self.assertIn(expected, source)
        self.assertNotIn('Command::new("/bin/mv")', source)
        self.assertNotIn("apply-update.sh", source)

    def test_dormant_macos_staging_binds_authenticated_zip_to_same_signer_app(self) -> None:
        updater = (
            ROOT / "rust" / "crates" / "smu-updater" / "src" / "macos.rs"
        ).read_text(encoding="utf-8")
        bundle = (
            ROOT / "rust" / "crates" / "smu-updater" / "src" / "macos_bundle.rs"
        ).read_text(encoding="utf-8")
        for expected in (
            "stage_verified_macos_update_asset",
            "read_verified_update_asset(asset, downloaded_path)",
            "validate_macos_bundle_archive(&authenticated_bytes)",
            'unique_directory(Path::new("/tmp"), "smu-macos-updater")',
            'work_dir.join("update.zip")',
            'Command::new("/usr/bin/ditto")',
            '.args(["-x", "-k"])',
            "require_matching_macos_release_signer(installed_bundle, staged_bundle)",
            "pub fn require_matching_macos_release_signer",
            "trusted_certificate != candidate_certificate",
            "pub struct StagedMacosBundle",
            "impl Drop for StagedMacosBundle",
            "fs::remove_dir_all(&self.work_dir)",
        ):
            self.assertIn(expected, updater)
        for expected in (
            "MAXIMUM_MACOS_BUNDLE_ENTRIES",
            "MAXIMUM_UPDATE_BYTES",
            'parts.first() == Some(&"__MACOSX")',
            'strip_prefix("._")',
            "entry.encrypted()",
            "entry.unix_mode()",
            "entry_is_supported_file",
            "BTreeSet",
            'format!("{root_app}/Contents/Info.plist")',
            'format!("{root_app}/Contents/MacOS/")',
        ):
            self.assertIn(expected, bundle)
        self.assertNotIn('Command::new("/bin/mv")', updater)
        self.assertNotIn("rename(&installed", updater)
        self.assertNotIn("macos_auto_apply_supported() -> bool {\n    true", updater)

    def test_dormant_macos_swap_transaction_pins_bundle_identity_and_rolls_back(self) -> None:
        updater = (
            ROOT / "rust" / "crates" / "smu-updater" / "src" / "macos.rs"
        ).read_text(encoding="utf-8")
        replacement = (
            ROOT / "rust" / "crates" / "smu-updater" / "src" / "macos_replace.rs"
        ).read_text(encoding="utf-8")
        for expected in (
            "pub fn prepare_macos_replacement",
            'unique_directory(parent, ".smu-update-swap")',
            'Command::new("/usr/bin/ditto")',
            "staged.app_path()",
            "validate_staged_bundle_signer(&current_app, &new_app)",
            "PreparedMacosReplacement",
        ):
            self.assertIn(expected, updater)
        prepare_block = updater.split("pub fn prepare_macos_replacement", 1)[1].split(
            "fn validate_staged_bundle_signer", 1
        )[0]
        self.assertLess(
            prepare_block.index("let current_metadata"),
            prepare_block.index("validate_staged_bundle_signer"),
        )
        self.assertLess(
            prepare_block.index("let new_metadata"),
            prepare_block.index("validate_staged_bundle_signer"),
        )
        self.assertLess(
            prepare_block.index("validate_staged_bundle_signer"),
            prepare_block.index("PreparedMacosReplacement::from_paths("),
        )
        for expected in (
            "struct DirectoryIdentity",
            "metadata.dev()",
            "metadata.ino()",
            "open_pinned_directory",
            "libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC",
            "libc::fstatat",
            "libc::AT_SYMLINK_NOFOLLOW",
            "libc::renameat",
            "libc::openat",
            "libc::unlinkat",
            "open_verified_child_directory",
            "remove_open_directory_tree_at",
            "directory changed identity after opening",
            "pinned root directory changed identity",
            "fdopendir",
            "swap_identity",
            "Pinned macOS update swap directory changed identity",
            "require_child_directory_identity",
            "activate_prepared_macos_replacement",
            "rename_at(",
            "pub struct MacosReplacementTransaction",
            "pub const fn recovery_available",
            "pub fn recover",
            "MacosReplaceError::with_recovery",
            "impl Drop for MacosReplaceError",
            "pub fn commit",
            "commit_identity_failure_retains_recoverable_rollback_state",
            "RecoveryState::Rollback(recovery)",
            "pub fn rollback",
            "enum RecoveryState",
            "enum ActivationPhase",
            "ActivationPhase::OldRestored",
            "ActivationPhase::PreparedUpdateRemoved",
            "activation_recovery_resumes_after_old_app_was_restored",
            "enum RollbackPhase",
            "RollbackPhase::NewMovedAside",
            "RollbackPhase::OldRestored",
            "take_from_transaction",
            "parent: Option<File>",
            "swap: Option<File>",
            'OsStr::new("failed-new.app")',
            "rollback_recovery_resumes_after_new_app_was_moved_aside",
            "impl Drop for MacosReplacementTransaction",
            "pinned_swap_handle_survives_swap_path_rename",
            "prepared_drop_empties_pinned_swap_after_path_rename_without_touching_substitute",
        ):
            self.assertIn(expected, replacement)
        self.assertNotIn('Command::new("/bin/mv")', replacement)
        replacement_production = replacement.split("#[cfg(test)]", 1)[0]
        self.assertNotIn("try_clone()", replacement_production)
        self.assertNotIn("kill(", replacement)
        self.assertNotIn("open -a", replacement)
        self.assertNotIn("auto_apply", replacement)

    def test_dormant_macos_helper_reauthenticates_release_and_pins_old_process(self) -> None:
        workspace = (ROOT / "rust" / "Cargo.toml").read_text(encoding="utf-8")
        manifest = tomllib.loads(
            (
                ROOT
                / "rust"
                / "crates"
                / "smu-macos-updater-helper"
                / "Cargo.toml"
            ).read_text(encoding="utf-8")
        )
        helper = (
            ROOT
            / "rust"
            / "crates"
            / "smu-macos-updater-helper"
            / "src"
            / "macos.rs"
        ).read_text(encoding="utf-8")
        main = (
            ROOT / "rust" / "crates" / "smu-macos-updater-helper" / "src" / "main.rs"
        ).read_text(encoding="utf-8")
        app_sources = "\n".join(
            path.read_text(encoding="utf-8")
            for path in (ROOT / "rust" / "crates" / "smu-app" / "src").glob("*.rs")
        )

        self.assertIn('"crates/smu-macos-updater-helper"', workspace)
        self.assertEqual(set(manifest["dependencies"]), {"smu-updater"})
        self.assertEqual(
            set(manifest["target"]['cfg(target_os = "macos")']["dependencies"]),
            {"libc"},
        )
        self.assertNotIn("sdl3", manifest["dependencies"])
        self.assertNotIn("objc2", manifest["dependencies"])
        self.assertIn('#[cfg(target_os = "macos")]\nmod macos;', main)

        for expected in (
            'const APPLY_UPDATE_FLAG: &str = "--apply-update"',
            "include_str!(\"../../../../version\")",
            "is_canonical_manifest_version",
            "detached_current_helper",
            "helper.starts_with(installed_bundle)",
            "parent_metadata.uid() != unsafe { libc::geteuid() }",
            "parent_metadata.mode() & 0o077 != 0",
            "path_has_extended_acl(helper_parent)?",
            "detached directory with an extended ACL",
            "require_matching_macos_release_signer(installed_bundle, &helper)",
            "libc::kqueue()",
            "libc::EVFILT_PROC",
            "libc::NOTE_EXIT",
            "libc::EV_ONESHOT",
            "let event_data = event.data",
            "let event_flags = event.flags",
            "libc::proc_pidpath",
            ".position(|byte| *byte == 0)",
            "macOS process executable path was empty",
            "actual_executable != expected_executable",
            "struct ReadyMarker",
            "create_new(true)",
            ".mode(0o600)",
            "ready_path.parent() != Some(helper_parent)",
            'ready_path.file_name() != Some(OsStr::new("ready"))',
            "let _ready = ReadyMarker::create(&helper, &args.ready_path)?",
            "UpdateChecker::new(CurlUpdateTransport::macos(), UpdatePlatform::MacOs, false)",
            "fetch_release_version(target_version)",
            "is_updater_version_allowed(&release, helper_version())",
            "select_update_asset(&release, UpdatePlatform::MacOs)",
            "download_to_file(&asset.download_url, &package)",
            "stage_verified_macos_update_asset_for_bundle",
            "prepare_macos_replacement_for_bundle",
            "compare_versions(&args.target_version, &installed_version) != Ordering::Greater",
            "staged_version != args.target_version",
            "old_process.wait_for_exit()",
            "old_process_is_gone_or_reused",
            "libc::kill(pid, 0)",
            "Some(libc::ESRCH)",
            "activate_prepared_macos_replacement",
            "recover_activation_failure",
            "error.recovery_available()",
            "error.recover()",
            'Command::new("/usr/bin/open")',
            '.arg("-n")',
            'Command::new("/usr/bin/xattr")',
            "macos-permissions-may-need-repair",
            "transaction.rollback()",
            "error.recovery_available()",
            "error.recover()",
            "fn finish_commit",
            "transaction.commit()",
            "finish_commit(transaction, &installed_bundle)",
        ):
            self.assertIn(expected, helper)

        run_block = helper.split("pub fn run()", 1)[1]
        self.assertLess(
            run_block.index("prepare_macos_replacement_for_bundle"),
            run_block.index("ReadyMarker::create"),
        )
        self.assertLess(
            run_block.index("ReadyMarker::create"),
            run_block.index("old_process.wait_for_exit"),
        )

        self.assertNotIn("smu_macos_updater_helper", app_sources)
        self.assertNotIn("smu-macos-updater-helper", app_sources)
        self.assertNotIn("launch_macos_update_helper", app_sources)

    def test_explicit_bundle_updater_apis_support_detached_helper_without_enabling_app_apply(self) -> None:
        source = (
            ROOT / "rust" / "crates" / "smu-updater" / "src" / "macos.rs"
        ).read_text(encoding="utf-8")
        for expected in (
            "pub fn macos_update_bundle_if_replaceable_at",
            "pub fn prepare_macos_replacement_for_bundle",
            "pub fn stage_verified_macos_update_asset_for_bundle",
            "pub fn require_matching_macos_release_signer",
            "macos_update_bundle_if_replaceable_at(&current_app_bundle_path()?)",
            "prepare_macos_replacement_for_bundle(staged, &current_app_bundle_path()?)",
            "stage_verified_macos_update_asset_for_bundle(",
        ):
            self.assertIn(expected, source)

        service = (
            ROOT / "rust" / "crates" / "smu-app" / "src" / "updater_service.rs"
        ).read_text(encoding="utf-8")
        self.assertIn(
            "UpdateChecker::new(CurlUpdateTransport::macos(), current_platform(), false)",
            service,
        )

    def test_macos_ci_runs_updater_and_source_contract(self) -> None:
        workflow = (ROOT / ".github" / "workflows" / "rust-rewrite.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn("tests.test_rust_macos_updater", workflow)
        self.assertIn(
            "cargo test -p smu-process -p smu-input -p smu-display -p smu-updater -p smu-macos-updater-helper -p smu-app",
            workflow,
        )
        self.assertIn(
            "cargo clippy -p smu-process -p smu-input -p smu-display -p smu-updater -p smu-macos-updater-helper -p smu-app --all-targets -- -D warnings",
            workflow,
        )


if __name__ == "__main__":
    unittest.main()

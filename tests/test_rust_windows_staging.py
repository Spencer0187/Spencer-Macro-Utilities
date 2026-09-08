import hashlib
import importlib.util
from pathlib import Path
import tempfile
import tomllib
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "stage_rust_windows_app.py"
SPEC = importlib.util.spec_from_file_location("stage_rust_windows_app", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)

PACKAGE_SCRIPT = ROOT / "scripts" / "package_rust_windows_bundle.py"
PACKAGE_SPEC = importlib.util.spec_from_file_location(
    "package_rust_windows_bundle", PACKAGE_SCRIPT
)
assert PACKAGE_SPEC is not None and PACKAGE_SPEC.loader is not None
PACKAGE_MODULE = importlib.util.module_from_spec(PACKAGE_SPEC)
PACKAGE_SPEC.loader.exec_module(PACKAGE_MODULE)


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
            helper = root / "smu-windows-updater-helper.exe"
            sdl = root / "SDL3.dll"
            windivert_dll = root / "SMCWinDivert.dll"
            windivert_driver = root / "SMCWinDivert64.sys"
            assets = root / "source-assets"
            tutorials = assets / "macro_tutorials"
            tutorials.mkdir(parents=True)

            binary.write_bytes(b"rust-app")
            helper.write_bytes(b"rust-helper")
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
                helper,
                sdl,
                assets,
                windivert_dll,
                windivert_driver,
                stage,
            )

            self.assertEqual(staged[0], stage / "Spencer-Macro-Utilities.exe")
            self.assertEqual(staged[1], stage / "SMU-Update-Helper.exe")
            self.assertEqual((stage / "Spencer-Macro-Utilities.exe").read_bytes(), b"rust-app")
            self.assertEqual((stage / "SMU-Update-Helper.exe").read_bytes(), b"rust-helper")
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
                    existing,
                    root / "missing-sdl.dll",
                    assets,
                    existing,
                    existing,
                    root / "stage",
                )

    def test_package_bundle_contains_runtime_tree_and_release_notices(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            temp = Path(directory)
            stage = temp / "stage"
            assets = stage / "assets" / "macro_tutorials"
            assets.mkdir(parents=True)
            for name in PACKAGE_MODULE.RUNTIME_FILES:
                (stage / name).write_bytes(name.encode())
            (stage / "assets" / "LSANS.TTF").write_bytes(b"font")
            (assets / "wallhop.jpg").write_bytes(b"tutorial")
            output = temp / "rust-windows.zip"

            PACKAGE_MODULE.package_bundle(ROOT, stage, output)

            with zipfile.ZipFile(output) as archive:
                names = set(archive.namelist())
            prefix = f"{PACKAGE_MODULE.PACKAGE_ROOT}/"
            for name in PACKAGE_MODULE.RUNTIME_FILES:
                self.assertIn(prefix + name, names)
            self.assertIn(prefix + "assets/LSANS.TTF", names)
            self.assertIn(prefix + "assets/macro_tutorials/wallhop.jpg", names)
            self.assertIn(prefix + "README.md", names)
            self.assertIn(prefix + "LICENSE", names)
            self.assertIn(prefix + "PRIVACY.md", names)
            self.assertIn(prefix + "THIRD_PARTY_NOTICES.md", names)
            self.assertIn(prefix + "licenses/SDL.txt", names)
            self.assertIn(prefix + "licenses/WinDivert-LGPL-3.0.txt", names)
            self.assertNotIn("Spencer-Macro-Utilities.exe", names)

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
        self.assertEqual(MODULE.HELPER_OUTPUT_NAME, "SMU-Update-Helper.exe")
        self.assertEqual(MODULE.HELPER_CARGO_BINARY_NAME, "smu-windows-updater-helper.exe")

    def test_windows_pe_resources_match_cpp_product_identity_contract(self) -> None:
        build_script = (ROOT / "rust" / "crates" / "smu-app" / "build.rs").read_text(
            encoding="utf-8"
        )
        manifest = tomllib.loads(
            (ROOT / "rust" / "crates" / "smu-app" / "Cargo.toml").read_text(encoding="utf-8")
        )

        self.assertEqual(manifest["build-dependencies"]["winres"], "=0.1.12")
        for expected in (
            'root.join("version")',
            'root.join("resources/windows/smu.ico")',
            '.set_language(0x0409)',
            '.set_icon_with_id(icon_path, "102")',
            '"CompanyName", "Spencer\'s Open Source Macro Industries"',
            '"FileDescription", "Spencer Macro Utilities"',
            '"InternalName", "Spencer-Macro-Utilities.exe"',
            '"LegalCopyright", "Copyright (C) 2025"',
            'format!("Spencer-Macro-Utilities-V{version}-Windows.exe")',
            '"ProductName", "Spencer Macro Utilities"',
            "VersionInfo::FILEVERSION",
            "VersionInfo::PRODUCTVERSION",
        ):
            self.assertIn(expected, build_script)

    def test_windows_windivert_failure_keeps_real_error_and_offers_admin_independently(self) -> None:
        main = (ROOT / "rust" / "crates" / "smu-app" / "src" / "main.rs").read_text(
            encoding="utf-8"
        )
        network = (
            ROOT / "rust" / "crates" / "smu-network" / "src" / "windows.rs"
        ).read_text(encoding="utf-8")

        for expected in (
            "ERROR_ACCESS_DENIED",
            "last_error_requires_administrator",
            "error.requires_administrator()",
            "state.last_error_requires_administrator = requires_administrator",
            "if state.last_error_requires_administrator",
            "LagSwitchInitializationFailure::RequiresAdministrator",
            "LagSwitchInitializationFailure::Error(state.last_error.clone())",
            "only_access_denied_load_failures_require_administrator",
        ):
            self.assertIn(expected, network)
        self.assertNotIn(
            "if !is_running_as_admin() {\n            return LagSwitchInitializationFailure::RequiresAdministrator;",
            network,
        )
        for expected in (
            "shell_state.push_critical(reason);",
            "if !smu_network::windows::is_running_as_admin()",
            "shell_state.request_admin_restart_popup();",
            "fn script_lag_switch_initialization_failure(",
            "smu_network::LagSwitchInitializationFailure::RequiresAdministrator",
            "script_lag_switch_initialization_failure(",
            "smu_network::windows::is_running_as_admin(),",
            "smu_network::LagSwitchInitializationFailure::Error(String::new())",
        ):
            self.assertIn(expected, main)

    def test_windows_admin_relaunch_preserves_cpp_modal_save_and_runas_contract(self) -> None:
        main = (ROOT / "rust" / "crates" / "smu-app" / "src" / "main.rs").read_text(
            encoding="utf-8"
        )
        shell = (ROOT / "rust" / "crates" / "smu-app" / "src" / "shell.rs").read_text(
            encoding="utf-8"
        )
        elevation = (
            ROOT / "rust" / "crates" / "smu-app" / "src" / "windows_elevation.rs"
        ).read_text(encoding="utf-8")
        manifest = tomllib.loads(
            (ROOT / "rust" / "crates" / "smu-app" / "Cargo.toml").read_text(encoding="utf-8")
        )
        windows_sys = manifest["target"]["cfg(target_os = \"windows\")"]["dependencies"][
            "windows-sys"
        ]

        for expected in (
            "RestartAsAdmin",
            "request_admin_restart_popup",
            "render_windows_admin_modal",
            'begin_modal_popup_config("Administrator Required")',
            'ui.button_with_size("Restart as Admin"',
            'ui.checkbox("Do not show this again"',
        ):
            self.assertIn(expected, shell)
        for expected in (
            "privileged_shell_action_needs_save",
            "ShellAction::RestartAsAdmin",
            "save_profiles_now",
            "handle_windows_privileged_shell_action",
            "windows_elevation::restart_as_admin()",
            "break 'main;",
        ):
            self.assertIn(expected, main)
        for expected in (
            "ShellExecuteExW",
            'OsStr::new("runas")',
            "std::env::current_exe()",
            "SW_SHOWNORMAL",
        ):
            self.assertIn(expected, elevation)
        for feature in (
            "Win32_Foundation",
            "Win32_System_Registry",
            "Win32_UI_Shell",
            "Win32_UI_WindowsAndMessaging",
        ):
            self.assertIn(feature, windows_sys["features"])

    def test_windows_script_drop_matches_cpp_sdl_and_elevated_uipi_contract(self) -> None:
        main = (ROOT / "rust" / "crates" / "smu-app" / "src" / "main.rs").read_text(
            encoding="utf-8"
        )
        script_ui = (
            ROOT / "rust" / "crates" / "smu-app" / "src" / "script_ui.rs"
        ).read_text(encoding="utf-8")
        drop_filter = (
            ROOT / "rust" / "crates" / "smu-app" / "src" / "windows_drop_filter.rs"
        ).read_text(encoding="utf-8")

        for expected in (
            "EventType::DropFile",
            "set_event_enabled(EventType::DropFile, true)",
            "Event::DropFile { filename, .. }",
            "queue_dropped_import_candidate(path.clone())",
            'format!("Rejected dropped script file: {}", path.display())',
            "windows_drop_filter::allow_elevated_file_drops(hwnd)",
        ):
            self.assertIn(expected, main)
        for expected in (
            "pub fn queue_dropped_import_candidate",
            "!is_supported_script_path(&path)",
            "!path.is_file()",
            "self.queue_import_candidate(path)",
        ):
            self.assertIn(expected, script_ui)
        for expected in (
            "GetModuleHandleW",
            "GetProcAddress",
            'c"ChangeWindowMessageFilterEx"',
            "WM_DROPFILES",
            "WM_COPYDATA",
            "const WM_COPYGLOBALDATA: u32 = 0x0049",
            "MSGFLT_ALLOW",
            "change_message_filter(hwnd, message, MSGFLT_ALLOW, null_mut())",
            "Could not find Win32 window handle for drag-and-drop message filter.",
            "ChangeWindowMessageFilterEx is unavailable; elevated Windows drag-and-drop may be blocked.",
        ):
            self.assertIn(expected, drop_filter)

    def test_windows_lag_switch_overlay_matches_cpp_layered_window_contract(self) -> None:
        main = (ROOT / "rust" / "crates" / "smu-app" / "src" / "main.rs").read_text(
            encoding="utf-8"
        )
        overlay = (
            ROOT / "rust" / "crates" / "smu-app" / "src" / "windows_lag_overlay.rs"
        ).read_text(encoding="utf-8")
        lag_switch = (
            ROOT / "rust" / "crates" / "smu-app" / "src" / "lag_switch.rs"
        ).read_text(encoding="utf-8")
        shell = (ROOT / "rust" / "crates" / "smu-app" / "src" / "shell.rs").read_text(
            encoding="utf-8"
        )
        manifest = tomllib.loads(
            (ROOT / "rust" / "crates" / "smu-app" / "Cargo.toml").read_text(encoding="utf-8")
        )
        windows_sys = manifest["target"]["cfg(target_os = \"windows\")"]["dependencies"][
            "windows-sys"
        ]

        for expected in (
            "WS_EX_TOPMOST",
            "WS_EX_LAYERED",
            "WS_EX_TRANSPARENT",
            "WS_EX_TOOLWINDOW",
            "WS_EX_NOACTIVATE",
            "WS_POPUP",
            "UpdateLayeredWindow",
            "ULW_ALPHA",
            "Lagswitch ON",
            "Lagswitch OFF",
            "AddFontResourceExW",
            "RemoveFontResourceExW",
            "crate::theme::font_path()",
            "settings.shown && backend_enabled",
            "settings.hide_inactive",
            "SM_CXSCREEN",
            "primary_screen_default_x",
            "SM_XVIRTUALSCREEN",
            "SM_YVIRTUALSCREEN",
            "SM_CXVIRTUALSCREEN",
            "SM_CYVIRTUALSCREEN",
            "pub fn virtual_desktop_bounds() -> VirtualDesktopBounds",
            "self.min_x.saturating_add(self.width.saturating_mul(4) / 5)",
            "pub fn shutdown(&mut self)",
        ):
            self.assertIn(expected, overlay)
        for expected in (
            "show_lag_overlay",
            "overlay_hide_inactive",
            "overlay_use_bg",
            "overlay_x",
            "overlay_y",
            "overlay_size",
            "overlay_bg_r",
            "overlay_bg_g",
            "overlay_bg_b",
            "windows_overlay_settings",
            "let virtual_desktop = crate::windows_lag_overlay::virtual_desktop_bounds()",
            "if state.x == -1",
            "state.x = virtual_desktop.default_overlay_x()",
            "virtual_desktop.min_x",
            "virtual_desktop.max_x()",
            "virtual_desktop.min_y",
            "virtual_desktop.max_y()",
            "set_overlay_x",
        ):
            self.assertIn(expected, lag_switch)
        for expected in (
            "windows_lag_overlay_settings",
            "set_windows_lag_overlay_x",
        ):
            self.assertIn(expected, shell)
        for expected in (
            "overlay: windows_lag_overlay::WindowsLagOverlay",
            "self.overlay.update(overlay_settings, backend_enabled, actively_lagging)",
            "shell_state.set_windows_lag_overlay_x(resolved_x)",
            "self.overlay.shutdown()",
            "lag_switch: &mut WindowsLagSwitchServices",
            "windows_lag_switch: &mut windows_lag_switch",
        ):
            self.assertIn(expected, main)
        for feature in ("Win32_Graphics_Gdi", "Win32_System_LibraryLoader"):
            self.assertIn(feature, windows_sys["features"])

    def test_windows_v340_suspend_filename_bridge_matches_cpp_one_release_contract(self) -> None:
        main = (ROOT / "rust" / "crates" / "smu-app" / "src" / "main.rs").read_text(
            encoding="utf-8"
        )
        migration = (
            ROOT / "rust" / "crates" / "smu-app" / "src" / "windows_legacy_filename.rs"
        ).read_text(encoding="utf-8")
        manifest = tomllib.loads(
            (ROOT / "rust" / "crates" / "smu-app" / "Cargo.toml").read_text(encoding="utf-8")
        )
        windows_sys = manifest["target"]["cfg(target_os = \"windows\")"]["dependencies"][
            "windows-sys"
        ]

        self.assertEqual((ROOT / "version").read_text(encoding="utf-8"), "3.4.0\n")
        for expected in (
            'const VERSION_BYTES: &[u8] = include_bytes!("../../../../version")',
            'const LEGACY_FILENAME: &str = "suspend.exe"',
            'const V340_TARGET_FILENAME: &str = "Spencer-Macro-Utilities-V3.4.0-Windows.exe"',
            "assert!(VERSION_BYTES[4] == b'0')",
            "GetModuleFileNameW",
            "FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE",
            "GetFileSizeEx",
            "ReadFile",
            "CopyFileW",
            "GetFileAttributesW",
            "FILE_ATTRIBUTE_DIRECTORY",
            "CREATE_NEW",
            "FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_TEMPORARY",
            "FlushFileBuffers",
            "GetSystemDirectoryW",
            '".smu-v3.4.0-filename-migration-{process_id}-{tick_count}.cmd"',
            'MIGRATION_SCRIPT.contains("%SMU_SYSTEM%\\\\timeout.exe")',
            '"SMU_DELETE_ATTEMPTS% GEQ 300"',
            'MIGRATION_SCRIPT.contains("%SMU_SYSTEM%\\\\WindowsPowerShell\\\\v1.0\\\\powershell.exe")',
            "CreateProcessW",
            "CREATE_NO_WINDOW",
            "delete_file_best_effort(&script_path)",
            "delete_file_best_effort(&target_path)",
            "Nothing was overwritten.",
        ):
            self.assertIn(expected, migration)
        migration_call = "windows_legacy_filename::schedule_v340_legacy_filename_migration()"
        timing_call = "windows_timing::WindowsTimingGuard::configure()"
        self.assertIn(migration_call, main)
        self.assertLess(main.index(migration_call), main.index(timing_call))
        self.assertIn("if windows_legacy_filename::schedule_v340_legacy_filename_migration()", main)
        self.assertIn("return Ok(());", main[main.index(migration_call) : main.index(timing_call)])
        for feature in (
            "Win32_Security",
            "Win32_Storage_FileSystem",
            "Win32_System_IO",
        ):
            self.assertIn(feature, windows_sys["features"])

    def test_windows_process_timing_policy_matches_cpp_timer_and_power_throttling_contract(self) -> None:
        main = (ROOT / "rust" / "crates" / "smu-app" / "src" / "main.rs").read_text(
            encoding="utf-8"
        )
        timing = (
            ROOT / "rust" / "crates" / "smu-app" / "src" / "windows_timing.rs"
        ).read_text(encoding="utf-8")
        manifest = tomllib.loads(
            (ROOT / "rust" / "crates" / "smu-app" / "Cargo.toml").read_text(encoding="utf-8")
        )
        windows_sys = manifest["target"]["cfg(target_os = \"windows\")"]["dependencies"][
            "windows-sys"
        ]

        self.assertIn("let _windows_timing = windows_timing::WindowsTimingGuard::configure();", main)
        for expected in (
            "timeGetDevCaps",
            "timeBeginPeriod",
            "timeEndPeriod",
            "PROCESS_POWER_THROTTLING_EXECUTION_SPEED",
            "PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION",
            "SetProcessInformation",
            "ProcessPowerThrottling",
            "FULL_CONTROL_MASK",
            "EXECUTION_SPEED_ONLY",
            "impl Drop for WindowsTimingGuard",
            "requested_timer_period(caps.wPeriodMin, caps.wPeriodMax)",
            "Windows refused the high-resolution timer-period request",
            "Windows refused process power-throttling configuration",
        ):
            self.assertIn(expected, timing)
        for feature in ("Win32_Media", "Win32_System_Threading"):
            self.assertIn(feature, windows_sys["features"])

    def test_windows_updater_uses_wininet_checks_without_enabling_auto_apply(self) -> None:
        updater = (ROOT / "rust" / "crates" / "smu-updater" / "src" / "windows.rs").read_text(
            encoding="utf-8"
        )
        updater_lib = (ROOT / "rust" / "crates" / "smu-updater" / "src" / "lib.rs").read_text(
            encoding="utf-8"
        )
        updater_service = (
            ROOT / "rust" / "crates" / "smu-app" / "src" / "updater_service.rs"
        ).read_text(encoding="utf-8")
        main = (ROOT / "rust" / "crates" / "smu-app" / "src" / "main.rs").read_text(
            encoding="utf-8"
        )
        updater_manifest = tomllib.loads(
            (ROOT / "rust" / "crates" / "smu-updater" / "Cargo.toml").read_text(
                encoding="utf-8"
            )
        )

        self.assertIn('mod windows;', updater_lib)
        self.assertIn('WinInetUpdateTransport', updater_lib)
        self.assertIn('verify_authenticode_signature', updater_lib)
        self.assertIn('authenticode_signer_sha256', updater_lib)
        self.assertIn('require_matching_authenticode_signer', updater_lib)
        self.assertIn('windows_file_version', updater_lib)
        self.assertIn('WindowsFileVersion', updater_lib)
        for expected in (
            "InternetOpenW",
            "InternetOpenUrlW",
            "HttpQueryInfoW",
            "InternetReadFile",
            "INTERNET_OPTION_CONNECT_TIMEOUT",
            "INTERNET_OPTION_RECEIVE_TIMEOUT",
            'url.starts_with("https://")',
            "MAXIMUM_UPDATE_BYTES",
            "if data.is_empty()",
            "Spencer-Macro-Utilities-Updater",
            "application/vnd.github+json",
            "WinVerifyTrust",
            "WINTRUST_ACTION_GENERIC_VERIFY_V2",
            "WTD_REVOKE_WHOLECHAIN",
            "WTD_REVOCATION_CHECK_CHAIN",
            "CryptQueryObject",
            "CryptMsgGetParam",
            "CertFindCertificateInStore",
            "CERT_SHA256_HASH_PROP_ID",
            "Downloaded update was signed by a different certificate",
            "verify_authenticode_signature(current_executable)?;",
            "verify_authenticode_signature(downloaded_executable)?;",
            "GetFileVersionInfoSizeW",
            "GetFileVersionInfoW",
            "VerQueryValueW",
            "VS_FIXEDFILEINFO",
            "std::ptr::read_unaligned",
            "dwFileVersionMS",
            "dwFileVersionLS",
            "dwStrucVersion",
        ):
            self.assertIn(expected, updater)
        windows_sys = updater_manifest["target"]["cfg(target_os = \"windows\")"][
            "dependencies"
        ]["windows-sys"]
        for feature in (
            "Win32_Foundation",
            "Win32_Networking_WinInet",
            "Win32_Security_Cryptography",
            "Win32_Security_WinTrust",
            "Win32_Storage_FileSystem",
        ):
            self.assertIn(feature, windows_sys["features"])
        self.assertIn("UpdateChecker::new(WinInetUpdateTransport, current_platform(), false)", updater_service)
        for expected in (
            "update_service: updater_service::UpdateService",
            "self.update_service.prepare()",
            "self.update_service.start_check(true)",
            "self.update_service.request_apply()",
            "self.update_service.confirm_apply()",
            "self.update_service.open_latest_release()",
            "self.update_service.shutdown()",
            "fn update_restart_requested(&self) -> bool",
            "if windows_builtin_runtime.update_restart_requested()",
        ):
            self.assertIn(expected, main)
        for expected in (
            "use smu_updater::{WinInetUpdateTransport, launch_windows_update_helper}",
            '#[cfg(target_os = "windows")]\n    pub fn confirm_apply',
            "if !state.status.auto_apply_supported",
            "state.status.selected_asset.is_none()",
            "launch_windows_update_helper(&target_version)",
            "ui.restart_requested = true",
            "Signed Windows update helper launched. The app will restart shortly.",
            "update_check_failure_message(&status.message)",
            "update_apply_failure_message(&error.to_string())",
            "log_warning(&warning)",
            "log_warning(&message)",
        ):
            self.assertIn(expected, updater_service)
        self.assertLess(
            updater_service.index("if !state.status.auto_apply_supported"),
            updater_service.index("launch_windows_update_helper(&target_version)"),
        )

    def test_windows_replacement_pins_authenticated_files_and_target_directories(self) -> None:
        replacement = (
            ROOT / "rust" / "crates" / "smu-updater" / "src" / "windows_replace.rs"
        ).read_text(encoding="utf-8")
        bundle = (
            ROOT / "rust" / "crates" / "smu-updater" / "src" / "windows_bundle.rs"
        ).read_text(encoding="utf-8")

        for expected in (
            "windows_bundle_file_sha256",
            "Sha256::new()",
            "prepare_windows_bundle_tree",
            "expected_sha256",
        ):
            self.assertIn(expected, bundle + replacement)
        for expected in (
            "FILE_ATTRIBUTE_REPARSE_POINT",
            "FILE_FLAG_OPEN_REPARSE_POINT",
            "FILE_FLAG_BACKUP_SEMANTICS",
            ".share_mode(FILE_SHARE_READ)",
            ".access_mode(0)",
            "prepare_target_directory_guards",
            "open_original_for_backup",
            "GENERIC_READ | DELETE",
            "FILE_FLAG_OPEN_REPARSE_POINT",
            "SetFileInformationByHandle",
            "FileRenameInfo",
            "backup_source: Option<File>",
            "backup_sha256: Option<[u8; 32]>",
            "rollback_with_guards",
            "Staged Windows update file changed after authenticated preparation",
        ):
            self.assertIn(expected, replacement)
        self.assertNotIn("FILE_SHARE_WRITE", replacement)
        self.assertNotIn("FILE_SHARE_DELETE", replacement)

    def test_windows_update_launcher_is_trust_pinned_wired_and_capability_gated(self) -> None:
        launcher = (
            ROOT / "rust" / "crates" / "smu-updater" / "src" / "windows_launch.rs"
        ).read_text(encoding="utf-8")
        updater_service = (
            ROOT / "rust" / "crates" / "smu-app" / "src" / "updater_service.rs"
        ).read_text(encoding="utf-8")

        for expected in (
            "launch_windows_update_helper",
            "fetch_release_version(target_version)",
            "verify_downloaded_asset_bytes",
            "is_safe_windows_leaf_name",
            "unsafe release asset filename",
            "DetachedLaunchDirectory::create()",
            "fs::create_dir(&path)",
            "FILE_FLAG_OPEN_REPARSE_POINT",
            "FILE_FLAG_BACKUP_SEMANTICS",
            ".share_mode(FILE_SHARE_READ)",
            ".create_new(true)",
            "require_matching_authenticode_signer",
            "windows_file_version(&current_app)",
            "current_version.revision != 0",
            "Sha256::digest",
            "stage_detached_helper",
            "struct ReadinessEvent",
            "CreateEventW(std::ptr::null(), 1, 0, std::ptr::null())",
            "Command::new(&detached_helper)",
            '.arg("--apply-update")',
            ".arg(std::process::id().to_string())",
            ".arg((readiness.raw() as usize).to_string())",
            "WaitForSingleObject",
            "READINESS_TIMEOUT",
            "READINESS_POLL",
            "READINESS_SURVIVAL",
            "stop_child",
            "wait_for_helper_readiness",
        ):
            self.assertIn(expected, launcher)
        self.assertNotIn("FILE_SHARE_WRITE", launcher)
        self.assertNotIn("FILE_SHARE_DELETE", launcher)
        self.assertIn(
            "UpdateChecker::new(WinInetUpdateTransport, current_platform(), false)",
            updater_service,
        )
        self.assertIn(
            '"An update is available, but automatic installation is not supported in this launch mode."',
            updater_service,
        )
        self.assertIn("launch_windows_update_helper", updater_service)
        self.assertIn("if !state.status.auto_apply_supported", updater_service)

    def test_windows_cutover_requires_production_rust_bundle_signing(self) -> None:
        production = (ROOT / ".github/workflows/build.yml").read_text(encoding="utf-8")
        rewrite = (ROOT / ".github/workflows/rust-rewrite.yml").read_text(encoding="utf-8")
        updater_service = (
            ROOT / "rust" / "crates" / "smu-app" / "src" / "updater_service.rs"
        ).read_text(encoding="utf-8")
        launcher = (
            ROOT / "rust" / "crates" / "smu-updater" / "src" / "windows_launch.rs"
        ).read_text(encoding="utf-8")
        helper = (
            ROOT / "rust" / "crates" / "smu-windows-updater-helper" / "src" / "windows.rs"
        ).read_text(encoding="utf-8")
        bundle = (
            ROOT / "rust" / "crates" / "smu-updater" / "src" / "windows_bundle.rs"
        ).read_text(encoding="utf-8")
        settings = (
            ROOT / "rust" / "crates" / "smu-app" / "src" / "settings_menu.rs"
        ).read_text(encoding="utf-8")
        roadmap = (ROOT / "docs" / "RUST_REWRITE.md").read_text(encoding="utf-8")
        updater_manifest = tomllib.loads(
            (ROOT / "rust" / "crates" / "smu-updater" / "Cargo.toml").read_text(
                encoding="utf-8"
            )
        )

        self.assertIn(
            "cmake --build build/windows --config Release --target spencer_macro_utilities --parallel",
            production,
        )
        self.assertIn("Sign executable with SignPath", production)
        self.assertNotIn("scripts/stage_rust_windows_app.py", production)
        self.assertNotIn("scripts/package_rust_windows_bundle.py", production)
        self.assertIn("scripts/stage_rust_windows_app.py", rewrite)
        self.assertIn("scripts/package_rust_windows_bundle.py", rewrite)
        self.assertIn("rust-windows-release-contract-test-signed", rewrite)
        self.assertIn("New-SelfSignedCertificate", rewrite)
        self.assertIn("Get-AuthenticodeSignature", rewrite)
        self.assertIn("SMU_RUST_WINDOWS_TEST_CERT_THUMBPRINT", rewrite)
        self.assertNotIn("SignPath", rewrite)

        self.assertIn(
            "UpdateChecker::new(WinInetUpdateTransport, current_platform(), false)",
            updater_service,
        )
        self.assertIn("launch_windows_update_helper(&target_version)", updater_service)
        self.assertIn("if !state.status.auto_apply_supported", updater_service)
        for expected in (
            "CreateEventW",
            "Command::new(&detached_helper)",
            "WaitForSingleObject",
            "PROCESS_DUP_HANDLE",
            "DuplicateHandle",
            "DUPLICATE_SAME_ACCESS",
            "GetCurrentProcess",
            "SetEvent(self.0)",
            "readiness.signal()?",
        ):
            self.assertIn(expected, launcher + helper)
        apply_block = helper[
            helper.index("fn apply(args: &ApplyUpdateArgs)") : helper.index("pub fn run()")
        ]
        self.assertLess(
            apply_block.index("prepare_windows_bundle_tree"),
            apply_block.index("readiness.signal()?"),
        )
        self.assertLess(
            apply_block.index("validate_versions_and_signers"),
            apply_block.index("readiness.signal()?"),
        )
        self.assertLess(
            apply_block.index("readiness.signal()?"),
            apply_block.index("wait_for_old_process"),
        )
        windows_features = updater_manifest["target"]['cfg(target_os = "windows")'][
            "dependencies"
        ]["windows-sys"]["features"]
        self.assertIn("Win32_Security", windows_features)
        self.assertIn("Win32_System_Threading", windows_features)
        for expected in (
            "WINDOWS_RELEASE_SIGNER_FILES",
            '"Spencer-Macro-Utilities.exe"',
            '"SMU-Update-Helper.exe"',
            '"SDL3.dll"',
            '"SMCWinDivert.dll"',
            "WINDOWS_UPSTREAM_SIGNER_CONTINUITY_FILES",
            '"SMCWinDivert64.sys"',
        ):
            self.assertIn(expected, bundle)
        for expected in (
            "for relative in WINDOWS_RELEASE_SIGNER_FILES",
            "require_matching_authenticode_signer(&installed_app, &staged)",
            "for relative in WINDOWS_UPSTREAM_SIGNER_CONTINUITY_FILES",
            "require_matching_authenticode_signer(&installed, &staged)",
        ):
            self.assertIn(expected, helper)
        self.assertIn(
            "Automatic installation is not yet enabled for the Rust Windows app.",
            settings,
        )
        for expected in (
            "bundle-wide trust",
            "private unnamed, non-inheritable",
            "readiness event",
            "PROCESS_DUP_HANDLE",
            "local-adversary boundary",
            "SMCWinDivert.dll",
            "auto_apply=false",
        ):
            self.assertIn(expected, roadmap)

    def test_windows_update_helper_is_minimal_authenticated_and_dormant(self) -> None:
        helper_root = ROOT / "rust" / "crates" / "smu-windows-updater-helper"
        manifest = tomllib.loads((helper_root / "Cargo.toml").read_text(encoding="utf-8"))
        main = (helper_root / "src" / "main.rs").read_text(encoding="utf-8")
        windows = (helper_root / "src" / "windows.rs").read_text(encoding="utf-8")
        build_script = (helper_root / "build.rs").read_text(encoding="utf-8")

        self.assertNotIn("sdl3", manifest.get("dependencies", {}))
        self.assertIn(
            '#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]',
            main,
        )
        self.assertIn("windows::show_error(&error)", main)
        self.assertIn("std::process::exit(1)", main)
        self.assertEqual(manifest["build-dependencies"]["winres"], "=0.1.12")
        windows_sys = manifest["target"]['cfg(target_os = "windows")']["dependencies"][
            "windows-sys"
        ]
        self.assertIn("Win32_Storage_FileSystem", windows_sys["features"])
        self.assertIn("Win32_System_SystemInformation", windows_sys["features"])
        self.assertIn("Win32_System_Threading", windows_sys["features"])
        self.assertIn("Win32_UI_WindowsAndMessaging", windows_sys["features"])
        for expected in (
            'const APPLY_UPDATE_FLAG: &str = "--apply-update"',
            "UpdateChecker::new(WinInetUpdateTransport, UpdatePlatform::Windows, false)",
            "fetch_release_version(&args.target_version)",
            "read_verified_update_asset",
            "windows_bundle_file_sha256",
            "extract_windows_bundle",
            "prepare_windows_bundle_tree",
            "require_matching_authenticode_signer",
            "WINDOWS_RELEASE_SIGNER_FILES",
            "WINDOWS_UPSTREAM_SIGNER_CONTINUITY_FILES",
            "Windows update runtime signer validation failed",
            "Windows update upstream signer continuity failed",
            "windows_file_version",
            "detached_current_helper",
            'format!("smu-update-launch-{old_pid}-")',
            "helper_directory.starts_with(&temp_root)",
            "current_helper.starts_with(&install_root)",
            "detached_package_path",
            "package.parent() != Some(helper_directory)",
            "struct ReadyEvent",
            "PROCESS_DUP_HANDLE",
            "DuplicateHandle",
            "DUPLICATE_SAME_ACCESS",
            "GetCurrentProcess",
            "ResetEvent(duplicate)",
            "SetEvent(self.0)",
            "readiness_handle: usize",
            "args.len() != 7",
            "ReadyEvent::duplicate_from_parent(old_process_for_readiness, args.readiness_handle)",
            "readiness.signal()?",
            "DetachedArtifactsCleanup",
            "MessageBoxW",
            "Spencer Macro Utilities Update Failed",
            "GetSystemDirectoryW",
            "spawn_cleanup_process",
            'system_executable_path("ping.exe", "the Windows system cleanup delay utility")',
            'env("SMU_CLEANUP_DELAY", cleanup_delay)',
            "CREATE_NO_WINDOW",
            '"/E:ON"',
            '"/V:OFF"',
            '"rd /Q \\"%SMU_CLEANUP_DIR%\\"',
            "MoveFileExW",
            "MOVEFILE_DELAY_UNTIL_REBOOT",
            "std::fs::remove_file(&package_path)",
            "fresh_staging_directory(&cleanup.directory)",
            "OpenProcess",
            "PROCESS_QUERY_LIMITED_INFORMATION",
            "QueryFullProcessImageNameW",
            "open_expected_old_process",
            "WaitForSingleObject",
            "replace_windows_bundle_tree",
            ".rollback()",
            "rollback_and_relaunch_previous",
            "The previous SMU version was restored and relaunched successfully.",
            "let _ = child.kill()",
            "let _ = child.wait()",
            ".commit()",
            "Command::new(install_root.join(PRODUCT_EXE))",
        ):
            self.assertIn(expected, windows)
        staging_function = windows[
            windows.index("fn fresh_staging_directory(base: &Path)") : windows.index(
                "fn fetch_and_authenticate_package"
            )
        ]
        self.assertIn("base.join", staging_function)
        self.assertNotIn("std::env::temp_dir()", staging_function)
        for expected in (
            '"FileDescription", "Spencer Macro Utilities Update Helper"',
            '"InternalName", "SMU-Update-Helper.exe"',
            'format!("SMU-Update-Helper-V{version}-Windows.exe")',
            "VersionInfo::FILEVERSION",
            "VersionInfo::PRODUCTVERSION",
        ):
            self.assertIn(expected, build_script)

    def test_rewrite_workflow_has_isolated_test_signed_windows_rust_validation(self) -> None:
        workflow = (ROOT / ".github" / "workflows" / "rust-rewrite.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn("windows-rust-app:", workflow)
        self.assertIn("runs-on: windows-latest", workflow)
        self.assertIn(
            "cargo test --manifest-path rust/Cargo.toml -p smu-app -p smu-updater -p smu-windows-updater-helper",
            workflow,
        )
        self.assertIn(
            "cargo clippy --manifest-path rust/Cargo.toml -p smu-app -p smu-updater -p smu-windows-updater-helper --all-targets -- -D warnings",
            workflow,
        )
        self.assertIn("scripts/stage_rust_windows_app.py", workflow)
        self.assertIn("scripts/package_rust_windows_bundle.py", workflow)
        self.assertIn("scripts/generate_update_manifest.py", workflow)
        self.assertIn("--only-platform windows-x64", workflow)
        self.assertIn("SMU-Update-Helper.exe", workflow)
        self.assertIn("New-SelfSignedCertificate", workflow)
        self.assertIn('"Cert:\\CurrentUser\\Root"', workflow)
        self.assertIn("SMU_RUST_WINDOWS_TEST_CERT_THUMBPRINT", workflow)
        self.assertIn("signtool.exe", workflow)
        self.assertIn("Get-AuthenticodeSignature", workflow)
        self.assertIn("Bundled WinDivert driver does not carry a valid upstream Authenticode signature", workflow)
        self.assertIn("Packaged WinDivert driver did not preserve its valid upstream signer", workflow)
        self.assertIn("rust-windows-app-test-signed", workflow)
        self.assertIn("rust-windows-release-contract-test-signed", workflow)
        self.assertNotIn("rust-windows-app-unsigned", workflow)
        self.assertNotIn("rust-windows-release-contract-unsigned", workflow)
        self.assertNotIn("SignPath", workflow)


if __name__ == "__main__":
    unittest.main()

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
        self.assertIn('pub use windows::WinInetUpdateTransport;', updater_lib)
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
        ):
            self.assertIn(expected, updater)
        windows_sys = updater_manifest["target"]["cfg(target_os = \"windows\")"][
            "dependencies"
        ]["windows-sys"]
        self.assertIn("Win32_Networking_WinInet", windows_sys["features"])
        self.assertIn("UpdateChecker::new(WinInetUpdateTransport, current_platform(), false)", updater_service)
        for expected in (
            "update_service: updater_service::UpdateService",
            "self.update_service.prepare()",
            "self.update_service.start_check(true)",
            "self.update_service.request_apply()",
            "self.update_service.open_latest_release()",
            "self.update_service.shutdown()",
        ):
            self.assertIn(expected, main)

    def test_rewrite_workflow_has_isolated_unsigned_windows_rust_validation(self) -> None:
        workflow = (ROOT / ".github" / "workflows" / "rust-rewrite.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn("windows-rust-app:", workflow)
        self.assertIn("runs-on: windows-latest", workflow)
        self.assertIn(
            "cargo test --manifest-path rust/Cargo.toml -p smu-app -p smu-updater",
            workflow,
        )
        self.assertIn(
            "cargo clippy --manifest-path rust/Cargo.toml -p smu-app -p smu-updater --all-targets -- -D warnings",
            workflow,
        )
        self.assertIn("scripts/stage_rust_windows_app.py", workflow)
        self.assertIn("scripts/package_rust_windows_bundle.py", workflow)
        self.assertIn("scripts/generate_update_manifest.py", workflow)
        self.assertIn("--only-platform windows-x64", workflow)
        self.assertIn("rust-windows-app-unsigned", workflow)
        self.assertIn("rust-windows-release-contract-unsigned", workflow)
        self.assertNotIn("SignPath", workflow)


if __name__ == "__main__":
    unittest.main()

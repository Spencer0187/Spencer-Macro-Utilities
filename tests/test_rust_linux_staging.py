import importlib.util
from pathlib import Path
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "stage_rust_linux_app.py"
SPEC = importlib.util.spec_from_file_location("stage_rust_linux_app", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class RustLinuxStagingTest(unittest.TestCase):
    def test_default_cmake_build_prefers_rust_linux_app(self) -> None:
        cmake = (ROOT / "CMakeLists.txt").read_text(encoding="utf-8")
        self.assertIn(
            'option(SMU_BUILD_CPP_LINUX_COMPAT_APP "Build the legacy C++ Linux GUI compatibility app alongside Rust package builds." OFF)',
            cmake,
        )
        self.assertIn(
            "if(SMU_PACKAGE_RUST_LINUX AND NOT SMU_BUILD_CPP_LINUX_COMPAT_APP)\n"
            "        set(SMU_BUILD_CPP_LINUX_APP OFF)",
            cmake,
        )
        self.assertEqual(cmake.count("add_custom_target(smu_rust_linux_app ALL"), 2)
        self.assertIn("if(SMU_BUILD_CPP_LINUX_APP)\n        add_executable(spencer_macro_utilities", cmake)
        self.assertIn("add_library(lua_static STATIC EXCLUDE_FROM_ALL", cmake)
        self.assertIn("add_library(smu_platform_linux STATIC EXCLUDE_FROM_ALL", cmake)

    def test_nix_package_builds_rust_offline_and_keeps_both_helpers(self) -> None:
        package = (ROOT / "nix" / "package.nix").read_text(encoding="utf-8")
        self.assertIn("rustPlatform.buildRustPackage {", package)
        self.assertIn("cargoLock.lockFile = ../rust/Cargo.lock;", package)
        self.assertIn('cargoRoot = "rust";', package)
        self.assertIn('buildAndTestSubdir = "rust";', package)
        self.assertIn("rustPlatform.bindgenHook", package)
        self.assertIn("GOPROXY=off", package)
        self.assertIn("GOSUMDB=off", package)
        self.assertIn("go test -mod=vendor ./...", package)
        self.assertIn('install -Dm755 "$helperOut/nethelper" "$appDir/nethelper-unwrapped"', package)
        self.assertIn('install -Dm755 "$helperOut/processhelper" "$appDir/processhelper"', package)
        self.assertNotIn("SMU_PACKAGE_RUST_LINUX=OFF", package)
        self.assertNotIn("libei", package)

    def test_nix_package_stages_exact_cargo_sdl_shared_library(self) -> None:
        package = (ROOT / "nix" / "package.nix").read_text(encoding="utf-8")
        self.assertIn(
            "find \"$root\" -path '*/release/build/sdl3-sys-*/out/lib64/libSDL3.so.0' -type l -print",
            package,
        )
        self.assertIn('install -Dm755 "$helperOut/$sdlName" "$out/lib/$sdlName"', package)
        self.assertIn('ln -s "$sdlName" "$out/lib/libSDL3.so.0"', package)
        self.assertIn('ln -s "$sdlName" "$out/lib/libSDL3.so"', package)
        self.assertIn('--prefix LD_LIBRARY_PATH : "$out/lib:${runtimeLibraryPath}"', package)

    def test_nix_cargo_lock_has_no_git_dependencies(self) -> None:
        lock = tomllib.loads((ROOT / "rust" / "Cargo.lock").read_text(encoding="utf-8"))
        sources = [package.get("source", "") for package in lock["package"]]
        self.assertFalse(any(source.startswith("git+") for source in sources))
        self.assertTrue(any(source.startswith("registry+") for source in sources))

    def test_smu_app_script_composition_modules_are_not_linux_cfg_gated(self) -> None:
        main = (ROOT / "rust" / "crates" / "smu-app" / "src" / "main.rs").read_text(
            encoding="utf-8"
        )
        self.assertIn("mod script_app_service;", main)
        self.assertIn("mod script_profile_bridge;", main)
        self.assertIn("mod script_ui;", main)
        self.assertNotIn(
            '#[cfg(target_os = "linux")]\nmod script_ui;',
            main,
        )
        self.assertNotIn(
            '#[cfg(target_os = "linux")]\nmod script_app_service;',
            main,
        )
        self.assertNotIn(
            '#[cfg(target_os = "linux")]\nmod script_profile_bridge;',
            main,
        )

    def test_smu_process_declares_windows_process_backend_with_locked_windows_sys(self) -> None:
        workspace = tomllib.loads((ROOT / "rust" / "Cargo.toml").read_text(encoding="utf-8"))
        manifest = tomllib.loads(
            (ROOT / "rust" / "crates" / "smu-process" / "Cargo.toml").read_text(
                encoding="utf-8"
            )
        )
        process_lib = (
            ROOT / "rust" / "crates" / "smu-process" / "src" / "lib.rs"
        ).read_text(encoding="utf-8")
        windows_backend = (
            ROOT / "rust" / "crates" / "smu-process" / "src" / "windows.rs"
        ).read_text(encoding="utf-8")

        self.assertEqual(workspace["workspace"]["dependencies"]["windows-sys"], "=0.61.2")
        windows_dependencies = manifest["target"]["cfg(target_os = \"windows\")"]["dependencies"]
        self.assertIn("windows-sys", windows_dependencies)
        self.assertIn('#[cfg(target_os = "windows")]\npub mod windows;', process_lib)
        self.assertIn("CreateToolhelp32Snapshot", windows_backend)
        self.assertIn("NtSuspendProcess", windows_backend)
        self.assertIn("NtResumeProcess", windows_backend)
        self.assertIn("GetForegroundWindow", windows_backend)

    def test_smu_app_generic_loop_owns_script_services_on_all_platforms(self) -> None:
        main = (ROOT / "rust" / "crates" / "smu-app" / "src" / "main.rs").read_text(
            encoding="utf-8"
        )
        shell = (ROOT / "rust" / "crates" / "smu-app" / "src" / "shell.rs").read_text(
            encoding="utf-8"
        )
        self.assertIn("script_app: &'a ScriptAppServices,", main)
        self.assertIn("fn start_portable_script_app(", main)
        self.assertIn("script_app.prepare_ui(shell_state.script_ui_mut());", main)
        self.assertIn("script_app.set_process_target(script_selector, script_take_all);", main)
        self.assertIn("script_app.handle_action(script_action, shell_state.script_ui_mut(), window)", main)
        self.assertIn("script_app.shutdown();", main)
        self.assertIn("smu_process::windows::WindowsProcessBackend::new()", main)
        self.assertIn("process_discovery: Some(process_discovery)", main)
        self.assertIn("process_control: Some(process_control)", main)
        self.assertNotIn(
            '#[cfg(target_os = "linux")]\n    Script(ScriptAction),',
            shell,
        )
        self.assertNotIn(
            '#[cfg(target_os = "linux")]\n    script_ui: ScriptUiState,',
            shell,
        )
        self.assertNotIn(
            '#[cfg(target_os = "linux")]\n            script_ui: ScriptUiState::default(),',
            shell,
        )
        self.assertNotIn(
            '#[cfg(target_os = "linux")]\n    #[must_use]\n    pub fn script_runtime_settings',
            shell,
        )

    def test_windows_script_hotkeys_use_native_key_state_and_process_foreground(self) -> None:
        input_manifest = tomllib.loads(
            (ROOT / "rust" / "crates" / "smu-input" / "Cargo.toml").read_text(encoding="utf-8")
        )
        windows_dependencies = input_manifest["target"]["cfg(target_os = \"windows\")"][
            "dependencies"
        ]
        self.assertIn("windows-sys", windows_dependencies)

        input_backend = (
            ROOT / "rust" / "crates" / "smu-input" / "src" / "windows.rs"
        ).read_text(encoding="utf-8")
        self.assertIn("GetAsyncKeyState", input_backend)
        self.assertIn("SendInput", input_backend)
        self.assertIn("GUI_INPUT_BUDGET", input_backend)
        self.assertIn("GetAncestor", input_backend)
        self.assertIn("set_display_scale_percent", input_backend)
        self.assertIn("set_gui_window_handle", input_backend)
        self.assertIn("impl KeyState for WindowsKeyState", input_backend)
        self.assertIn("impl InputInjector for WindowsKeyState", input_backend)
        self.assertIn("fn move_mouse_absolute", input_backend)
        self.assertIn("MOUSEEVENTF_ABSOLUTE", input_backend)
        self.assertIn("MOUSEEVENTF_VIRTUALDESK", input_backend)

        display_manifest = tomllib.loads(
            (ROOT / "rust" / "crates" / "smu-display" / "Cargo.toml").read_text(encoding="utf-8")
        )
        display_dependencies = display_manifest["target"]["cfg(target_os = \"windows\")"][
            "dependencies"
        ]
        self.assertIn("windows-sys", display_dependencies)
        display_backend = (
            ROOT / "rust" / "crates" / "smu-display" / "src" / "windows.rs"
        ).read_text(encoding="utf-8")
        self.assertIn("CreateDIBSection", display_backend)
        self.assertIn("BitBlt", display_backend)
        self.assertIn("GetMonitorInfoA", display_backend)
        self.assertIn("GetCursorPos", display_backend)
        self.assertIn("impl DisplayCapture for WindowsDisplayCapture", display_backend)

        main = (ROOT / "rust" / "crates" / "smu-app" / "src" / "main.rs").read_text(
            encoding="utf-8"
        )
        runtime = (
            ROOT / "rust" / "crates" / "smu-app" / "src" / "script_runtime.rs"
        ).read_text(encoding="utf-8")
        target_runtime = (
            ROOT / "rust" / "crates" / "smu-app" / "src" / "target_runtime.rs"
        ).read_text(encoding="utf-8")
        self.assertIn("fn start_portable_script_runtime(", main)
        self.assertIn("smu_input::windows::WindowsKeyState::new()", main)
        self.assertIn("input: Some(input)", main)
        self.assertIn("smu_display::windows::WindowsDisplayCapture::new()", main)
        self.assertIn("display: Some(display)", main)
        self.assertIn("smu_process::windows::WindowsProcessBackend::new()", main)
        self.assertIn("input: Arc<dyn KeyState>", runtime)
        self.assertIn("discovery.find_all_processes(selector)", target_runtime)
        self.assertIn("discovery.find_main_process(selector)", target_runtime)
        for module in ("runtime_input", "script_runtime", "target_runtime"):
            self.assertNotIn(f'#[cfg(target_os = "linux")]\nmod {module};', main)

    def test_smu_app_generic_loop_owns_process_status_on_windows_and_linux(self) -> None:
        main = (ROOT / "rust" / "crates" / "smu-app" / "src" / "main.rs").read_text(
            encoding="utf-8"
        )
        service = (
            ROOT / "rust" / "crates" / "smu-app" / "src" / "process_status_service.rs"
        ).read_text(encoding="utf-8")

        self.assertIn("process_status: &'a ProcessStatusService,", main)
        self.assertIn("fn start_portable_process_status() -> ProcessStatusService", main)
        self.assertIn("smu_process::windows::WindowsProcessBackend::new()", main)
        self.assertIn("let process_status = linux_services.process_status();", main)
        self.assertIn("shell_state.set_process_found(process_status.found(&selector));", main)
        self.assertIn("Duration::from_secs(1)", service)
        self.assertIn("discovery.find_main_process(selector)", service)
        self.assertIn("parity_override", service)

    def test_smu_app_keeps_portable_script_profile_dependencies_outside_linux_cfg(self) -> None:
        manifest = tomllib.loads(
            (ROOT / "rust" / "crates" / "smu-app" / "Cargo.toml").read_text(encoding="utf-8")
        )
        dependencies = manifest["dependencies"]
        linux_dependencies = manifest["target"]["cfg(target_os = \"linux\")"]["dependencies"]
        for name in (
            "smu-core",
            "smu-display",
            "smu-input",
            "smu-process",
            "smu-profile",
            "smu-scripting",
        ):
            self.assertIn(name, dependencies)
        for name in ("smu-core", "smu-display", "smu-input", "smu-process", "smu-scripting"):
            self.assertNotIn(name, linux_dependencies)

    def test_rustflags_add_origin_runpath_without_dropping_existing_flags(self) -> None:
        self.assertEqual(
            MODULE.rustflags_with_origin_runpath("-C target-cpu=x86-64-v2"),
            "-C target-cpu=x86-64-v2 -C link-arg=-Wl,-rpath,$ORIGIN/lib",
        )
        self.assertEqual(
            MODULE.rustflags_with_origin_runpath(None),
            "-C link-arg=-Wl,-rpath,$ORIGIN/lib",
        )

    def test_resolve_sdl_library_requires_one_exact_cargo_build(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            out = target / "release" / "build" / "sdl3-sys-abc" / "out" / "lib64"
            out.mkdir(parents=True)
            real = out / "libSDL3.so.0.4.14"
            real.write_bytes(b"sdl")
            (out / "libSDL3.so.0").symlink_to(real.name)
            self.assertEqual(MODULE.resolve_sdl_library(target, "release"), real)

    def test_stage_runtime_renames_binary_and_creates_sdl_soname_links(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "smu-app"
            binary.write_bytes(b"rust-app")
            sdl = root / "libSDL3.so.0.4.14"
            sdl.write_bytes(b"sdl")
            stage = root / "stage"
            stale_lib = stage / "lib"
            stale_lib.mkdir(parents=True)
            (stale_lib / "libSDL3.so.0.old").write_bytes(b"old")

            staged_binary, staged_sdl = MODULE.stage_runtime(binary, sdl, stage)

            self.assertEqual(staged_binary.name, "Spencer-Macro-Utilities")
            self.assertEqual(staged_binary.read_bytes(), b"rust-app")
            self.assertTrue(staged_binary.stat().st_mode & 0o111)
            self.assertEqual(staged_sdl.read_bytes(), b"sdl")
            self.assertEqual((stage / "lib" / "libSDL3.so.0").resolve(), staged_sdl)
            self.assertEqual((stage / "lib" / "libSDL3.so").resolve(), staged_sdl)
            self.assertFalse((stage / "lib" / "libSDL3.so.0.old").exists())

    def test_ambiguous_sdl_builds_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            for suffix in ("one", "two"):
                out = target / "release" / "build" / f"sdl3-sys-{suffix}" / "out" / "lib64"
                out.mkdir(parents=True)
                real = out / f"libSDL3.so.0.4.{suffix}"
                real.write_bytes(suffix.encode())
                (out / "libSDL3.so.0").symlink_to(real.name)
            with self.assertRaises(RuntimeError):
                MODULE.resolve_sdl_library(target, "release")


if __name__ == "__main__":
    unittest.main()

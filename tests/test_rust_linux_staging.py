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
        self.assertIn("EnumWindows", windows_backend)
        self.assertIn("GetWindowRect", windows_backend)
        self.assertIn("find_newest_process_window", windows_backend)
        self.assertIn("WindowsWindowHandle", windows_backend)

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
        self.assertIn("process_discovery: Some(Arc::clone(&platform.discovery))", main)
        self.assertIn("process_control: Some(Arc::clone(&platform.control))", main)
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
        input_windows_features = windows_dependencies["windows-sys"]["features"]
        for feature in (
            "Win32_Foundation",
            "Win32_System_LibraryLoader",
            "Win32_System_Threading",
            "Win32_UI_Input_KeyboardAndMouse",
            "Win32_UI_WindowsAndMessaging",
        ):
            self.assertIn(feature, input_windows_features)

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
        self.assertIn("SetWindowsHookExW", input_backend)
        self.assertIn("WH_MOUSE_LL", input_backend)
        self.assertIn("WH_KEYBOARD_LL", input_backend)
        self.assertIn("real_keyboard_activity_serial", input_backend)
        self.assertIn("SetCursorPos", input_backend)
        self.assertIn("WM_MOUSEWHEEL", input_backend)
        self.assertIn("LLMHF_INJECTED", input_backend)
        self.assertIn("LLMHF_LOWER_IL_INJECTED", input_backend)
        self.assertIn("PENDING_BINDING_INPUT", input_backend)
        self.assertIn("fn consume_next_transient_input", input_backend)
        self.assertIn("PostThreadMessageW", input_backend)

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
        self.assertIn("input: Some(Arc::clone(&platform.input))", main)
        self.assertIn("smu_display::windows::WindowsDisplayCapture::new()", main)
        self.assertIn("display: Some(Arc::clone(&platform.display))", main)
        self.assertIn("smu_process::windows::WindowsProcessBackend::new()", main)
        self.assertIn("Arc::clone(&platform.key_state)", main)
        self.assertIn("Arc::clone(&platform.foreground)", main)
        self.assertIn("input: Arc<dyn KeyState>", runtime)
        self.assertIn("discovery.find_all_processes(selector)", target_runtime)
        self.assertIn("discovery.find_main_process(selector)", target_runtime)
        for module in ("runtime_input", "script_runtime", "target_runtime"):
            self.assertNotIn(f'#[cfg(target_os = "linux")]\nmod {module};', main)

    def test_windows_builtin_macro_runtime_uses_shared_native_platform_services(self) -> None:
        main = (ROOT / "rust" / "crates" / "smu-app" / "src" / "main.rs").read_text(
            encoding="utf-8"
        )
        shell = (ROOT / "rust" / "crates" / "smu-app" / "src" / "shell.rs").read_text(
            encoding="utf-8"
        )

        for expected in (
            "struct WindowsMacroPlatformServices",
            "struct WindowsBuiltinRuntimeServices",
            "WindowsBuiltinRuntimeServices::new(",
            "&windows_platform,",
            "ProcessStatusService::new(Some(Arc::clone(&windows_platform.discovery)), None)",
            "windows_builtin_runtime.prepare_shell(shell_state);",
            "windows_builtin_runtime.finish_shell(shell_action.as_ref(), shell_state);",
            "builtin_runtime.shutdown();",
            "start_portable_script_runtime(script_app, &shell_state, &windows_platform)",
        ):
            self.assertIn(expected, main)
        for controller in (
            "BunnyhopController::start",
            "HhjController::start",
            "FloorBounceController::start",
            "FreezeController::start",
            "ItemDesyncController::start",
            "ItemClipController::start",
            "ItemUnequipController::start",
            "LaughClipController::start",
            "PressKeyController::start",
            "SpamKeyController::start",
            "LedgeBounceController::start",
            "SpeedglitchController::start",
            "WallWalkController::start",
            "WallessLhjController::start",
            "WallhopController::start",
        ):
            self.assertIn(controller, main)
        self.assertNotIn('#[cfg(target_os = "linux")]\nmod bunnyhop_runtime;', main)
        self.assertIn(
            '#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]\n    #[must_use]\n    pub fn bunnyhop_controller_settings',
            shell,
        )

    def test_windows_anti_afk_runtime_matches_cpp_hook_timer_and_window_contract(self) -> None:
        main = (ROOT / "rust" / "crates" / "smu-app" / "src" / "main.rs").read_text(
            encoding="utf-8"
        )
        shell = (ROOT / "rust" / "crates" / "smu-app" / "src" / "shell.rs").read_text(
            encoding="utf-8"
        )
        runtime = (
            ROOT / "rust" / "crates" / "smu-app" / "src" / "anti_afk_runtime.rs"
        ).read_text(encoding="utf-8")
        input_backend = (
            ROOT / "rust" / "crates" / "smu-input" / "src" / "windows.rs"
        ).read_text(encoding="utf-8")
        process_backend = (
            ROOT / "rust" / "crates" / "smu-process" / "src" / "windows.rs"
        ).read_text(encoding="utf-8")

        for expected in (
            'mod anti_afk_runtime;',
            "AntiAfkController::start",
            "anti_afk_controller.update_settings",
            "anti_afk_controller.stop();",
            "keyboard_activity_hook_ready()",
            "Anti-AFK may not reset on physical keyboard activity.",
        ):
            self.assertIn(expected, main)
        global_settings = (
            ROOT / "rust" / "crates" / "smu-app" / "src" / "global_settings.rs"
        ).read_text(encoding="utf-8")
        self.assertIn("pub fn anti_afk_controller_settings", shell)
        self.assertIn("self.keybind_capture.runtime_allowed()", shell)
        self.assertIn('anti_afk_enabled: cfg!(target_os = "windows")', global_settings)
        self.assertNotIn("macro_enabled()", shell.split("pub fn anti_afk_controller_settings", 1)[1].split("pub fn freeze_controller_settings", 1)[0])
        for expected in (
            "evaluate_anti_afk_timer_step",
            "real_keyboard_activity_serial",
            "find_newest_process_window",
            "foreground_window",
            "BACKGROUND_FOCUS_SETTLE_MS",
            "ORIGINAL_FOREGROUND_SETTLE_MS",
            "hold_key_chord",
            "set_cursor_position_direct",
            "set_left_mouse_button_direct",
        ):
            self.assertIn(expected, runtime)
        for expected in (
            "WH_KEYBOARD_LL",
            "KBDLLHOOKSTRUCT",
            "REAL_KEYBOARD_ACTIVITY_SERIAL",
            "KEYBOARD_HOOK_READY",
            "keyboard_activity_hook_ready",
            "LLKHF_INJECTED",
            "LLKHF_LOWER_IL_INJECTED",
        ):
            self.assertIn(expected, input_backend)
        for expected in (
            "EnumWindows",
            "IsWindowVisible",
            "GW_OWNER",
            "GetWindowRect",
            "SetWindowPos",
            "SetForegroundWindow",
        ):
            self.assertIn(expected, process_backend)

    def test_smu_network_freezes_cpp_windivert_filter_grammar(self) -> None:
        network = (
            ROOT / "rust" / "crates" / "smu-network" / "src" / "lib.rs"
        ).read_text(encoding="utf-8")
        self.assertIn("pub fn build_windows_windivert_filter(", network)
        self.assertIn("WINDOWS_ROBLOX_RANGE_FILTER", network)
        self.assertIn("udp.SrcPort", network)
        self.assertIn("udp.DstPort", network)
        self.assertIn("ip.DstAddr", network)
        self.assertIn("tcp.DstPort", network)
        self.assertIn("LagSwitchTargetMode::Roblox", network)
        self.assertIn("LagSwitchTargetMode::Custom", network)
        self.assertIn("LagSwitchTargetMode::All", network)

    def test_smu_network_has_native_windows_windivert_loader_foundation(self) -> None:
        manifest = tomllib.loads(
            (ROOT / "rust" / "crates" / "smu-network" / "Cargo.toml").read_text(
                encoding="utf-8"
            )
        )
        network = (
            ROOT / "rust" / "crates" / "smu-network" / "src" / "lib.rs"
        ).read_text(encoding="utf-8")
        windows = (
            ROOT / "rust" / "crates" / "smu-network" / "src" / "windows.rs"
        ).read_text(encoding="utf-8")

        windows_dependencies = manifest["target"]["cfg(target_os = \"windows\")"]["dependencies"]
        self.assertEqual(
            windows_dependencies["windows-sys"]["features"],
            [
                "Win32_Foundation",
                "Win32_NetworkManagement_IpHelper",
                "Win32_Security",
                "Win32_System_LibraryLoader",
                "Win32_System_SystemServices",
            ],
        )
        self.assertIn("smu-process", windows_dependencies)
        self.assertIn('#[cfg(target_os = "windows")]\npub mod windows;', network)
        self.assertIn(
            '#[cfg(any(target_os = "windows", test))]\npub(crate) mod windows_delay;', network
        )
        self.assertIn('WINDIVERT_DLL_NAME: &str = "SMCWinDivert.dll"', windows)
        self.assertIn('WINDIVERT_DRIVER_NAME: &str = "SMCWinDivert64.sys"', windows)
        self.assertIn("CheckTokenMembership", windows)
        self.assertIn("LagSwitchInitializationFailure::RequiresAdministrator", windows)
        self.assertIn("pub struct WinDivertAddress", windows)
        delayed = (
            ROOT / "rust" / "crates" / "smu-network" / "src" / "windows_delay.rs"
        ).read_text(encoding="utf-8")
        self.assertIn("pub struct WindowsDelayedSender", delayed)
        self.assertIn("pub fn start", delayed)
        self.assertIn("pub fn enqueue", delayed)
        self.assertIn("pub fn stop", delayed)
        self.assertIn("pub struct SharedWinDivertHandle", windows)
        self.assertIn("pub struct WinDivertDelayedPacketSender", windows)
        self.assertIn("open_filter_shared", windows)
        self.assertIn("Arc<SharedWinDivertHandle>", windows)
        self.assertIn("Arc<WinDivertHandleSlot>", windows)
        self.assertIn("WindowsDelayedSender<DelayedWinDivertPacket>", windows)
        self.assertIn("let current = lock_mutex(&self.current);", windows)
        self.assertIn("pub(crate) struct WinDivertPacketWorker", windows)
        self.assertIn("fn packet_worker_loop(", windows)
        self.assertIn("let current = lock_mutex(&shared.capture);", windows)
        self.assertIn("shared.handle_slot.install(Arc::clone(&handle));", windows)
        self.assertIn("fn run_capture_handle(", windows)
        self.assertIn("windows_packet_decision(", windows)
        self.assertIn("WindowsPacketAction::DelayMs", windows)
        self.assertIn("handle_slot.shutdown_current()", windows)
        self.assertIn("pub struct WindowsLagSwitchBackend", windows)
        self.assertIn("impl LagSwitchControl for WindowsLagSwitchBackend", windows)
        self.assertIn("GetExtendedUdpTable", windows)
        self.assertIn("DYNAMIC_TARGET_REFRESH", windows)
        self.assertIn("set_process_target", windows)
        self.assertIn("invalid_dynamic_ips_reset", windows)
        self.assertIn("ERROR_INVALID_PARAMETER", windows)
        self.assertIn("WindowsDynamicTargetState", network)
        self.assertIn("assert_eq!(std::mem::size_of::<WinDivertAddress>(), 80);", windows)
        for export in (
            "WinDivertOpen",
            "WinDivertRecv",
            "WinDivertSend",
            "WinDivertHelperParsePacket",
            "WinDivertShutdown",
            "WinDivertClose",
        ):
            self.assertIn(export, windows)
        self.assertIn('b"false\\0".as_ptr()', windows)
        self.assertIn("WINDIVERT_FLAG_SNIFF", windows)

    def test_lag_switch_controller_uses_shared_network_contract(self) -> None:
        runtime = (
            ROOT / "rust" / "crates" / "smu-app" / "src" / "lag_switch_runtime.rs"
        ).read_text(encoding="utf-8")
        network = (
            ROOT / "rust" / "crates" / "smu-network" / "src" / "lib.rs"
        ).read_text(encoding="utf-8")

        self.assertIn("Arc<dyn LagSwitchControl>", runtime)
        self.assertNotIn("LinuxLagSwitchBackend", runtime)
        self.assertNotIn("LagSwitchBackendPort", runtime)
        for method in (
            "fn initialize(&self) -> bool;",
            "fn set_base_blocking_active(&self, active: bool);",
            "fn set_base_config(&self, config: LagSwitchConfig);",
        ):
            self.assertIn(method, network)

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

    def test_smu_app_keeps_portable_dependencies_global_and_network_runtime_platform_scoped(self) -> None:
        manifest = tomllib.loads(
            (ROOT / "rust" / "crates" / "smu-app" / "Cargo.toml").read_text(encoding="utf-8")
        )
        dependencies = manifest["dependencies"]
        targets = manifest["target"]
        linux_dependencies = targets["cfg(target_os = \"linux\")"]["dependencies"]
        windows_dependencies = targets["cfg(target_os = \"windows\")"]["dependencies"]
        for name in (
            "smu-core",
            "smu-display",
            "smu-input",
            "smu-process",
            "smu-profile",
            "smu-scripting",
        ):
            self.assertIn(name, dependencies)
            self.assertNotIn(name, linux_dependencies)
            self.assertNotIn(name, windows_dependencies)
        for name in ("smu-network", "smu-roblox", "smu-runtime"):
            self.assertNotIn(name, dependencies)
            self.assertIn(name, linux_dependencies)
            self.assertIn(name, windows_dependencies)

    def test_windows_lag_switch_app_wiring_shares_native_backend_and_tracks_windows_init_flag(self) -> None:
        main = (ROOT / "rust" / "crates" / "smu-app" / "src" / "main.rs").read_text(
            encoding="utf-8"
        )
        lag_switch = (
            ROOT / "rust" / "crates" / "smu-app" / "src" / "lag_switch.rs"
        ).read_text(encoding="utf-8")
        runtime = (
            ROOT / "rust" / "crates" / "smu-app" / "src" / "lag_switch_runtime.rs"
        ).read_text(encoding="utf-8")
        script_logging = (
            ROOT / "rust" / "crates" / "smu-app" / "src" / "script_logging.rs"
        ).read_text(encoding="utf-8")

        for expected in (
            "struct WindowsLagSwitchControlAdapter",
            "struct WindowsLagSwitchServices",
            "WindowsLagSwitchBackend::new",
            "self.enabled.store(true, Ordering::Release);",
            "Arc::clone(&self.control)",
            "Some(lag_switch.script_network())",
            "roblox_log_factory: Some(roblox_log_source_factory())",
            "logger: Some(script_log_sink())",
            "windows_lag_switch.prepare_shell(shell_state);",
            "windows_lag_switch.finish_shell(shell_action.as_ref(), shell_state);",
            "lag_switch.shutdown();",
            "self.backend.set_process_target(selector, take_all_process_ids);",
        ):
            self.assertIn(expected, main)
        self.assertNotIn("Some(lag_switch.roblox_log())", main)
        self.assertNotIn("fn roblox_log(&self)", main)
        self.assertEqual(main.count("logger: Some(script_log_sink())"), 3)
        for expected in (
            "MAX_LOG_ENTRIES: usize = 512",
            '"DEBUG"',
            '"SMU_DEBUG"',
            '"SMC.log"',
            "current_exe()",
            "localtime_r",
            "GetLocalTime",
            "OutputDebugStringA",
            "pub fn drain_warnings() -> Vec<String>",
            "fn log_warning(&self, message: &[u8])",
        ):
            self.assertIn(expected, script_logging)
        self.assertIn("for warning in script_logging::drain_warnings()", main)
        self.assertIn("shell_state.push_warning(warning);", main)
        self.assertIn(
            '#[cfg(any(target_os = "linux", target_os = "windows"))]\n    #[must_use]\n    pub fn controller_settings(',
            lag_switch,
        )
        self.assertIn("let prevent_disconnect = self.prevent_disconnect;", lag_switch)
        self.assertIn("let prevent_disconnect = false;", lag_switch)
        self.assertNotIn("backend_enabled.store(initialized, Ordering::Release);", runtime)

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

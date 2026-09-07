from __future__ import annotations

import pathlib
import tomllib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]


class RustMacosAppCompositionTests(unittest.TestCase):
    def test_composed_input_device_owns_state_and_injector_lifecycle(self) -> None:
        source = (
            ROOT / "rust" / "crates" / "smu-input" / "src" / "macos.rs"
        ).read_text(encoding="utf-8")
        for expected in (
            "pub struct MacosInputDevice",
            "state: MacosInputState",
            "injector: MacosInputInjector",
            "self.state.init()?",
            "if let Err(error) = self.injector.init()",
            "self.state.shutdown()",
            "self.injector.shutdown()",
            "impl KeyState for MacosInputDevice",
            "impl InputInjector for MacosInputDevice",
        ):
            self.assertIn(expected, source)

    def test_app_composition_gates_live_input_on_both_macos_privacy_permissions(self) -> None:
        source = (
            ROOT / "rust" / "crates" / "smu-app" / "src" / "main.rs"
        ).read_text(encoding="utf-8")
        for expected in (
            "struct MacosPlatformServices",
            "Arc<MacosInputDevice>",
            "Arc<MacosDisplayCapture>",
            "Arc<MacosProcessService>",
            "let accessibility_ready = accessibility_trusted()",
            "let screen_recording_ready = screen_recording_trusted()",
            "if accessibility_ready && screen_recording_ready",
            "match input.init()",
            'missing.push("Accessibility")',
            'missing.push("Screen Recording")',
            "macOS macro input is disabled until",
            "fn start_macos_app_services",
            "fn start_macos_script_app",
            "fn start_macos_script_runtime",
            "if !platform.input_ready",
            "fn prepare_shell(&self, shell_state: &mut shell::ShellState)",
            "keybind::InputSnapshot::capture",
            "macos_platform.prepare_shell(shell_state)",
            "fn shutdown_macos_services",
            "platform.shutdown()",
        ):
            self.assertIn(expected, source)

    def test_macos_scripts_get_native_process_display_and_only_ready_input(self) -> None:
        source = (
            ROOT / "rust" / "crates" / "smu-app" / "src" / "main.rs"
        ).read_text(encoding="utf-8")
        for expected in (
            "let input: Option<Arc<dyn smu_input::InputDevice>> = if platform.input_ready",
            "display: Some(display)",
            "process_discovery: Some(discovery)",
            "process_control: Some(control)",
            "let input: Arc<dyn smu_input::KeyState> = platform.input.clone()",
            "let foreground: Arc<dyn smu_process::ProcessForeground> = platform.process.clone()",
            "ProcessStatusService::new(Some(discovery), None)",
        ):
            self.assertIn(expected, source)
        self.assertNotIn("MacosLagSwitch", source)

    def test_macos_builtin_runtime_matches_cpp_non_network_controller_set(self) -> None:
        main = (ROOT / "rust" / "crates" / "smu-app" / "src" / "main.rs").read_text(
            encoding="utf-8"
        )
        self.assertIn("struct MacosBuiltinRuntimeServices", main)
        self.assertIn("if !platform.input_ready", main)
        for controller in (
            "BunnyhopController",
            "HhjController",
            "FloorBounceController",
            "FreezeController",
            "ItemDesyncController",
            "ItemClipController",
            "ItemUnequipController",
            "LaughClipController",
            "PressKeyController",
            "SpamKeyController",
            "LedgeBounceController",
            "SpeedglitchController",
            "WallWalkController",
            "WallessLhjController",
            "WallhopController",
        ):
            self.assertIn(controller, main)
        macos_block = main.split("struct MacosBuiltinRuntimeServices", 1)[1].split(
            '#[cfg(target_os = "windows")]', 1
        )[0]
        self.assertNotIn("AntiAfkController", macos_block)
        self.assertNotIn("LagSwitchController", macos_block)
        self.assertIn("macos_builtin_runtime: Option<&'a MacosBuiltinRuntimeServices>", main)
        self.assertIn("runtime.finish_shell(shell_state)", main)
        self.assertIn("macos_builtin_runtime.as_ref()", main)
        self.assertIn("runtime.shutdown()", main)

        shell = (ROOT / "rust" / "crates" / "smu-app" / "src" / "shell.rs").read_text(
            encoding="utf-8"
        )
        macos_cfg = '#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]'
        for settings_fn in (
            "freeze_controller_settings",
            "floor_bounce_controller_settings",
            "hhj_controller_settings",
            "item_unequip_controller_settings",
            "item_desync_controller_settings",
            "item_clip_controller_settings",
            "laugh_clip_controller_settings",
            "spam_key_controller_settings",
            "bunnyhop_controller_settings",
            "ledge_bounce_controller_settings",
            "speedglitch_controller_settings",
            "press_key_controller_settings",
            "wallhop_controller_settings",
            "walless_lhj_controller_settings",
            "wall_walk_controller_settings",
        ):
            marker = f"{macos_cfg}\n    #[must_use]\n    pub fn {settings_fn}"
            self.assertIn(marker, shell)
        self.assertNotIn(
            f"{macos_cfg}\n    #[must_use]\n    pub fn lag_switch_controller_settings",
            shell,
        )

    def test_macos_app_keeps_network_backend_and_windows_only_runtime_unavailable(self) -> None:
        manifest = tomllib.loads(
            (ROOT / "rust" / "crates" / "smu-app" / "Cargo.toml").read_text(
                encoding="utf-8"
            )
        )
        macos = manifest.get("target", {}).get('cfg(target_os = "macos")', {})
        dependencies = macos.get("dependencies", {})
        self.assertIn("smu-network", dependencies)
        self.assertIn("smu-runtime", dependencies)
        self.assertIn("smu-updater", dependencies)
        self.assertNotIn("smu-roblox", dependencies)

        main = (ROOT / "rust" / "crates" / "smu-app" / "src" / "main.rs").read_text(
            encoding="utf-8"
        )
        self.assertNotIn("MacosLagSwitch", main)
        self.assertNotIn("start_lag_switch_controller(&macos", main)
        self.assertNotIn("WindowsBuiltinRuntimeServices::new(&shell_state, &macos", main)

    def test_rewrite_workflow_compiles_full_macos_app_composition(self) -> None:
        workflow = (ROOT / ".github" / "workflows" / "rust-rewrite.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn("tests.test_rust_macos_app", workflow)
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

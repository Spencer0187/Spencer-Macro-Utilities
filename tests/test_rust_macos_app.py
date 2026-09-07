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

    def test_macos_app_compiles_shared_runtime_models_without_starting_native_backends(self) -> None:
        manifest = tomllib.loads(
            (ROOT / "rust" / "crates" / "smu-app" / "Cargo.toml").read_text(
                encoding="utf-8"
            )
        )
        macos = manifest.get("target", {}).get('cfg(target_os = "macos")', {})
        dependencies = macos.get("dependencies", {})
        self.assertIn("smu-network", dependencies)
        self.assertIn("smu-runtime", dependencies)
        self.assertNotIn("smu-updater", dependencies)
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
            "cargo test -p smu-process -p smu-input -p smu-display -p smu-app",
            workflow,
        )
        self.assertIn(
            "cargo clippy -p smu-process -p smu-input -p smu-display -p smu-app --all-targets -- -D warnings",
            workflow,
        )


if __name__ == "__main__":
    unittest.main()

from __future__ import annotations

import pathlib
import tomllib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]


class RustMacosDisplayTests(unittest.TestCase):
    def test_manifest_keeps_coregraphics_display_dependencies_target_scoped(self) -> None:
        manifest = tomllib.loads(
            (ROOT / "rust" / "crates" / "smu-display" / "Cargo.toml").read_text(
                encoding="utf-8"
            )
        )
        dependencies = manifest["target"]['cfg(target_os = "macos")']["dependencies"]
        self.assertIn("objc2-application-services", dependencies)
        self.assertIn("objc2-core-foundation", dependencies)
        self.assertIn("objc2-core-graphics", dependencies)
        features = dependencies["objc2-core-graphics"]["features"]
        for feature in (
            "CGBitmapContext",
            "CGColorSpace",
            "CGContext",
            "CGDirectDisplay",
            "CGError",
            "CGImage",
            "CGWindow",
        ):
            self.assertIn(feature, features)

    def test_native_display_backend_preserves_cpp_coregraphics_contract(self) -> None:
        source = (
            ROOT / "rust" / "crates" / "smu-display" / "src" / "macos.rs"
        ).read_text(encoding="utf-8")
        for expected in (
            "pub struct MacosDisplayCapture",
            "impl DisplayCapture for MacosDisplayCapture",
            "CGGetActiveDisplayList",
            "CGDisplayBounds",
            "CGEvent::location",
            "macos_display_index_for_point",
            "macos_global_screen_bounds",
            "CGDisplayCopyDisplayMode",
            "CGDisplayMode::refresh_rate",
            "Duration::from_millis(16)",
            "CGPreflightScreenCaptureAccess",
            "CGRequestScreenCaptureAccess",
            "CGDisplayCreateImage",
            "CGImage::with_image_in_rect",
            "CGImage::width",
            "CGImage::height",
            "CGColorSpace::new_device_rgb",
            "CGBitmapContextCreate",
            "CGImageAlphaInfo::PremultipliedLast",
            "CGImageByteOrderInfo::Order32Big",
            "CGContext::draw_image",
            "macOS Screen Recording permission is required before SMU can read screen pixels.",
        ):
            self.assertIn(expected, source)

    def test_accessibility_script_hook_is_not_silently_ready(self) -> None:
        source = (
            ROOT / "rust" / "crates" / "smu-display" / "src" / "macos.rs"
        ).read_text(encoding="utf-8")
        for expected in (
            "AXIsProcessTrusted",
            "AXIsProcessTrustedWithOptions",
            "kAXTrustedCheckOptionPrompt",
            "CFDictionary::<CFString, CFBoolean>::from_slices",
            "options.as_opaque()",
            "request_absolute_pointer_access_for_script",
            "macOS Accessibility permission is required to move the global cursor.",
        ):
            self.assertIn(expected, source)

    def test_adapter_is_wired_only_through_macos_composition(self) -> None:
        main = (ROOT / "rust" / "crates" / "smu-app" / "src" / "main.rs").read_text(
            encoding="utf-8"
        )
        self.assertIn("use smu_display::macos::{MacosDisplayCapture, screen_recording_trusted};", main)
        self.assertIn("display: Arc<MacosDisplayCapture>", main)
        self.assertIn("let display: Arc<dyn smu_display::DisplayCapture> = platform.display.clone()", main)

    def test_rewrite_workflow_runs_native_macos_display_gate(self) -> None:
        workflow = (ROOT / ".github" / "workflows" / "rust-rewrite.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn("macos-native-adapters:", workflow)
        self.assertIn("runs-on: macos-latest", workflow)
        self.assertIn(
            "cargo test -p smu-process -p smu-input -p smu-display -p smu-updater -p smu-app", workflow
        )
        self.assertIn(
            "cargo clippy -p smu-process -p smu-input -p smu-display -p smu-updater -p smu-app --all-targets -- -D warnings",
            workflow,
        )


if __name__ == "__main__":
    unittest.main()

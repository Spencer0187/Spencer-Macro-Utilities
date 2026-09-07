from __future__ import annotations

import pathlib
import re
import tomllib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]


class RustMacosInputTests(unittest.TestCase):
    def test_manifest_keeps_coregraphics_dependencies_target_scoped(self) -> None:
        manifest = tomllib.loads(
            (ROOT / "rust" / "crates" / "smu-input" / "Cargo.toml").read_text(
                encoding="utf-8"
            )
        )
        dependencies = manifest["target"]['cfg(target_os = "macos")']["dependencies"]
        self.assertIn("objc2-application-services", dependencies)
        self.assertIn("objc2-core-foundation", dependencies)
        self.assertIn("objc2-core-graphics", dependencies)

    def test_virtual_key_map_matches_cpp_table(self) -> None:
        cpp = (ROOT / "core" / "keymapping.h").read_text(encoding="utf-8")
        cpp_block = cpp.split(
            "inline uint16_t win_vkey_to_macos_key(uint8_t win_vkey_code) {", 1
        )[1].split("\n}", 1)[0]
        cpp_pairs = {
            int(windows, 16): int(macos, 16)
            for windows, macos in re.findall(
                r"case (0x[0-9A-Fa-f]+): return (0x[0-9A-Fa-f]+);", cpp_block
            )
        }

        rust = (ROOT / "rust" / "crates" / "smu-input" / "src" / "lib.rs").read_text(
            encoding="utf-8"
        )
        rust_block = rust.split("const MACOS_KEY_MAP: &[(u32, u16)] = &[", 1)[1].split(
            "];", 1
        )[0]
        rust_pairs = {
            int(windows, 16): int(macos, 16)
            for windows, macos in re.findall(
                r"\((0x[0-9A-Fa-f]+), (0x[0-9A-Fa-f]+)\)", rust_block
            )
        }
        self.assertEqual(rust_pairs, cpp_pairs)

    def test_coregraphics_output_preserves_cpp_contract(self) -> None:
        source = (ROOT / "rust" / "crates" / "smu-input" / "src" / "macos.rs").read_text(
            encoding="utf-8"
        )
        for expected in (
            "AXIsProcessTrusted",
            "initialized.store(true, Ordering::Release)",
            "self.initialized.load(Ordering::Acquire) && accessibility_trusted()",
            "0x534D_554D_4143_4F53",
            "CGEventField::EventSourceUserData",
            "CGEventTapLocation::HIDEventTap",
            "CGEvent::new_keyboard_event",
            "CGEvent::new_mouse_event",
            "CGEventType::OtherMouseDown",
            "CGEventField::MouseEventButtonNumber",
            "CGEventField::MouseEventDeltaX",
            "CGEventField::MouseEventDeltaY",
            "CGEvent::new_scroll_wheel_event2",
            "CGScrollEventUnit::Line",
            "set_direct_cursor_movement",
            "prefers_direct_absolute_positioning",
        ):
            self.assertIn(expected, source)
        self.assertNotIn("CGEventTapCreate", source)
        self.assertNotIn("TISCopyCurrentKeyboardLayoutInputSource", source)

    def test_adapter_remains_unwired_until_state_and_display_backends_exist(self) -> None:
        app_sources = "\n".join(
            path.read_text(encoding="utf-8")
            for path in (ROOT / "rust" / "crates" / "smu-app" / "src").glob("*.rs")
        )
        self.assertNotIn("MacosInputInjector", app_sources)

    def test_rewrite_workflow_runs_native_macos_input_gate(self) -> None:
        workflow = (ROOT / ".github" / "workflows" / "rust-rewrite.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn("runs-on: macos-latest", workflow)
        self.assertIn("cargo test -p smu-process -p smu-input", workflow)
        self.assertIn(
            "cargo clippy -p smu-process -p smu-input --all-targets -- -D warnings",
            workflow,
        )


if __name__ == "__main__":
    unittest.main()

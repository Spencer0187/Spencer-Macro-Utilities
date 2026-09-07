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

    def test_workspace_enables_corefoundation_event_tap_runloop_features(self) -> None:
        workspace = tomllib.loads((ROOT / "rust" / "Cargo.toml").read_text(encoding="utf-8"))
        core_foundation = workspace["workspace"]["dependencies"]["objc2-core-foundation"]
        self.assertFalse(core_foundation["default-features"])
        for feature in ("std", "CFData", "CFDate", "CFMachPort", "CFRunLoop"):
            self.assertIn(feature, core_foundation["features"])

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

    def test_active_layout_text_preserves_tis_uckeytranslate_contract(self) -> None:
        source = (ROOT / "rust" / "crates" / "smu-input" / "src" / "macos.rs").read_text(
            encoding="utf-8"
        )
        for expected in (
            '#[link(name = "Carbon", kind = "framework")]',
            "TISCopyCurrentKeyboardLayoutInputSource",
            "TISGetInputSourceProperty",
            "kTISPropertyUnicodeKeyLayoutData",
            "CFRetained::<CFType>::from_raw",
            "MINIMUM_UC_KEYBOARD_LAYOUT_BYTES",
            "UCKeyTranslate",
            "UC_KEY_ACTION_DOWN",
            "UC_KEY_TRANSLATE_NO_DEAD_KEYS_MASK",
            "LMGetKbdType",
            "[0, SHIFT_KEY, OPTION_KEY, SHIFT_KEY | OPTION_KEY]",
            "(modifier_state >> 8) & 0xFF",
            "output_length == 1 && output[0] == wanted",
            "MAC_KEY_OPTION",
            "MAC_KEY_SHIFT",
            "CGEvent::keyboard_set_unicode_string",
            "post_unicode_character",
            "press_character_with_active_layout",
            "fn press_character(&self, character: u8, delay_ms: i32) -> bool",
        ):
            self.assertIn(expected, source)

    def test_coregraphics_state_reader_preserves_cpp_event_tap_contract(self) -> None:
        source = (ROOT / "rust" / "crates" / "smu-input" / "src" / "macos.rs").read_text(
            encoding="utf-8"
        )
        for expected in (
            "pub struct MacosInputState",
            "impl KeyState for MacosInputState",
            "CGEvent::tap_create",
            "CGEventTapLocation::SessionEventTap",
            "CGEventTapPlacement::HeadInsertEventTap",
            "CGEventTapOptions::ListenOnly",
            "CFMachPort::new_run_loop_source",
            "kCFRunLoopCommonModes",
            "CFRunLoop::run_in_mode",
            "CGEventType::TapDisabledByTimeout",
            "CGEventType::TapDisabledByUserInput",
            "CGEvent::tap_enable",
            "CGEventField::KeyboardEventKeycode",
            "modifier_down_for_flags_changed",
            "mouse_event_key",
            "CGEventField::ScrollWheelEventDeltaAxis1",
            "KeyCode::MOUSE_WHEEL_UP",
            "KeyCode::MOUSE_WHEEL_DOWN",
            "pending_transient.swap",
            "== INJECTED_EVENT_TAG",
            'name("smu-macos-input-tap".to_owned())',
        ):
            self.assertIn(expected, source)

    def test_composed_input_facade_is_used_by_macos_app_services(self) -> None:
        main = (ROOT / "rust" / "crates" / "smu-app" / "src" / "main.rs").read_text(
            encoding="utf-8"
        )
        self.assertIn("use smu_input::macos::{MacosInputDevice, accessibility_trusted};", main)
        self.assertIn("input: Arc<MacosInputDevice>", main)
        self.assertIn("if accessibility_ready && screen_recording_ready", main)
        self.assertIn("macos_platform.prepare_shell(shell_state)", main)
        self.assertNotIn("MacosInputInjector", main)
        self.assertNotIn("MacosInputState", main)

    def test_rewrite_workflow_runs_native_macos_input_gate(self) -> None:
        workflow = (ROOT / ".github" / "workflows" / "rust-rewrite.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn("runs-on: macos-latest", workflow)
        self.assertIn("cargo test -p smu-process -p smu-input -p smu-display -p smu-updater -p smu-app", workflow)
        self.assertIn(
            "cargo clippy -p smu-process -p smu-input -p smu-display -p smu-updater -p smu-app --all-targets -- -D warnings",
            workflow,
        )


if __name__ == "__main__":
    unittest.main()

#!/usr/bin/env python3
from pathlib import Path
import re
import sys


ROOT_FILES = {
    "globals": Path("core/legacy_globals.h"),
    "ui": Path("app/app_ui.cpp"),
    "runtime": Path("app/macro_runtime.cpp"),
}


def backup(path: Path, original: str) -> None:
    bak = path.with_suffix(path.suffix + ".pre_keybind_final.bak")
    bak.write_text(original, encoding="utf-8")


def find_function_bounds(text: str, signature_start: str) -> tuple[int, int]:
    start = text.find(signature_start)
    if start == -1:
        raise RuntimeError(f"Could not find function signature: {signature_start}")

    brace = text.find("{", start)
    if brace == -1:
        raise RuntimeError(f"Could not find opening brace for: {signature_start}")

    depth = 0
    i = brace
    while i < len(text):
        ch = text[i]
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return start, i + 1
        i += 1

    raise RuntimeError(f"Could not find closing brace for: {signature_start}")


def replace_function(text: str, signature_start: str, replacement: str) -> str:
    start, end = find_function_bounds(text, signature_start)
    return text[:start] + replacement + text[end:]


def patch_globals(path: Path) -> bool:
    text = path.read_text(encoding="utf-8")
    original = text

    # Normalize notbinding to atomic<bool>.
    text = text.replace(
        "    inline bool notbinding = true;\n",
        "    inline std::atomic<bool> notbinding{ true };\n",
    )

    if "inline std::atomic<bool> notbinding{ true };" not in text:
        raise RuntimeError("Could not find Globals::notbinding declaration.")

    # Remove possible duplicate suppress/capture lines from previous attempts.
    text = re.sub(
        r"    inline std::atomic<bool> g_keybindCaptureActive\{ false \};\n",
        "",
        text,
    )
    text = re.sub(
        r"    inline std::atomic<bool> g_suppressHotkeysUntilRelease\{ false \};\n",
        "",
        text,
    )

    # Add the final flags immediately after notbinding.
    text = text.replace(
        "    inline std::atomic<bool> notbinding{ true };\n",
        "    inline std::atomic<bool> notbinding{ true };\n"
        "    inline std::atomic<bool> g_keybindCaptureActive{ false };\n"
        "    inline std::atomic<bool> g_suppressHotkeysUntilRelease{ false };\n",
        1,
    )

    if text != original:
        backup(path, original)
        path.write_text(text, encoding="utf-8")
        return True

    return False


def patch_app_ui(path: Path) -> bool:
    text = path.read_text(encoding="utf-8")
    original = text

    # Add per-control pre-capture release state.
    if "bool waitingForReleaseBeforeCapture = false;" not in text:
        text = text.replace(
            "    bool firstRun = true;\n"
            "    std::array<bool, 258> keyWasPressed{};\n",
            "    bool firstRun = true;\n"
            "    bool waitingForReleaseBeforeCapture = false;\n"
            "    std::array<bool, 258> keyWasPressed{};\n",
            1,
        )

    # Add final helper functions. Do not reuse names from previous patch attempts.
    helper_marker = "bool AnyPhysicalKeyOrMouseButtonPressedForBinding()"
    if helper_marker not in text:
        insert_after = (
            "std::unordered_map<unsigned int*, BindingState> g_bindingStates;\n"
            "bool g_hasStoredSettingsWindowPos = false;\n"
            "ImVec2 g_storedSettingsWindowPos{};\n"
        )

        helpers = (
            "std::unordered_map<unsigned int*, BindingState> g_bindingStates;\n"
            "bool g_hasStoredSettingsWindowPos = false;\n"
            "ImVec2 g_storedSettingsWindowPos{};\n\n"
            "bool AnyPhysicalKeyOrMouseButtonPressedForBinding()\n"
            "{\n"
            "    for (int key = 1; key <= 0xFF; ++key) {\n"
            "        if (IsKeyPressed(static_cast<smu::core::KeyCode>(key))) {\n"
            "            return true;\n"
            "        }\n"
            "    }\n"
            "    return false;\n"
            "}\n\n"
            "void StartKeybindCapture(BindingState& state)\n"
            "{\n"
            "    state.bindingMode = true;\n"
            "    state.notBinding = false;\n"
            "    state.waitingForReleaseBeforeCapture = true;\n"
            "    state.firstRun = false;\n"
            "    state.pendingModifierKey = 0;\n"
            "    state.pendingModifierCombo = 0;\n"
            "    state.keyWasPressed.fill(false);\n"
            "    state.rebindTime = std::chrono::steady_clock::now();\n"
            "    state.buttonText = \"Release Keys...\";\n\n"
            "    g_keybindCaptureActive.store(true, std::memory_order_release);\n"
            "    g_suppressHotkeysUntilRelease.store(true, std::memory_order_release);\n"
            "    notbinding.store(false, std::memory_order_release);\n"
            "}\n\n"
            "void FinishKeybindCapture()\n"
            "{\n"
            "    g_keybindCaptureActive.store(false, std::memory_order_release);\n"
            "    g_suppressHotkeysUntilRelease.store(true, std::memory_order_release);\n"
            "    notbinding.store(false, std::memory_order_release);\n"
            "}\n\n"
            "void RefreshFinishedKeybindSuppression()\n"
            "{\n"
            "    if (g_keybindCaptureActive.load(std::memory_order_acquire) ||\n"
            "        !g_suppressHotkeysUntilRelease.load(std::memory_order_acquire)) {\n"
            "        return;\n"
            "    }\n\n"
            "    if (AnyPhysicalKeyOrMouseButtonPressedForBinding()) {\n"
            "        notbinding.store(false, std::memory_order_release);\n"
            "        return;\n"
            "    }\n\n"
            "    g_suppressHotkeysUntilRelease.store(false, std::memory_order_release);\n"
            "    notbinding.store(true, std::memory_order_release);\n"
            "}\n"
        )

        if insert_after not in text:
            raise RuntimeError("Could not find app_ui.cpp binding globals insertion point.")

        text = text.replace(insert_after, helpers, 1)

    new_bind_key_mode = r'''unsigned int BindKeyMode(unsigned int* keyVar, unsigned int currentkey, int currentSection)
{
    BindingState& state = g_bindingStates[keyVar];
    RefreshFinishedKeybindSuppression();

    if (state.bindingMode) {
        g_keybindCaptureActive.store(true, std::memory_order_release);
        notbinding.store(false, std::memory_order_release);
        state.rebindTime = std::chrono::steady_clock::now();

        if (state.waitingForReleaseBeforeCapture) {
            state.buttonText = "Release Keys...";
            CopyString(state.keyBuffer, sizeof(state.keyBuffer), "Release keys...");
            CopyString(state.keyBufferHuman, sizeof(state.keyBufferHuman), "Release keys...");

            if (AnyPhysicalKeyOrMouseButtonPressedForBinding()) {
                return currentkey;
            }

            state.waitingForReleaseBeforeCapture = false;
            state.firstRun = false;
            state.pendingModifierKey = 0;
            state.pendingModifierCombo = 0;
            state.keyWasPressed.fill(false);
            state.buttonText = "Press a Key...";
        }

        const unsigned int currentModifiers = CurrentModifierMask();
        if (currentModifiers == 0) {
            CopyString(state.keyBuffer, sizeof(state.keyBuffer), "Waiting...");
            CopyString(state.keyBufferHuman, sizeof(state.keyBufferHuman), "Waiting...");
        } else {
            char previewHex[64] = {};
            FormatHexKeyString(currentModifiers, previewHex, sizeof(previewHex));
            std::string hexStr(previewHex);
            const std::string suffix = " + 0x0";
            if (const auto pos = hexStr.find(suffix); pos != std::string::npos) {
                hexStr = hexStr.substr(0, pos) + " + ...";
            }

            std::string previewHuman;
            if (currentModifiers & HOTKEY_MASK_WIN) previewHuman += "Win + ";
            if (currentModifiers & HOTKEY_MASK_CTRL) previewHuman += "Ctrl + ";
            if (currentModifiers & HOTKEY_MASK_ALT) previewHuman += "Alt + ";
            if (currentModifiers & HOTKEY_MASK_SHIFT) previewHuman += "Shift + ";
            previewHuman += "...";

            CopyString(state.keyBuffer, sizeof(state.keyBuffer), hexStr);
            CopyString(state.keyBufferHuman, sizeof(state.keyBufferHuman), previewHuman);
        }

        auto backend = smu::platform::GetInputBackend();
        if (!backend) {
            return currentkey;
        }

        for (int key = 1; key < static_cast<int>(state.keyWasPressed.size()); ++key) {
            const bool currentlyPressed = IsKeyPressed(static_cast<smu::core::KeyCode>(key));
            const bool wasPressed = state.keyWasPressed[key];
            state.keyWasPressed[key] = currentlyPressed;

            if (!currentlyPressed || wasPressed) {
                continue;
            }

            if (smu::core::IsModifierKey(key)) {
                state.pendingModifierKey = static_cast<unsigned int>(key);
                state.pendingModifierCombo = currentModifiers;
                continue;
            }

            const unsigned int finalCombo =
                NormalizeBoundHotkey((static_cast<unsigned int>(key) & HOTKEY_KEY_MASK) | currentModifiers);

            state.bindingMode = false;
            state.waitingForReleaseBeforeCapture = false;
            state.firstRun = true;
            state.pendingModifierKey = 0;
            state.pendingModifierCombo = 0;

            GetKeyNameFromHex(finalCombo, state.keyBufferHuman, sizeof(state.keyBufferHuman));
            FormatHexKeyString(finalCombo, state.keyBuffer, sizeof(state.keyBuffer));
            state.buttonText = "Click to Bind Key";

            FinishKeybindCapture();
            return finalCombo;
        }

        if (state.pendingModifierKey != 0 &&
            !IsKeyPressed(static_cast<smu::core::KeyCode>(state.pendingModifierKey)) &&
            currentModifiers == 0) {
            const unsigned int finalCombo = NormalizeBoundHotkey(
                (state.pendingModifierKey & HOTKEY_KEY_MASK) | state.pendingModifierCombo);

            state.bindingMode = false;
            state.waitingForReleaseBeforeCapture = false;
            state.firstRun = true;
            state.pendingModifierKey = 0;
            state.pendingModifierCombo = 0;

            GetKeyNameFromHex(finalCombo, state.keyBufferHuman, sizeof(state.keyBufferHuman));
            FormatHexKeyString(finalCombo, state.keyBuffer, sizeof(state.keyBuffer));
            state.buttonText = "Click to Bind Key";

            FinishKeybindCapture();
            return finalCombo;
        }

        return currentkey;
    }

    state.firstRun = true;
    if (currentSection != state.lastSelectedSection || currentSection == -1) {
        FormatHexKeyString(currentkey, state.keyBuffer, sizeof(state.keyBuffer));
        GetKeyNameFromHex(currentkey, state.keyBufferHuman, sizeof(state.keyBufferHuman));
        if (currentSection != -1) {
            state.lastSelectedSection = currentSection;
        }
    }

    state.buttonText = "Click to Bind Key";
    auto currentTime = std::chrono::steady_clock::now();
    std::chrono::duration<double> elapsedtime = currentTime - state.rebindTime;
    if (!g_keybindCaptureActive.load(std::memory_order_acquire) &&
        !g_suppressHotkeysUntilRelease.load(std::memory_order_acquire) &&
        elapsedtime.count() >= 0.3) {
        state.notBinding = true;
        notbinding.store(true, std::memory_order_release);
    }

    return currentkey;
}'''

    text = replace_function(
        text,
        "unsigned int BindKeyMode(unsigned int* keyVar, unsigned int currentkey, int currentSection)",
        new_bind_key_mode,
    )

    new_draw_keybind = r'''void DrawKeyBindControl(const char* id, unsigned int& key, int currentSection, float humanWidth = 170.0f, float hexWidth = 130.0f)
{
    ImGui::PushID(id);
    BindingState& state = g_bindingStates[&key];

    if (ImGui::Button(state.buttonText.c_str())) {
        StartKeybindCapture(state);
    }

    ImGui::SameLine();
    key = BindKeyMode(&key, key, currentSection);

    ImGui::SetNextItemWidth(humanWidth);
    GetKeyNameFromHex(key, state.keyBufferHuman, sizeof(state.keyBufferHuman));
    ImGui::InputText("##KeyHuman", state.keyBufferHuman, sizeof(state.keyBufferHuman), ImGuiInputTextFlags_ReadOnly);

    ImGui::SameLine();
    ImGui::TextWrapped("Key Binding");

    ImGui::SameLine();
    ImGui::SetNextItemWidth(hexWidth);
    ImGui::InputText("##KeyHex", state.keyBuffer, sizeof(state.keyBuffer), ImGuiInputTextFlags_CharsNoBlank | ImGuiInputTextFlags_CharsHexadecimal | ImGuiInputTextFlags_ReadOnly);

    ImGui::SameLine();
    ImGui::TextWrapped("(Hexadecimal)");
    ImGui::PopID();
}'''

    text = replace_function(
        text,
        "void DrawKeyBindControl(const char* id, unsigned int& key, int currentSection",
        new_draw_keybind,
    )

    if text != original:
        backup(path, original)
        path.write_text(text, encoding="utf-8")
        return True

    return False


def patch_macro_runtime(path: Path) -> bool:
    text = path.read_text(encoding="utf-8")
    original = text

    helper_marker = "bool AnyInputPressedForHotkeySuppression()"
    if helper_marker not in text:
        insert_after = '''bool ShouldKeepRunning()
{
    return running.load(std::memory_order_acquire) && !done.load(std::memory_order_acquire);
}
'''
        helper = insert_after + r'''
bool AnyInputPressedForHotkeySuppression()
{
    auto input = smu::platform::GetInputBackend();
    if (!input) {
        return false;
    }

    for (unsigned int key = 1; key <= 0xFF; ++key) {
        if (input->isKeyPressed(key)) {
            return true;
        }
    }

    return false;
}
'''
        if insert_after not in text:
            raise RuntimeError("Could not find macro_runtime.cpp ShouldKeepRunning insertion point.")
        text = text.replace(insert_after, helper, 1)

    gate_marker = "g_keybindCaptureActive.load(std::memory_order_acquire)"
    if gate_marker not in text[text.find("void MacroRuntime::controllerLoop()"):text.find("void MacroRuntime::refreshTargetProcesses")]:
        old = '''        refreshTargetProcesses();

        if (!macrotoggled || !notbinding) {
'''
        alt = '''        refreshTargetProcesses();

        if (!macrotoggled || !notbinding.load(std::memory_order_acquire)) {
'''
        new = '''        refreshTargetProcesses();

        if (g_keybindCaptureActive.load(std::memory_order_acquire)) {
            if (freezeSuspended_) {
                setTargetSuspended(false);
            }
            std::this_thread::sleep_for(5ms);
            continue;
        }

        if (g_suppressHotkeysUntilRelease.load(std::memory_order_acquire)) {
            if (AnyInputPressedForHotkeySuppression()) {
                if (freezeSuspended_) {
                    setTargetSuspended(false);
                }
                std::this_thread::sleep_for(5ms);
                continue;
            }

            g_suppressHotkeysUntilRelease.store(false, std::memory_order_release);
            notbinding.store(true, std::memory_order_release);
        }

        if (!macrotoggled || !notbinding.load(std::memory_order_acquire)) {
'''
        if old in text:
            text = text.replace(old, new, 1)
        elif alt in text:
            text = text.replace(alt, new, 1)
        else:
            raise RuntimeError("Could not find controllerLoop refresh/notbinding gate.")

    # Normalize remaining notbinding checks in runtime.
    text = text.replace(
        "if (!macrotoggled || !notbinding) {",
        "if (!macrotoggled || !notbinding.load(std::memory_order_acquire)) {",
    )
    text = text.replace(
        "const bool canProcess = foregroundAllowed && section_toggles[13] && macrotoggled && notbinding;",
        "const bool canProcess = foregroundAllowed && section_toggles[13] && macrotoggled && notbinding.load(std::memory_order_acquire);",
    )
    text = text.replace(
        "                   macrotoggled &&\n"
        "                   notbinding &&\n",
        "                   macrotoggled &&\n"
        "                   notbinding.load(std::memory_order_acquire) &&\n",
    )

    new_is_hotkey_pressed = r'''bool MacroRuntime::isHotkeyPressed(unsigned int combinedKey) const
{
    auto input = smu::platform::GetInputBackend();
    if (!input) {
        return false;
    }

    if (g_keybindCaptureActive.load(std::memory_order_acquire) ||
        g_suppressHotkeysUntilRelease.load(std::memory_order_acquire)) {
        return false;
    }

    const unsigned int key = combinedKey & HOTKEY_KEY_MASK;
    if (key == 0 || key == smu::core::SMU_VK_MOUSE_WHEEL_UP || key == smu::core::SMU_VK_MOUSE_WHEEL_DOWN) {
        return false;
    }

    if ((combinedKey & HOTKEY_MASK_WIN) && !IsModifierPressed(*input, VK_LWIN)) return false;
    if ((combinedKey & HOTKEY_MASK_CTRL) && !IsModifierPressed(*input, VK_CONTROL)) return false;
    if ((combinedKey & HOTKEY_MASK_ALT) && !IsModifierPressed(*input, VK_MENU)) return false;
    if ((combinedKey & HOTKEY_MASK_SHIFT) && !IsModifierPressed(*input, VK_SHIFT)) return false;
    return IsModifierPressed(*input, key);
}'''

    text = replace_function(
        text,
        "bool MacroRuntime::isHotkeyPressed(unsigned int combinedKey) const",
        new_is_hotkey_pressed,
    )

    if text != original:
        backup(path, original)
        path.write_text(text, encoding="utf-8")
        return True

    return False


def main() -> int:
    for path in ROOT_FILES.values():
        if not path.exists():
            print(f"error: missing {path}. Run this from the repository root.", file=sys.stderr)
            return 1

    try:
        changed = []
        if patch_globals(ROOT_FILES["globals"]):
            changed.append(str(ROOT_FILES["globals"]))
        if patch_app_ui(ROOT_FILES["ui"]):
            changed.append(str(ROOT_FILES["ui"]))
        if patch_macro_runtime(ROOT_FILES["runtime"]):
            changed.append(str(ROOT_FILES["runtime"]))

        if changed:
            print("Patched:")
            for path in changed:
                print(f"  - {path}")
            print("Backups created as *.pre_keybind_final.bak")
        else:
            print("No changes made. Files already appear patched.")

        return 0

    except Exception as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
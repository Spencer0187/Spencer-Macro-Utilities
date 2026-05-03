#include "input_sendinput.h"

#if defined(_WIN32)

#include "../../core/legacy_globals.h"

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>

#include <chrono>
#include <cstdint>
#include <cstdio>
#include <mutex>
#include <thread>
#include <vector>

namespace smu::platform::windows {
namespace {

using namespace Globals;

constexpr ULONG_PTR kInjectedInputTag = static_cast<ULONG_PTR>(0x534D4301u);

std::mutex g_guiInjectedInputBudgetMutex;
auto g_guiInjectedInputBudgetResetTime = std::chrono::steady_clock::now();
int g_guiInjectedInputBudgetRemaining = 50;

void TagInjectedInput(INPUT& input)
{
    if (input.type == INPUT_MOUSE) {
        input.mi.dwExtraInfo = kInjectedInputTag;
    } else if (input.type == INPUT_KEYBOARD) {
        input.ki.dwExtraInfo = kInjectedInputTag;
    }
}

bool IsGuiWindowForegroundForBudget()
{
    HWND guiWindow = hwnd;
    HWND foreground = GetForegroundWindow();
    if (!guiWindow || !foreground) {
        return false;
    }

    return foreground == guiWindow || GetAncestor(foreground, GA_ROOT) == guiWindow;
}

void AcquireInjectedInputBudget(std::size_t inputCount)
{
    if (!IsGuiWindowForegroundForBudget()) {
        return;
    }

    constexpr int kMaxInjectedInputsPerGuiSlice = 50;
    constexpr auto kGuiSliceDuration = std::chrono::milliseconds(16);

    while (true) {
        std::chrono::milliseconds sleepDuration{0};
        {
            std::lock_guard<std::mutex> lock(g_guiInjectedInputBudgetMutex);
            const auto now = std::chrono::steady_clock::now();
            const auto elapsed = now - g_guiInjectedInputBudgetResetTime;
            if (elapsed >= kGuiSliceDuration) {
                g_guiInjectedInputBudgetResetTime = now;
                g_guiInjectedInputBudgetRemaining = kMaxInjectedInputsPerGuiSlice;
            }

            if (inputCount <= static_cast<std::size_t>(g_guiInjectedInputBudgetRemaining)) {
                g_guiInjectedInputBudgetRemaining -= static_cast<int>(inputCount);
                return;
            }

            const auto remaining = std::chrono::duration_cast<std::chrono::milliseconds>(kGuiSliceDuration - elapsed);
            sleepDuration = remaining > std::chrono::milliseconds(0) ? remaining : std::chrono::milliseconds(1);
        }

        std::this_thread::sleep_for(sleepDuration);
    }
}

void DispatchTaggedInputs(INPUT* inputs, std::size_t inputCount)
{
    if (inputCount == 0) {
        return;
    }

    AcquireInjectedInputBudget(inputCount);
    SendInput(static_cast<UINT>(inputCount), inputs, sizeof(INPUT));
}

bool IsKeyPressedNative(WORD vkKey)
{
    if (vkKey == smu::core::SMU_VK_MOUSE_WHEEL_UP || vkKey == smu::core::SMU_VK_MOUSE_WHEEL_DOWN) {
        return false;
    }

    return (::GetAsyncKeyState(vkKey) & 0x8000) != 0;
}

void HoldKeyNative(WORD scanCode, bool extended)
{
    INPUT input = {};
    input.type = INPUT_KEYBOARD;
    input.ki.wScan = scanCode;
    input.ki.dwFlags = KEYEVENTF_SCANCODE;
    if (extended) {
        input.ki.dwFlags |= KEYEVENTF_EXTENDEDKEY;
    }
    TagInjectedInput(input);
    DispatchTaggedInputs(&input, 1);
}

void ReleaseKeyNative(WORD scanCode, bool extended)
{
    INPUT input = {};
    input.type = INPUT_KEYBOARD;
    input.ki.wScan = scanCode;
    input.ki.dwFlags = KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP;
    if (extended) {
        input.ki.dwFlags |= KEYEVENTF_EXTENDEDKEY;
    }
    TagInjectedInput(input);
    DispatchTaggedInputs(&input, 1);
}

void SendMouseWheel(int delta)
{
    INPUT input = {};
    input.type = INPUT_MOUSE;
    input.mi.dwFlags = MOUSEEVENTF_WHEEL;
    input.mi.mouseData = static_cast<DWORD>(delta);
    TagInjectedInput(input);
    DispatchTaggedInputs(&input, 1);
}

void HoldChordNative(unsigned int combinedKey)
{
    const WORD vk = combinedKey & HOTKEY_KEY_MASK;
    const bool useWin = (combinedKey & HOTKEY_MASK_WIN) != 0;
    const bool useCtrl = (combinedKey & HOTKEY_MASK_CTRL) != 0;
    const bool useAlt = (combinedKey & HOTKEY_MASK_ALT) != 0;
    const bool useShift = (combinedKey & HOTKEY_MASK_SHIFT) != 0;

    std::vector<INPUT> inputs;
    inputs.reserve(5);

    auto pushModifier = [&inputs](WORD modifierVk) {
        INPUT input = {};
        input.type = INPUT_KEYBOARD;
        input.ki.wVk = modifierVk;
        TagInjectedInput(input);
        inputs.push_back(input);
    };

    if (useWin) pushModifier(VK_LWIN);
    if (useCtrl) pushModifier(VK_CONTROL);
    if (useAlt) pushModifier(VK_MENU);
    if (useShift) pushModifier(VK_SHIFT);

    INPUT mainInput = {};
    if (vk == smu::core::SMU_VK_MOUSE_WHEEL_UP) {
        mainInput.type = INPUT_MOUSE;
        mainInput.mi.dwFlags = MOUSEEVENTF_WHEEL;
        mainInput.mi.mouseData = WHEEL_DELTA * 100;
    } else if (vk == smu::core::SMU_VK_MOUSE_WHEEL_DOWN) {
        mainInput.type = INPUT_MOUSE;
        mainInput.mi.dwFlags = MOUSEEVENTF_WHEEL;
        mainInput.mi.mouseData = static_cast<DWORD>(-WHEEL_DELTA * 100);
    } else if (vk >= VK_LBUTTON && vk <= VK_XBUTTON2) {
        mainInput.type = INPUT_MOUSE;
        if (vk == VK_LBUTTON) mainInput.mi.dwFlags = MOUSEEVENTF_LEFTDOWN;
        else if (vk == VK_RBUTTON) mainInput.mi.dwFlags = MOUSEEVENTF_RIGHTDOWN;
        else if (vk == VK_MBUTTON) mainInput.mi.dwFlags = MOUSEEVENTF_MIDDLEDOWN;
        else if (vk == VK_XBUTTON1) {
            mainInput.mi.dwFlags = MOUSEEVENTF_XDOWN;
            mainInput.mi.mouseData = XBUTTON1;
        } else if (vk == VK_XBUTTON2) {
            mainInput.mi.dwFlags = MOUSEEVENTF_XDOWN;
            mainInput.mi.mouseData = XBUTTON2;
        }
    } else {
        mainInput.type = INPUT_KEYBOARD;
        mainInput.ki.wScan = MapVirtualKeyA(vk, MAPVK_VK_TO_VSC);
        mainInput.ki.dwFlags = KEYEVENTF_SCANCODE;
    }

    TagInjectedInput(mainInput);
    inputs.push_back(mainInput);
    DispatchTaggedInputs(inputs.data(), inputs.size());
}

void ReleaseChordNative(unsigned int combinedKey)
{
    const WORD vk = combinedKey & HOTKEY_KEY_MASK;
    const bool useWin = (combinedKey & HOTKEY_MASK_WIN) != 0;
    const bool useCtrl = (combinedKey & HOTKEY_MASK_CTRL) != 0;
    const bool useAlt = (combinedKey & HOTKEY_MASK_ALT) != 0;
    const bool useShift = (combinedKey & HOTKEY_MASK_SHIFT) != 0;

    if (vk == smu::core::SMU_VK_MOUSE_WHEEL_UP || vk == smu::core::SMU_VK_MOUSE_WHEEL_DOWN) {
        return;
    }

    std::vector<INPUT> inputs;
    inputs.reserve(5);

    INPUT mainInput = {};
    if (vk >= VK_LBUTTON && vk <= VK_XBUTTON2) {
        mainInput.type = INPUT_MOUSE;
        if (vk == VK_LBUTTON) mainInput.mi.dwFlags = MOUSEEVENTF_LEFTUP;
        else if (vk == VK_RBUTTON) mainInput.mi.dwFlags = MOUSEEVENTF_RIGHTUP;
        else if (vk == VK_MBUTTON) mainInput.mi.dwFlags = MOUSEEVENTF_MIDDLEUP;
        else if (vk == VK_XBUTTON1) {
            mainInput.mi.dwFlags = MOUSEEVENTF_XUP;
            mainInput.mi.mouseData = XBUTTON1;
        } else if (vk == VK_XBUTTON2) {
            mainInput.mi.dwFlags = MOUSEEVENTF_XUP;
            mainInput.mi.mouseData = XBUTTON2;
        }
    } else {
        mainInput.type = INPUT_KEYBOARD;
        mainInput.ki.wScan = MapVirtualKeyA(vk, MAPVK_VK_TO_VSC);
        mainInput.ki.dwFlags = KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP;
    }
    TagInjectedInput(mainInput);
    inputs.push_back(mainInput);

    auto pushModifierUp = [&inputs](WORD modifierVk) {
        INPUT input = {};
        input.type = INPUT_KEYBOARD;
        input.ki.wVk = modifierVk;
        input.ki.dwFlags = KEYEVENTF_KEYUP;
        TagInjectedInput(input);
        inputs.push_back(input);
    };

    if (useShift) pushModifierUp(VK_SHIFT);
    if (useAlt) pushModifierUp(VK_MENU);
    if (useCtrl) pushModifierUp(VK_CONTROL);
    if (useWin) pushModifierUp(VK_LWIN);

    DispatchTaggedInputs(inputs.data(), inputs.size());
}

void MoveMouseNative(int dx, int dy)
{
    INPUT input = {};
    input.type = INPUT_MOUSE;
    dx = static_cast<int>(static_cast<std::int64_t>(dx) * display_scale / 100);
    dy = static_cast<int>(static_cast<std::int64_t>(dy) * display_scale / 100);
    input.mi.dx = dx;
    input.mi.dy = dy;
    input.mi.dwFlags = MOUSEEVENTF_MOVE;
    TagInjectedInput(input);
    DispatchTaggedInputs(&input, 1);
}

class SendInputBackend final : public smu::platform::InputBackend {
public:
    bool init(std::string* errorMessage = nullptr) override
    {
        if (errorMessage) {
            errorMessage->clear();
        }
        return true;
    }

    void shutdown() override {}

    bool isKeyPressed(PlatformKeyCode key) const override
    {
        return IsKeyPressedNative(static_cast<WORD>(key));
    }

    void holdKey(PlatformKeyCode key, bool extended = false) override
    {
        HoldKeyNative(static_cast<WORD>(key), extended);
    }

    void releaseKey(PlatformKeyCode key, bool extended = false) override
    {
        ReleaseKeyNative(static_cast<WORD>(key), extended);
    }

    void pressKey(PlatformKeyCode key, int delayMs = 50) override
    {
        holdKey(key);
        std::this_thread::sleep_for(std::chrono::milliseconds(delayMs));
        releaseKey(key);
    }

    void holdKeyChord(PlatformKeyCode combinedKey) override
    {
        HoldChordNative(combinedKey);
    }

    void releaseKeyChord(PlatformKeyCode combinedKey) override
    {
        ReleaseChordNative(combinedKey);
    }

    void moveMouse(int dx, int dy) override
    {
        MoveMouseNative(dx, dy);
    }

    void mouseWheel(int delta) override
    {
        SendMouseWheel(delta);
    }

    std::optional<PlatformKeyCode> getCurrentPressedKey() const override
    {
        for (PlatformKeyCode key = 1; key < smu::core::SMU_VK_MOUSE_WHEEL_UP; ++key) {
            if (IsKeyPressedNative(static_cast<WORD>(key))) {
                return key;
            }
        }
        return std::nullopt;
    }

    std::string formatKeyName(PlatformKeyCode key) const override
    {
        const std::string_view coreName = smu::core::KeyCodeName(key);
        if (!coreName.empty()) {
            return std::string(coreName);
        }

        auto it = vkToString.find(static_cast<int>(key));
        if (it != vkToString.end()) {
            return it->second;
        }

        char keyNameBuffer[64] = {};
        const UINT scanCode = MapVirtualKeyA(static_cast<UINT>(key), MAPVK_VK_TO_VSC);
        if (scanCode != 0 && GetKeyNameTextA(static_cast<LONG>(scanCode << 16), keyNameBuffer, sizeof(keyNameBuffer)) > 0) {
            return keyNameBuffer;
        }

        char fallback[16] = {};
        std::snprintf(fallback, sizeof(fallback), "0x%X", static_cast<unsigned int>(key));
        return fallback;
    }
};

} // namespace

std::shared_ptr<smu::platform::InputBackend> CreateWindowsInputBackend()
{
    return std::make_shared<SendInputBackend>();
}

} // namespace smu::platform::windows

#endif

#include "input_sendinput.h"

#if defined(_WIN32)

#include "../../core/legacy_globals.h"
#include "../logging.h"

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>

#include <atomic>
#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <mutex>
#include <optional>
#include <string>
#include <thread>
#include <vector>

namespace smu::platform::windows {
namespace {

using namespace Globals;

constexpr const char kWindowsBunnyhopHookWarningId[] = "windows_bunnyhop_hook_install_failed";

constexpr ULONG_PTR kInjectedInputTag = static_cast<ULONG_PTR>(0x534D4301u);

std::mutex g_guiInjectedInputBudgetMutex;
auto g_guiInjectedInputBudgetResetTime = std::chrono::steady_clock::now();
int g_guiInjectedInputBudgetRemaining = 50;

struct MonitorCaptureInfo {
    HMONITOR monitor = nullptr;
    ScreenBounds bounds {};
    std::chrono::microseconds refreshInterval {16666};
    std::optional<int> refreshRateHz;
};

std::chrono::microseconds RefreshIntervalForMonitor(HMONITOR monitor)
{
    constexpr std::chrono::microseconds kDefaultRefreshInterval{16666};

    if (!monitor) {
        return kDefaultRefreshInterval;
    }

    MONITORINFOEXA monitorInfo = {};
    monitorInfo.cbSize = sizeof(monitorInfo);
    if (GetMonitorInfoA(monitor, &monitorInfo)) {
        DEVMODEA mode = {};
        mode.dmSize = sizeof(mode);
        if (EnumDisplaySettingsA(monitorInfo.szDevice, ENUM_CURRENT_SETTINGS, &mode) &&
            mode.dmDisplayFrequency > 1) {
            const auto hz = static_cast<std::int64_t>(mode.dmDisplayFrequency);
            return std::chrono::microseconds{std::max<std::int64_t>(1, 1000000 / hz)};
        }
    }

    HDC screenDc = GetDC(nullptr);
    if (screenDc) {
        const int refresh = GetDeviceCaps(screenDc, VREFRESH);
        ReleaseDC(nullptr, screenDc);
        if (refresh > 1) {
            const auto hz = static_cast<std::int64_t>(refresh);
            return std::chrono::microseconds{std::max<std::int64_t>(1, 1000000 / hz)};
        }
    }

    return kDefaultRefreshInterval;
}

std::optional<int> RefreshRateForMonitor(HMONITOR monitor)
{
    if (!monitor) {
        return std::nullopt;
    }

    MONITORINFOEXA monitorInfo = {};
    monitorInfo.cbSize = sizeof(monitorInfo);
    if (GetMonitorInfoA(monitor, &monitorInfo)) {
        DEVMODEA mode = {};
        mode.dmSize = sizeof(mode);
        if (EnumDisplaySettingsA(monitorInfo.szDevice, ENUM_CURRENT_SETTINGS, &mode) &&
            mode.dmDisplayFrequency > 1) {
            return static_cast<int>(mode.dmDisplayFrequency);
        }
    }

    HDC screenDc = GetDC(nullptr);
    if (screenDc) {
        const int refresh = GetDeviceCaps(screenDc, VREFRESH);
        ReleaseDC(nullptr, screenDc);
        if (refresh > 1) {
            return refresh;
        }
    }

    return std::nullopt;
}

bool ResolveMonitorCaptureInfo(int x, int y, MonitorCaptureInfo& info, std::string* errorMessage)
{
    POINT point = {};
    point.x = x;
    point.y = y;

    info.monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
    if (!info.monitor) {
        if (errorMessage) {
            *errorMessage = "failed to resolve the monitor for the requested pixel";
        }
        return false;
    }

    MONITORINFOEXA monitorInfo = {};
    monitorInfo.cbSize = sizeof(monitorInfo);
    if (!GetMonitorInfoA(info.monitor, &monitorInfo)) {
        if (errorMessage) {
            *errorMessage = "GetMonitorInfo failed while preparing screen pixel sampling";
        }
        return false;
    }

    const int left = static_cast<int>(monitorInfo.rcMonitor.left);
    const int top = static_cast<int>(monitorInfo.rcMonitor.top);
    const int width = static_cast<int>(monitorInfo.rcMonitor.right - monitorInfo.rcMonitor.left);
    const int height = static_cast<int>(monitorInfo.rcMonitor.bottom - monitorInfo.rcMonitor.top);
    if (width <= 0 || height <= 0) {
        if (errorMessage) {
            *errorMessage = "monitor bounds were invalid while preparing screen pixel sampling";
        }
        return false;
    }

    info.bounds = ScreenBounds{left, top, width, height};
    info.refreshInterval = RefreshIntervalForMonitor(info.monitor);
    info.refreshRateHz = RefreshRateForMonitor(info.monitor);
    return true;
}


std::thread g_bunnyhopPhysicalKeyHookThread;
std::atomic<bool> g_bunnyhopPhysicalKeyHookRunning{false};
std::atomic<bool> g_bunnyhopPhysicalKeyHookReady{false};
std::atomic<DWORD> g_bunnyhopPhysicalKeyHookThreadId{0};
HHOOK g_bunnyhopPhysicalKeyHook = nullptr;

void TagInjectedInput(INPUT& input)
{
    if (input.type == INPUT_MOUSE) {
        input.mi.dwExtraInfo = kInjectedInputTag;
    } else if (input.type == INPUT_KEYBOARD) {
        input.ki.dwExtraInfo = kInjectedInputTag;
    }
}


bool IsInjectedKeyboardEvent(const KBDLLHOOKSTRUCT& event)
{
    return (event.flags & LLKHF_INJECTED) != 0 ||
           (event.flags & LLKHF_LOWER_IL_INJECTED) != 0 ||
           event.dwExtraInfo == kInjectedInputTag;
}

void UpdateBunnyhopPhysicalKeyState(WPARAM wParam, const KBDLLHOOKSTRUCT& event)
{
    const unsigned int configuredKey = vk_bunnyhopkey & HOTKEY_KEY_MASK;
    if (configuredKey == 0 ||
        configuredKey == smu::core::SMU_VK_MOUSE_WHEEL_UP ||
        configuredKey == smu::core::SMU_VK_MOUSE_WHEEL_DOWN) {
        return;
    }

    if (event.vkCode != configuredKey) {
        return;
    }

    if (IsInjectedKeyboardEvent(event)) {
        return;
    }

    if (wParam == WM_KEYDOWN || wParam == WM_SYSKEYDOWN) {
        g_isVk_BunnyhopHeldDown.store(true, std::memory_order_release);
    } else if (wParam == WM_KEYUP || wParam == WM_SYSKEYUP) {
        g_isVk_BunnyhopHeldDown.store(false, std::memory_order_release);
    }
}

LRESULT CALLBACK BunnyhopPhysicalKeyHookProc(int nCode, WPARAM wParam, LPARAM lParam)
{
    if (nCode == HC_ACTION && lParam) {
        const auto* event = reinterpret_cast<const KBDLLHOOKSTRUCT*>(lParam);
        UpdateBunnyhopPhysicalKeyState(wParam, *event);
    }

    return CallNextHookEx(g_bunnyhopPhysicalKeyHook, nCode, wParam, lParam);
}

void StartBunnyhopPhysicalKeyHook()
{
    bool expected = false;
    if (!g_bunnyhopPhysicalKeyHookRunning.compare_exchange_strong(expected, true, std::memory_order_acq_rel)) {
        return;
    }

    g_bunnyhopPhysicalKeyHookReady.store(false, std::memory_order_release);
    g_bunnyhopPhysicalKeyHookThreadId.store(0, std::memory_order_release);
    g_isVk_BunnyhopHeldDown.store(false, std::memory_order_release);

    g_bunnyhopPhysicalKeyHookThread = std::thread([] {
        g_bunnyhopPhysicalKeyHookThreadId.store(GetCurrentThreadId(), std::memory_order_release);

        MSG msg {};
        PeekMessageW(&msg, nullptr, WM_USER, WM_USER, PM_NOREMOVE);

        g_bunnyhopPhysicalKeyHook = SetWindowsHookExW(
            WH_KEYBOARD_LL,
            BunnyhopPhysicalKeyHookProc,
            GetModuleHandleW(nullptr),
            0);

        if (!g_bunnyhopPhysicalKeyHook) {
            LogWarning("Windows input backend failed to install Smart Bunnyhop physical-key hook. Bunnyhop may stop after one injected key press.",
                kWindowsBunnyhopHookWarningId, true);
        }

        g_bunnyhopPhysicalKeyHookReady.store(true, std::memory_order_release);

        while (g_bunnyhopPhysicalKeyHookRunning.load(std::memory_order_acquire)) {
            const BOOL result = GetMessageW(&msg, nullptr, 0, 0);
            if (result <= 0) {
                break;
            }

            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        if (g_bunnyhopPhysicalKeyHook) {
            UnhookWindowsHookEx(g_bunnyhopPhysicalKeyHook);
            g_bunnyhopPhysicalKeyHook = nullptr;
        }

        g_isVk_BunnyhopHeldDown.store(false, std::memory_order_release);
        g_bunnyhopPhysicalKeyHookThreadId.store(0, std::memory_order_release);
        g_bunnyhopPhysicalKeyHookReady.store(false, std::memory_order_release);
    });

    for (int i = 0; i < 250; ++i) {
        if (g_bunnyhopPhysicalKeyHookReady.load(std::memory_order_acquire)) {
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
    }
}

void StopBunnyhopPhysicalKeyHook()
{
    bool expected = true;
    if (!g_bunnyhopPhysicalKeyHookRunning.compare_exchange_strong(expected, false, std::memory_order_acq_rel)) {
        return;
    }

    const DWORD threadId = g_bunnyhopPhysicalKeyHookThreadId.load(std::memory_order_acquire);
    if (threadId != 0) {
        PostThreadMessageW(threadId, WM_QUIT, 0, 0);
    }

    if (g_bunnyhopPhysicalKeyHookThread.joinable()) {
        g_bunnyhopPhysicalKeyHookThread.join();
    }

    g_isVk_BunnyhopHeldDown.store(false, std::memory_order_release);
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

bool IsMouseButtonVirtualKey(WORD vk)
{
    return vk >= VK_LBUTTON && vk <= VK_XBUTTON2;
}

bool IsExtendedVirtualKey(WORD vk)
{
    switch (vk) {
    case VK_RCONTROL:
    case VK_RMENU:
    case VK_INSERT:
    case VK_DELETE:
    case VK_HOME:
    case VK_END:
    case VK_PRIOR:
    case VK_NEXT:
    case VK_LEFT:
    case VK_UP:
    case VK_RIGHT:
    case VK_DOWN:
    case VK_NUMLOCK:
        return true;
    default:
        return false;
    }
}

WORD ScanCodeForVirtualKey(WORD vk)
{
    switch (vk) {
    case VK_SHIFT:
    case VK_LSHIFT:
        return 0x2A;
    case VK_RSHIFT:
        return 0x36;
    case VK_CONTROL:
    case VK_LCONTROL:
    case VK_RCONTROL:
        return 0x1D;
    case VK_MENU:
    case VK_LMENU:
    case VK_RMENU:
        return 0x38;
    default:
        return static_cast<WORD>(MapVirtualKeyA(vk, MAPVK_VK_TO_VSC));
    }
}

void SendMouseButtonVirtualKey(WORD vk, bool down)
{
    INPUT input = {};
    input.type = INPUT_MOUSE;
    if (vk == VK_LBUTTON) {
        input.mi.dwFlags = down ? MOUSEEVENTF_LEFTDOWN : MOUSEEVENTF_LEFTUP;
    } else if (vk == VK_RBUTTON) {
        input.mi.dwFlags = down ? MOUSEEVENTF_RIGHTDOWN : MOUSEEVENTF_RIGHTUP;
    } else if (vk == VK_MBUTTON) {
        input.mi.dwFlags = down ? MOUSEEVENTF_MIDDLEDOWN : MOUSEEVENTF_MIDDLEUP;
    } else if (vk == VK_XBUTTON1) {
        input.mi.dwFlags = down ? MOUSEEVENTF_XDOWN : MOUSEEVENTF_XUP;
        input.mi.mouseData = XBUTTON1;
    } else if (vk == VK_XBUTTON2) {
        input.mi.dwFlags = down ? MOUSEEVENTF_XDOWN : MOUSEEVENTF_XUP;
        input.mi.mouseData = XBUTTON2;
    }
    TagInjectedInput(input);
    DispatchTaggedInputs(&input, 1);
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

void HoldVirtualKeyNative(WORD vk, bool extended)
{
    if (IsMouseButtonVirtualKey(vk)) {
        SendMouseButtonVirtualKey(vk, true);
        return;
    }

    const WORD scanCode = ScanCodeForVirtualKey(vk);
    if (scanCode == 0) {
        return;
    }
    HoldKeyNative(scanCode, extended || IsExtendedVirtualKey(vk));
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

void ReleaseVirtualKeyNative(WORD vk, bool extended)
{
    if (IsMouseButtonVirtualKey(vk)) {
        SendMouseButtonVirtualKey(vk, false);
        return;
    }

    const WORD scanCode = ScanCodeForVirtualKey(vk);
    if (scanCode == 0) {
        return;
    }
    ReleaseKeyNative(scanCode, extended || IsExtendedVirtualKey(vk));
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

void MoveMouseNativeRaw(int dx, int dy)
{
    INPUT input = {};
    input.type = INPUT_MOUSE;
    input.mi.dx = dx;
    input.mi.dy = dy;
    input.mi.dwFlags = MOUSEEVENTF_MOVE;
    TagInjectedInput(input);
    DispatchTaggedInputs(&input, 1);
}

void MoveMouseNative(int dx, int dy)
{
    dx = static_cast<int>(static_cast<std::int64_t>(dx) * display_scale / 100);
    dy = static_cast<int>(static_cast<std::int64_t>(dy) * display_scale / 100);
    MoveMouseNativeRaw(dx, dy);
}

bool MoveMouseAbsoluteNative(int x, int y, std::string* errorMessage)
{
    const int left = GetSystemMetrics(SM_XVIRTUALSCREEN);
    const int top = GetSystemMetrics(SM_YVIRTUALSCREEN);
    const int width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
    const int height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
    if (width <= 0 || height <= 0) {
        if (errorMessage) {
            *errorMessage = "virtual desktop bounds are not available in the current Windows session";
        }
        return false;
    }

    const int clampedX = std::clamp(x, left, left + width - 1);
    const int clampedY = std::clamp(y, top, top + height - 1);
    const int widthDenominator = std::max(width - 1, 1);
    const int heightDenominator = std::max(height - 1, 1);
    const LONG normalizedX = static_cast<LONG>(
        std::llround((static_cast<double>(clampedX - left) * 65535.0) / static_cast<double>(widthDenominator)));
    const LONG normalizedY = static_cast<LONG>(
        std::llround((static_cast<double>(clampedY - top) * 65535.0) / static_cast<double>(heightDenominator)));

    INPUT input = {};
    input.type = INPUT_MOUSE;
    input.mi.dx = normalizedX;
    input.mi.dy = normalizedY;
    input.mi.dwFlags = MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK;
    TagInjectedInput(input);
    DispatchTaggedInputs(&input, 1);
    if (errorMessage) {
        errorMessage->clear();
    }
    return true;
}

std::optional<CursorPosition> GetCursorPositionNative()
{
    POINT point = {};
    if (!GetCursorPos(&point)) {
        return std::nullopt;
    }
    return CursorPosition{static_cast<int>(point.x), static_cast<int>(point.y)};
}

std::optional<ScreenBounds> GetScreenBoundsNative()
{
    const int x = GetSystemMetrics(SM_XVIRTUALSCREEN);
    const int y = GetSystemMetrics(SM_YVIRTUALSCREEN);
    const int width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
    const int height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
    if (width <= 0 || height <= 0) {
        return std::nullopt;
    }
    return ScreenBounds{x, y, width, height};
}

std::optional<ScreenBounds> GetActiveMonitorBoundsNative()
{
    POINT point = {};
    if (!GetCursorPos(&point)) {
        return std::nullopt;
    }

    HMONITOR monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
    if (!monitor) {
        return std::nullopt;
    }

    MONITORINFO info = {};
    info.cbSize = sizeof(info);
    if (!GetMonitorInfoA(monitor, &info)) {
        return std::nullopt;
    }

    const int x = static_cast<int>(info.rcMonitor.left);
    const int y = static_cast<int>(info.rcMonitor.top);
    const int width = static_cast<int>(info.rcMonitor.right - info.rcMonitor.left);
    const int height = static_cast<int>(info.rcMonitor.bottom - info.rcMonitor.top);
    if (width <= 0 || height <= 0) {
        return std::nullopt;
    }
    return ScreenBounds{x, y, width, height};
}

std::optional<int> GetActiveMonitorRefreshRateHzNative()
{
    POINT point = {};
    if (!GetCursorPos(&point)) {
        return std::nullopt;
    }

    HMONITOR monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
    if (!monitor) {
        return std::nullopt;
    }

    return RefreshRateForMonitor(monitor);
}

class ScreenPixelSamplerNative {
public:
    ScreenPixelSamplerNative() = default;
    ScreenPixelSamplerNative(const ScreenPixelSamplerNative&) = delete;
    ScreenPixelSamplerNative& operator=(const ScreenPixelSamplerNative&) = delete;

    ~ScreenPixelSamplerNative()
    {
        resetResources();
    }

    std::optional<PixelColor> sample(int x, int y, std::string* errorMessage)
    {
        std::lock_guard<std::mutex> lock(mutex_);
        MonitorCaptureInfo monitorInfo {};
        if (!ResolveMonitorCaptureInfo(x, y, monitorInfo, errorMessage)) {
            return std::nullopt;
        }

        if (!ensureInitialized(monitorInfo, errorMessage)) {
            return std::nullopt;
        }

        if (shouldRefreshFrame(monitorInfo)) {
            if (!captureFrame(monitorInfo, errorMessage)) {
                cacheValid_ = false;
                return std::nullopt;
            }
        }

        return sampleCachedPixel(x, y, errorMessage);
    }

private:
    void resetFrameResources()
    {
        if (memDc_) {
            if (oldBitmap_) {
                SelectObject(memDc_, oldBitmap_);
            }
            DeleteDC(memDc_);
            memDc_ = nullptr;
        }
        if (bitmap_) {
            DeleteObject(bitmap_);
            bitmap_ = nullptr;
        }
        oldBitmap_ = nullptr;
        pixelBits_ = nullptr;
        bitmapWidth_ = 0;
        bitmapHeight_ = 0;
        cachedMonitor_ = nullptr;
        cachedBounds_ = {};
        cacheCaptureTime_ = {};
        cacheRefreshInterval_ = std::chrono::microseconds{0};
        cacheValid_ = false;
    }

    void resetResources()
    {
        resetFrameResources();
    }

    bool ensureInitialized(const MonitorCaptureInfo& monitorInfo, std::string* errorMessage)
    {
        if (memDc_ && bitmap_ && pixelBits_ &&
            bitmapWidth_ == monitorInfo.bounds.width &&
            bitmapHeight_ == monitorInfo.bounds.height) {
            return true;
        }

        HDC screenDc = GetDC(nullptr);
        if (!screenDc) {
            if (errorMessage) {
                *errorMessage = "GetDC failed while preparing screen pixel sampling";
            }
            return false;
        }

        resetFrameResources();

        memDc_ = CreateCompatibleDC(screenDc);
        if (!memDc_) {
            if (errorMessage) {
                *errorMessage = "CreateCompatibleDC failed while preparing screen pixel sampling";
            }
            ReleaseDC(nullptr, screenDc);
            return false;
        }

        BITMAPINFO bmi = {};
        bmi.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
        bmi.bmiHeader.biWidth = monitorInfo.bounds.width;
        bmi.bmiHeader.biHeight = -monitorInfo.bounds.height;
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB;

        bitmap_ = CreateDIBSection(screenDc, &bmi, DIB_RGB_COLORS, &pixelBits_, nullptr, 0);
        if (!bitmap_ || !pixelBits_) {
            if (errorMessage) {
                *errorMessage = "CreateDIBSection failed while preparing screen pixel sampling";
            }
            resetFrameResources();
            ReleaseDC(nullptr, screenDc);
            return false;
        }

        oldBitmap_ = SelectObject(memDc_, bitmap_);
        if (!oldBitmap_ || oldBitmap_ == reinterpret_cast<HGDIOBJ>(HGDI_ERROR)) {
            if (errorMessage) {
                *errorMessage = "SelectObject failed while preparing screen pixel sampling";
            }
            resetFrameResources();
            ReleaseDC(nullptr, screenDc);
            return false;
        }

        bitmapWidth_ = monitorInfo.bounds.width;
        bitmapHeight_ = monitorInfo.bounds.height;
        ReleaseDC(nullptr, screenDc);
        return true;
    }

    bool shouldRefreshFrame(const MonitorCaptureInfo& monitorInfo) const
    {
        if (!cacheValid_) {
            return true;
        }
        if (cachedMonitor_ != monitorInfo.monitor) {
            return true;
        }
        if (cachedBounds_.x != monitorInfo.bounds.x ||
            cachedBounds_.y != monitorInfo.bounds.y ||
            cachedBounds_.width != monitorInfo.bounds.width ||
            cachedBounds_.height != monitorInfo.bounds.height) {
            return true;
        }
        if (cacheRefreshInterval_ != monitorInfo.refreshInterval) {
            return true;
        }

        const auto now = std::chrono::steady_clock::now();
        return (now - cacheCaptureTime_) >= cacheRefreshInterval_;
    }

    bool captureFrame(const MonitorCaptureInfo& monitorInfo, std::string* errorMessage)
    {
        if (!memDc_ || !bitmap_ || !pixelBits_) {
            if (errorMessage) {
                *errorMessage = "screen pixel sampling resources are not ready";
            }
            return false;
        }

        HDC screenDc = GetDC(nullptr);
        if (!screenDc) {
            if (errorMessage) {
                *errorMessage = "GetDC failed while sampling the screen frame";
            }
            cacheValid_ = false;
            return false;
        }

        if (!BitBlt(memDc_, 0, 0, monitorInfo.bounds.width, monitorInfo.bounds.height,
                screenDc, monitorInfo.bounds.x, monitorInfo.bounds.y, SRCCOPY | CAPTUREBLT)) {
            ReleaseDC(nullptr, screenDc);
            if (errorMessage) {
                const DWORD lastError = GetLastError();
                if (lastError != 0) {
                    *errorMessage = "BitBlt failed while sampling the screen frame (GetLastError=" + std::to_string(lastError) + ")";
                } else {
                    *errorMessage = "BitBlt failed while sampling the screen frame";
                }
            }
            cacheValid_ = false;
            return false;
        }

        ReleaseDC(nullptr, screenDc);

        cachedMonitor_ = monitorInfo.monitor;
        cachedBounds_ = monitorInfo.bounds;
        cacheRefreshInterval_ = monitorInfo.refreshInterval;
        cacheCaptureTime_ = std::chrono::steady_clock::now();
        cacheValid_ = true;
        return true;
    }

    std::optional<PixelColor> sampleCachedPixel(int x, int y, std::string* errorMessage) const
    {
        if (!cacheValid_ || !pixelBits_ || bitmapWidth_ <= 0 || bitmapHeight_ <= 0) {
            if (errorMessage) {
                *errorMessage = "screen pixel cache is not ready";
            }
            return std::nullopt;
        }

        const int localX = x - cachedBounds_.x;
        const int localY = y - cachedBounds_.y;
        if (localX < 0 || localY < 0 || localX >= bitmapWidth_ || localY >= bitmapHeight_) {
            if (errorMessage) {
                *errorMessage = "requested pixel is outside the cached monitor bounds";
            }
            return std::nullopt;
        }

        const auto* bytes = static_cast<const unsigned char*>(pixelBits_);
        const std::size_t offset = (static_cast<std::size_t>(localY) * static_cast<std::size_t>(bitmapWidth_) +
            static_cast<std::size_t>(localX)) * 4;
        return PixelColor{bytes[offset + 2], bytes[offset + 1], bytes[offset]};
    }

    mutable std::mutex mutex_;
    HDC memDc_ = nullptr;
    HBITMAP bitmap_ = nullptr;
    HGDIOBJ oldBitmap_ = nullptr;
    void* pixelBits_ = nullptr;
    int bitmapWidth_ = 0;
    int bitmapHeight_ = 0;
    HMONITOR cachedMonitor_ = nullptr;
    ScreenBounds cachedBounds_ {};
    std::chrono::steady_clock::time_point cacheCaptureTime_ {};
    std::chrono::microseconds cacheRefreshInterval_ {0};
    bool cacheValid_ = false;
};

ScreenPixelSamplerNative& GetScreenPixelSamplerNative()
{
    static ScreenPixelSamplerNative sampler;
    return sampler;
}

std::optional<PixelColor> GetPixelColorNative(int x, int y, std::string* errorMessage)
{
    return GetScreenPixelSamplerNative().sample(x, y, errorMessage);
}

class SendInputBackend final : public smu::platform::InputBackend {
public:
    bool init(std::string* errorMessage = nullptr) override
    {
        if (errorMessage) {
            errorMessage->clear();
        }
        StartBunnyhopPhysicalKeyHook();
        return true;
    }

    void shutdown() override
    {
        StopBunnyhopPhysicalKeyHook();
    }

    bool isKeyPressed(PlatformKeyCode key) const override
    {
        return IsKeyPressedNative(static_cast<WORD>(key));
    }

    void holdKey(PlatformKeyCode key, bool extended = false) override
    {
        HoldVirtualKeyNative(static_cast<WORD>(key), extended);
    }

    void releaseKey(PlatformKeyCode key, bool extended = false) override
    {
        ReleaseVirtualKeyNative(static_cast<WORD>(key), extended);
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

    void moveMouseRaw(int dx, int dy) override
    {
        MoveMouseNativeRaw(dx, dy);
    }

    bool moveMouseAbsolute(int x, int y, std::string* errorMessage = nullptr) override
    {
        return MoveMouseAbsoluteNative(x, y, errorMessage);
    }

    std::optional<CursorPosition> getCursorPosition() const override
    {
        return GetCursorPositionNative();
    }

    std::optional<ScreenBounds> getScreenBounds() const override
    {
        return GetScreenBoundsNative();
    }

    std::optional<ScreenBounds> getActiveMonitorBounds() const override
    {
        return GetActiveMonitorBoundsNative();
    }

    std::optional<int> getActiveMonitorRefreshRateHz() const override
    {
        return GetActiveMonitorRefreshRateHzNative();
    }

    std::optional<PixelColor> getPixelColor(int x, int y, std::string* errorMessage = nullptr) const override
    {
        return GetPixelColorNative(x, y, errorMessage);
    }

    std::string screenReadUnavailableReason() const override
    {
        return "screen pixel color sampling is not available in the current Windows session";
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

#define _CRT_SECURE_NO_WARNINGS
#define NOMINMAX
#pragma warning(disable: 4101) // Suppress unreferenced formal parameter warning
#include <windows.h>
#include "resource.h"

#ifndef SMU_USE_SDL_UI
#define SMU_USE_SDL_UI 1
#endif

#include <iostream>
#include <vector>
#include <processthreadsapi.h>
#include <tchar.h>
#include <thread>
#include <chrono>
#include <string>
#include <atomic>
#include <algorithm>
#include <tlhelp32.h>
#include <GL/gl.h>
#include <shlwapi.h>
#include <random>
#include <optional>
#include <memory>

#include "imgui-files/imgui.h"
#include "imgui-files/imgui_impl_opengl3.h"
#if SMU_USE_SDL_UI
#define SDL_MAIN_HANDLED
#include <SDL3/SDL.h>
#include <SDL3/SDL_main.h>
#include <SDL3/SDL_properties.h>
#include <SDL3/SDL_system.h>
#include "imgui-files/imgui_impl_sdl3.h"
#else
#include "imgui-files/imgui_impl_win32.h"
#endif

#include "Resource Files/json.hpp"
#include "Resource Files/globals.h"
#include "Resource Files/miniz.h"
#include "Resource Files/keymapping.h"
#include "Resource Files/wine_compatibility_layer.h"
#include "Resource Files/updater_system.h"
#include "Resource Files/network_manager.h"
#include "Resource Files/macros.h"
#include "Resource Files/profile_manager.h"
#include "Resource Files/theme_manager.h"
#include "Resource Files/overlay.h"
#include "Resource Files/imgui_helpers.h"
#include "../platform/logging.h"
#include "../platform/network_backend.h"
#include "../platform/platform_capabilities.h"

#include <comdef.h>
#include <shlobj.h>
#include <condition_variable>
#include <fstream>
#include <filesystem>
#include <locale>
#include <codecvt>
#include <sstream>
#include <format>
#include <unordered_map>
#include <math.h>
#include <memory.h>
#include <synchapi.h>
#include <dwmapi.h>
#include <variant>
#include <shellscalingapi.h>

// Include windivert
#include "windivert-files/windivert.h"

// Libraries for linking

#pragma comment(lib, "opengl32.lib")
#pragma comment(lib, "ole32.lib")
#pragma comment(lib, "oleaut32.lib")
#pragma comment(lib, "shlwapi.lib")
#pragma comment(lib, "uuid.lib")
#pragma comment(lib, "shell32.lib")
#pragma comment(lib, "Shcore.lib")

using namespace Globals;

// OpenGL Data
static HGLRC g_hRC;
HDC g_hDC;

using PFNWGLSWAPINTERVALEXTPROC = BOOL(WINAPI*)(int);

using json = nlohmann::json;

#if !SMU_USE_SDL_UI
extern IMGUI_IMPL_API LRESULT ImGui_ImplWin32_WndProcHandler(HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam);

LRESULT WINAPI WndProc(HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam);
#endif

// TO PUT IN A KEYBOARD KEY, GO TO https://www.millisecond.com/support/docs/current/html/language/scancodes.htm
// Convert the scancode into hexadecimal before putting it into the HoldKey or ReleaseKey functions
// Ex: E = 18 = 0x12 = HoldKey(0x12)

// If you want to create custom HOTKEYS for stuff that isn't an alphabet/function key, go to https://learn.microsoft.com/en-us/windows/win32/inputdev/virtual-key-codes and get your virtual key code or value

std::atomic<DWORD> keyboardThreadId = 0;

std::mutex renderMutex;
std::condition_variable renderCondVar;
bool renderFlag = false;

HHOOK g_keyboardHook = NULL;

const DWORD SCAN_CODE_FLAGS = KEYEVENTF_SCANCODE;
const DWORD RELEASE_FLAGS = KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP;

INPUT inputkey = {};
INPUT inputhold = {};
INPUT inputrelease = {};

// Window and UI settings
std::string KeyButtonText = "Click to Bind Key";
std::string KeyButtonTextalt = "Click to Bind Key";

// Dropdown options
const char* optionsforoffset[] = {"/e dance2", "/e laugh", "/e cheer"};

// Window and UI state
RECT screen_rect;

// Process and timing state
bool bindingMode = false;
bool bindingModealt = false;
bool wallhopupdate = false;
bool wasMButtonPressed = false;

// Timing and chrono
auto rebindtime = std::chrono::steady_clock::now();
auto suspendStartTime = std::chrono::steady_clock::time_point();

static INPUT createInput()
{
	INPUT inputkey = {};
	inputkey.type = INPUT_KEYBOARD;
	return inputkey;
}

static const char* kForegroundFallbackTooltip =
	"Foreground Roblox window detection is unavailable on this display server. This usually happens on Wayland because apps cannot reliably inspect other apps' active windows. This macro has been forced to always-active mode.";

static const smu::platform::PlatformCapabilities& GetRuntimePlatformCapabilities()
{
	static const smu::platform::PlatformCapabilities capabilities = smu::platform::GetPlatformCapabilities();
	return capabilities;
}

static bool IsForegroundDetectionFallbackActive()
{
	return g_isLinuxWine || !GetRuntimePlatformCapabilities().canDetectForegroundProcess;
}

static bool ForegroundRestrictionAllows(bool disableOutsideRoblox, bool tabbedIntoRoblox)
{
	if (IsForegroundDetectionFallbackActive()) {
		return true;
	}
	return !disableOutsideRoblox || tabbedIntoRoblox;
}

static void MaybeWarnForegroundDetectionFallback()
{
	static bool warned = false;
	if (warned || !IsForegroundDetectionFallbackActive()) {
		return;
	}

	warned = true;
	LogWarning("Foreground-detection-dependent macros were forced into always-active mode because foreground Roblox window detection is unavailable on this display server.");
}

static smu::platform::LagSwitchConfig BuildLagSwitchConfigFromUiState()
{
	smu::platform::LagSwitchConfig config;
	config.enabled = bWinDivertEnabled;
	config.currentlyBlocking = g_windivert_blocking.load(std::memory_order_relaxed);
	config.inboundHardBlock = lagswitchinbound;
	config.outboundHardBlock = lagswitchoutbound;
	config.fakeLagEnabled = lagswitchlag;
	config.inboundFakeLag = lagswitchlaginbound;
	config.outboundFakeLag = lagswitchlagoutbound;
	config.fakeLagDelayMs = lagswitchlagdelay;
	config.targetRobloxOnly = lagswitchtargetroblox;
	config.useUdp = true;
	config.useTcp = lagswitchusetcp;
	config.preventDisconnect = prevent_disconnect;
	config.autoUnblock = lagswitch_autounblock;
	config.maxDurationSeconds = lagswitch_max_duration;
	config.unblockDurationMs = lagswitch_unblock_ms;
	return config;
}

static std::shared_ptr<smu::platform::NetworkLagBackend> GetLagSwitchBackend()
{
	return smu::platform::GetNetworkLagBackend();
}

static void SyncLagSwitchBackendConfig()
{
	if (auto backend = GetLagSwitchBackend()) {
		backend->setConfig(BuildLagSwitchConfigFromUiState());
	}
}

static bool InitializeLagSwitchBackend(std::string* errorMessage = nullptr)
{
	SyncLagSwitchBackendConfig();
	if (auto backend = GetLagSwitchBackend()) {
		return backend->init(errorMessage);
	}
	if (errorMessage) {
		*errorMessage = "Network lagswitch backend is unavailable.";
	}
	return false;
}

static void ShutdownLagSwitchBackend()
{
	if (auto backend = GetLagSwitchBackend()) {
		backend->shutdown();
	}
}

static void RestartLagSwitchCapture()
{
	SyncLagSwitchBackendConfig();
	if (auto backend = GetLagSwitchBackend()) {
		backend->restartCapture();
	}
}

static void SetLagSwitchBlocking(bool active)
{
	if (auto backend = GetLagSwitchBackend()) {
		backend->setBlockingActive(active);
	} else {
		g_windivert_blocking.store(active, std::memory_order_relaxed);
	}
}

static void RenderForegroundDependentCheckbox(const char* label, const char* id, bool* value)
{
	if (!value) {
		return;
	}

	const bool fallbackActive = IsForegroundDetectionFallbackActive();
	bool forcedValue = false;
	bool* checkboxValue = fallbackActive ? &forcedValue : value;

	ImGui::BeginDisabled(fallbackActive);
	ImGui::BeginGroup();
	ImGui::TextWrapped("%s", label);
	ImGui::SameLine();
	ImGui::Checkbox(id, checkboxValue);
	ImGui::EndGroup();
	ImGui::EndDisabled();

	if (fallbackActive && ImGui::IsItemHovered(ImGuiHoveredFlags_AllowWhenDisabled)) {
		ImGui::SetTooltip("%s", kForegroundFallbackTooltip);
	}
}

// This is ran in a separate thread to avoid interfering with other functions

static bool IsTaggedInjectedGuiInputMessage(UINT msg)
{
	switch (msg) {
	case WM_KEYDOWN:
	case WM_KEYUP:
	case WM_SYSKEYDOWN:
	case WM_SYSKEYUP:
	case WM_CHAR:
	case WM_SYSCHAR:
	case WM_MOUSEMOVE:
	case WM_LBUTTONDOWN:
	case WM_LBUTTONUP:
	case WM_LBUTTONDBLCLK:
	case WM_RBUTTONDOWN:
	case WM_RBUTTONUP:
	case WM_RBUTTONDBLCLK:
	case WM_MBUTTONDOWN:
	case WM_MBUTTONUP:
	case WM_MBUTTONDBLCLK:
	case WM_XBUTTONDOWN:
	case WM_XBUTTONUP:
	case WM_XBUTTONDBLCLK:
	case WM_MOUSEWHEEL:
	case WM_MOUSEHWHEEL:
		return GetMessageExtraInfo() == kSmcInjectedInputTag;
	default:
		return false;
	}
}

LRESULT CALLBACK LowLevelKeyboardProc(int nCode, WPARAM wParam, LPARAM lParam) {
    if (nCode == HC_ACTION) {
        const KBDLLHOOKSTRUCT* pkbhs = reinterpret_cast<const KBDLLHOOKSTRUCT*>(lParam);

        // Reset AFK on any key activity seen by the low-level hook.
        isafk.store(false, std::memory_order_relaxed);

        if (pkbhs->vkCode == vk_bunnyhopkey) {
            if ((pkbhs->flags & LLKHF_INJECTED) == 0) {
                // Use relaxed for the bunnyhop flag
                g_isVk_BunnyhopHeldDown.store((wParam & 1) == 0, std::memory_order_relaxed);
            }
        }
    }
    return CallNextHookEx(g_keyboardHook, nCode, wParam, lParam);
}

static void setBunnyHopState(bool state) {
	g_isVk_BunnyhopHeldDown.store(state, std::memory_order_relaxed); // For linux compatibility
}

static void StopKeyboardThread() {
    PostThreadMessage(keyboardThreadId, WM_QUIT, 0, 0);
}

static void KeyboardHookThread()
{
    keyboardThreadId = GetCurrentThreadId();

    HINSTANCE hMod = GetModuleHandle(NULL);
    g_keyboardHook = SetWindowsHookEx(WH_KEYBOARD_LL, LowLevelKeyboardProc, hMod, 0);

    MSG msg;
    while (running && GetMessage(&msg, NULL, 0, 0) > 0) {
        TranslateMessage(&msg);
        DispatchMessage(&msg);
    }

    if (g_keyboardHook) {
        UnhookWindowsHookEx(g_keyboardHook);
        g_keyboardHook = NULL;
    }
}

// Special class to allow mouse scroll events to be found and used
class MouseWheelHandler {
private:
    std::atomic<bool> wheelUp{false};
    std::atomic<bool> wheelDown{false};

    void SetWheelUp() {
        wheelUp.store(true);
        std::thread([this]() {
            std::this_thread::sleep_for(std::chrono::milliseconds(20));
            wheelUp.store(false);
        }).detach();
    }

    void SetWheelDown() {
        wheelDown.store(true);
        std::thread([this]() {
            std::this_thread::sleep_for(std::chrono::milliseconds(20));
            wheelDown.store(false);
        }).detach();
    }

public:
	void ReportWheelDelta(float wheelDelta) {
		if (wheelDelta > 0.0f) {
			SetWheelUp();
		} else if (wheelDelta < 0.0f) {
			SetWheelDown();
		}
	}

#if !SMU_USE_SDL_UI
    // Call this once when GUI initializes
    bool Initialize(HWND windowhandle) {
        RAWINPUTDEVICE rid;
        rid.usUsagePage = 0x01; // Generic desktop
        rid.usUsage = 0x02;     // Mouse
        rid.dwFlags = RIDEV_INPUTSINK;
        rid.hwndTarget = windowhandle; // The GUI window
        
        return RegisterRawInputDevices(&rid, 1, sizeof(rid));
    }

    // Call this from WndProc when you get WM_INPUT
    void ProcessRawInput(LPARAM lParam) {
        UINT dwSize = 0;
        GetRawInputData((HRAWINPUT)lParam, RID_INPUT, nullptr, &dwSize, sizeof(RAWINPUTHEADER));
        
        if (dwSize > 0) {
            LPBYTE lpb = new BYTE[dwSize];
            if (GetRawInputData((HRAWINPUT)lParam, RID_INPUT, lpb, &dwSize, sizeof(RAWINPUTHEADER)) == dwSize) {
                RAWINPUT* raw = (RAWINPUT*)lpb;
                
                if (raw->header.dwType == RIM_TYPEMOUSE) {
                    if (raw->data.mouse.usButtonFlags & RI_MOUSE_WHEEL) {
                        short wheelDelta = (short)raw->data.mouse.usButtonData;
                        if (wheelDelta > 0) {
                            SetWheelUp();
                        } else if (wheelDelta < 0) {
                            SetWheelDown();
                        }
                    }
                }
            }
            delete[] lpb;
        }
    }
#else
    bool Initialize(HWND windowhandle) {
        RAWINPUTDEVICE rid;
        rid.usUsagePage = 0x01;
        rid.usUsage = 0x02;
        rid.dwFlags = RIDEV_INPUTSINK;
        rid.hwndTarget = windowhandle;

        return RegisterRawInputDevices(&rid, 1, sizeof(rid));
    }

    void ProcessRawInput(LPARAM lParam) {
        UINT dwSize = 0;
        GetRawInputData((HRAWINPUT)lParam, RID_INPUT, nullptr, &dwSize, sizeof(RAWINPUTHEADER));

        if (dwSize > 0) {
            LPBYTE lpb = new BYTE[dwSize];
            if (GetRawInputData((HRAWINPUT)lParam, RID_INPUT, lpb, &dwSize, sizeof(RAWINPUTHEADER)) == dwSize) {
                RAWINPUT* raw = (RAWINPUT*)lpb;

                if (raw->header.dwType == RIM_TYPEMOUSE && (raw->data.mouse.usButtonFlags & RI_MOUSE_WHEEL)) {
                    ReportWheelDelta(static_cast<short>(raw->data.mouse.usButtonData));
                }
            }
            delete[] lpb;
        }
    }
#endif

    bool IsWheelUp() const { return wheelUp.load(); }
    bool IsWheelDown() const { return wheelDown.load(); }
};

MouseWheelHandler g_mouseWheel;
#if SMU_USE_SDL_UI
static bool g_mouseWheelRawInputEnabled = false;
#endif

// Tell the other file containg IsKeyPressed that these functions exist 
bool IsWheelUp() { return g_mouseWheel.IsWheelUp(); }
bool IsWheelDown() { return g_mouseWheel.IsWheelDown(); }

bool IsRunAsAdmin() {
    BOOL fIsRunAsAdmin = FALSE;
    PSID pAdminSID = NULL;
    
    // 1. Define the Authority as a variable so we can take its address
    SID_IDENTIFIER_AUTHORITY NtAuthority = SECURITY_NT_AUTHORITY;

    // 2. Use SECURITY_NT_AUTHORITY with DOMAIN_ALIAS_RID_ADMINS
    if (AllocateAndInitializeSid(
        &NtAuthority, 
        2, 
        SECURITY_BUILTIN_DOMAIN_RID, 
        DOMAIN_ALIAS_RID_ADMINS,
        0, 0, 0, 0, 0, 0, 
        &pAdminSID)) 
    {
        // 3. Check if the token is present
        if (!CheckTokenMembership(NULL, pAdminSID, &fIsRunAsAdmin)) {
            fIsRunAsAdmin = FALSE;
        }
        FreeSid(pAdminSID);
    }
    return fIsRunAsAdmin;
}

// Restart the current executable with "runas" (Admin) verb
static void RestartAsAdmin() {
    char szPath[MAX_PATH];
    if (GetModuleFileNameA(NULL, szPath, ARRAYSIZE(szPath))) {
        SHELLEXECUTEINFOA sei = { sizeof(sei) };
        sei.lpVerb = "runas"; // Requests Admin
        sei.lpFile = szPath;
        sei.hwnd = NULL;
        sei.nShow = SW_NORMAL;
        
        if (!ShellExecuteExA(&sei)) {
            // User likely clicked "No" on the UAC prompt
            std::cout << "User refused UAC." << std::endl;
        } else {
            // Success, close the current instance
			done = true;
			running = false;
        }
    }
}

static bool IsMainWindow(HWND hwnd)
{
	return (IsWindowVisible(hwnd) && GetWindow(hwnd, GW_OWNER) == NULL);
}

static std::vector<HWND> FindWindowByProcessHandle(const std::vector<HANDLE> &handles)
{
    std::vector<HWND> windows;
    for (HANDLE hProcess : handles) {
        DWORD targetPID = GetProcessId(hProcess);
        HWND rbxhwnd = FindWindowEx(NULL, NULL, NULL, NULL);
        while (rbxhwnd != NULL) {
            DWORD windowPID = 0;
            GetWindowThreadProcessId(rbxhwnd, &windowPID);
            if (windowPID == targetPID && IsMainWindow(rbxhwnd)) {
                windows.push_back(rbxhwnd);
                break; // Assume one main window per process
            }
            rbxhwnd = FindWindowEx(NULL, rbxhwnd, NULL, NULL);
        }
    }
    return windows;
}

static HWND FindNewestProcessWindow(const std::vector<HWND> &hwnds)
{
    if (hwnds.empty()) {
        return NULL;
    }

    DWORD newestPID = 0;
    ULONGLONG newestCreationTime = 0;
    HWND newestHWND = NULL;
    bool foundAny = false;

    for (HWND hwnd : hwnds) {
        DWORD pid = 0;
        GetWindowThreadProcessId(hwnd, &pid);

        // Open the process to get its creation time
        HANDLE hProcess = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
        if (hProcess) {
            FILETIME ftCreation, ftExit, ftKernel, ftUser;
            if (GetProcessTimes(hProcess, &ftCreation, &ftExit, &ftKernel, &ftUser)) {
                ULONGLONG creationTime = (static_cast<ULONGLONG>(ftCreation.dwHighDateTime) << 32) | ftCreation.dwLowDateTime;
                if ((!foundAny || creationTime > newestCreationTime) && IsMainWindow(hwnd)) {
                    newestCreationTime = creationTime;
                    newestPID = pid;
                    newestHWND = hwnd;
                    foundAny = true;
                }
            }
            CloseHandle(hProcess);
        }
    }

    return foundAny ? newestHWND : NULL;
}

static bool IsForegroundWindowProcess(const std::vector<HANDLE> &handles)
{
    if (IsForegroundDetectionFallbackActive()) {
		return true;
	}

    if (g_isLinuxWine) {
		return true;
	}

    HWND foreground = GetForegroundWindow();
    if (!foreground) return false; // No foreground window

    DWORD foregroundPID = 0;
    GetWindowThreadProcessId(foreground, &foregroundPID);

    for (HANDLE hProcess : handles) {
        DWORD targetPID = GetProcessId(hProcess);
        if (foregroundPID == targetPID) {
            return true; // Found a match
        }
    }
    return false; // No match
}

#if SMU_USE_SDL_UI
static HWND GetNativeWindowHandle(SDL_Window* window)
{
	if (!window) {
		return nullptr;
	}

	SDL_PropertiesID props = SDL_GetWindowProperties(window);
	return reinterpret_cast<HWND>(SDL_GetPointerProperty(props, SDL_PROP_WINDOW_WIN32_HWND_POINTER, nullptr));
}

static void UpdateWindowMetrics(SDL_Window* window, HWND nativeWindow)
{
	if (window) {
		int pixelWidth = 0;
		int pixelHeight = 0;
		if (SDL_GetWindowSizeInPixels(window, &pixelWidth, &pixelHeight) && pixelWidth > 0 && pixelHeight > 0) {
			screen_width = pixelWidth;
			screen_height = pixelHeight;
		} else {
			int windowWidth = 0;
			int windowHeight = 0;
			if (SDL_GetWindowSize(window, &windowWidth, &windowHeight) && windowWidth > 0 && windowHeight > 0) {
				screen_width = windowWidth;
				screen_height = windowHeight;
			}
		}
	}

	if (nativeWindow) {
		RECT raw_screen_rect;
		GetWindowRect(nativeWindow, &raw_screen_rect);
		raw_window_width = raw_screen_rect.right - raw_screen_rect.left;
		raw_window_height = raw_screen_rect.bottom - raw_screen_rect.top;
	}
}

static bool SetGuiWindowTopmost(SDL_Window* window, HWND nativeWindow, bool enabled)
{
	if (window && SDL_SetWindowAlwaysOnTop(window, enabled)) {
		return true;
	}

	if (nativeWindow) {
		return SetWindowPos(nativeWindow, enabled ? HWND_TOPMOST : HWND_NOTOPMOST, 0, 0, 0, 0,
			SWP_NOMOVE | SWP_NOSIZE) != FALSE;
	}

	return false;
}

static bool SetGuiWindowOpacity(SDL_Window* window, HWND nativeWindow, float opacityPercent)
{
	const float opacity = std::clamp(opacityPercent / 100.0f, 0.2f, 1.0f);
	if (window && SDL_SetWindowOpacity(window, opacity)) {
		return true;
	}

	if (nativeWindow) {
		LONG_PTR style = GetWindowLongPtr(nativeWindow, GWL_EXSTYLE);
		SetWindowLongPtr(nativeWindow, GWL_EXSTYLE, style | WS_EX_LAYERED);
		BYTE alpha = static_cast<BYTE>(opacity * 255.0f);
		return SetLayeredWindowAttributes(nativeWindow, 0, alpha, LWA_ALPHA) != FALSE;
	}

	return false;
}

static bool IsSdlWindowFocused(SDL_Window* window)
{
	return window && ((SDL_GetWindowFlags(window) & SDL_WINDOW_INPUT_FOCUS) != 0);
}

static bool SDLCALL SdlWindowsMessageHook(void*, MSG* msg)
{
	if (!msg) {
		return true;
	}

	if (IsTaggedInjectedGuiInputMessage(msg->message)) {
		return false;
	}

	if (msg->message == WM_INPUT) {
		g_mouseWheel.ProcessRawInput(msg->lParam);
	}

	return true;
}
#else
static bool SetGuiWindowTopmost(HWND nativeWindow, bool enabled)
{
	return SetWindowPos(nativeWindow, enabled ? HWND_TOPMOST : HWND_NOTOPMOST, 0, 0, 0, 0,
		SWP_NOMOVE | SWP_NOSIZE) != FALSE;
}

static bool SetGuiWindowOpacity(HWND nativeWindow, float opacityPercent)
{
	const float opacity = std::clamp(opacityPercent / 100.0f, 0.2f, 1.0f);
	LONG_PTR style = GetWindowLongPtr(nativeWindow, GWL_EXSTYLE);
	SetWindowLongPtr(nativeWindow, GWL_EXSTYLE, style | WS_EX_LAYERED);
	BYTE alpha = static_cast<BYTE>(opacity * 255.0f);
	return SetLayeredWindowAttributes(nativeWindow, 0, alpha, LWA_ALPHA) != FALSE;
}
#endif

#if !SMU_USE_SDL_UI
LRESULT CALLBACK WndProc(HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam)
{
    if (IsTaggedInjectedGuiInputMessage(msg)) {
		return 0;
	}

    if (ImGui_ImplWin32_WndProcHandler(hWnd, msg, wParam, lParam))
		return true; // Forward ImGUI-related messages

    switch (msg) {
    case WM_INPUT: {
	    g_mouseWheel.ProcessRawInput(lParam);
	    break;
    }
    case WM_SIZE: {
	    // Get window size
	    GetWindowRect(hWnd, &screen_rect);
	    screen_width = screen_rect.right - screen_rect.left;
	    screen_height = screen_rect.bottom - screen_rect.top;

	    // Update raw screen rects
	    RECT raw_screen_rect;
	    GetWindowRect(hwnd, &raw_screen_rect);
	    raw_window_width = raw_screen_rect.right - raw_screen_rect.left;
	    raw_window_height = raw_screen_rect.bottom - raw_screen_rect.top;
	    return 0;
    }
    case WM_GETMINMAXINFO: {
        MINMAXINFO* mmi = reinterpret_cast<MINMAXINFO*>(lParam);

        // Set the minimum screen size
        mmi->ptMinTrackSize.x = 1147;
        mmi->ptMinTrackSize.y = 780;
        return 0;
    }
    case WM_SYSCOMMAND:
		if ((wParam & 0xfff0) == SC_KEYMENU) // Disable ALT application menu
			return 0;
		break;
    case WM_DESTROY:
	    return 0;
    case WM_CLOSE:
	    done = true;
	    running = false;
		return 0;
    }

    return DefWindowProc(hWnd, msg, wParam, lParam);
}
#endif

static void RenderPlatformCriticalNotifications()
{
	static std::vector<smu::log::LogEntry> activeNotifications;

	auto newNotifications = smu::log::DrainCriticalNotifications();
	activeNotifications.insert(activeNotifications.end(), newNotifications.begin(), newNotifications.end());

	if (activeNotifications.empty()) {
		return;
	}

	ImGui::OpenPopup("Critical Platform Error");
	ImVec2 center = ImGui::GetMainViewport()->GetCenter();
	ImGui::SetNextWindowPos(center, ImGuiCond_Appearing, ImVec2(0.5f, 0.5f));
	ImGui::SetNextWindowSize(ImVec2(560.0f, 0.0f), ImGuiCond_Appearing);

	if (ImGui::BeginPopupModal("Critical Platform Error", nullptr, ImGuiWindowFlags_AlwaysAutoResize)) {
		ImGui::TextWrapped("A required platform feature failed to initialize.");
		ImGui::Separator();
		ImGui::PushTextWrapPos(ImGui::GetCursorPosX() + 520.0f);
		ImGui::TextUnformatted(activeNotifications.front().message.c_str());
		ImGui::PopTextWrapPos();
		ImGui::Separator();

		const bool hasMore = activeNotifications.size() > 1;
		if (ImGui::Button(hasMore ? "Next" : "Dismiss", ImVec2(120, 0))) {
			activeNotifications.erase(activeNotifications.begin());
			if (activeNotifications.empty()) {
				ImGui::CloseCurrentPopup();
			}
		}

		ImGui::EndPopup();
	}
}

static void RenderPlatformWarningNotifications()
{
	static std::vector<smu::log::LogEntry> activeWarnings;

	auto newWarnings = smu::log::DrainWarningNotifications();
	activeWarnings.insert(activeWarnings.end(), newWarnings.begin(), newWarnings.end());

	if (activeWarnings.empty()) {
		return;
	}

	const ImGuiViewport* viewport = ImGui::GetMainViewport();
	ImVec2 pos = ImVec2(viewport->WorkPos.x + viewport->WorkSize.x - 24.0f, viewport->WorkPos.y + 24.0f);
	ImGui::SetNextWindowPos(pos, ImGuiCond_Always, ImVec2(1.0f, 0.0f));
	ImGui::SetNextWindowSize(ImVec2(420.0f, 0.0f), ImGuiCond_Always);
	ImGui::SetNextWindowBgAlpha(0.96f);

	ImGuiWindowFlags flags = ImGuiWindowFlags_NoDecoration |
		ImGuiWindowFlags_NoSavedSettings |
		ImGuiWindowFlags_AlwaysAutoResize |
		ImGuiWindowFlags_NoFocusOnAppearing |
		ImGuiWindowFlags_NoNav;

	if (ImGui::Begin("Platform Warnings", nullptr, flags)) {
		ImGui::TextUnformatted("Warning");
		ImGui::Separator();
		ImGui::PushTextWrapPos(ImGui::GetCursorPosX() + 380.0f);
		ImGui::TextUnformatted(activeWarnings.front().message.c_str());
		ImGui::PopTextWrapPos();

		if (ImGui::Button("Dismiss", ImVec2(100.0f, 0.0f))) {
			activeWarnings.erase(activeWarnings.begin());
		}
		if (activeWarnings.size() > 1) {
			ImGui::SameLine();
			ImGui::Text("%zu more", activeWarnings.size() - 1);
		}
	}
	ImGui::End();
}

bool CreateDeviceWGL(HWND hWnd)
{
	HDC hDc = GetDC(hWnd);
	PIXELFORMATDESCRIPTOR pfd = {0};
	pfd.nSize = sizeof(pfd);
	pfd.nVersion = 1;
	pfd.dwFlags = PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER;
	pfd.iPixelType = PFD_TYPE_RGBA;
	pfd.cColorBits = 32;

	int pf = ChoosePixelFormat(hDc, &pfd);
	if (pf == 0) {
		LogCritical("Failed OpenGL initialization: ChoosePixelFormat returned no compatible format.");
		ReleaseDC(hWnd, hDc);
		return false;
	}
	if (SetPixelFormat(hDc, pf, &pfd) == FALSE) {
		LogCritical("Failed OpenGL initialization: SetPixelFormat failed.");
		ReleaseDC(hWnd, hDc);
		return false;
	}

	g_hRC = wglCreateContext(hDc);
	if (!g_hRC) {
		LogCritical("Failed OpenGL initialization: wglCreateContext failed.");
		ReleaseDC(hWnd, hDc);
		return false;
	}

	g_hDC = hDc;

	return true;
}

void CleanupDeviceWGL(HWND hWnd)
{
	wglMakeCurrent(NULL, NULL);
	if (g_hRC) {
		wglDeleteContext(g_hRC);
		g_hRC = NULL;
	}
	
	if (g_hDC) {  // Use the stored HDC
		ReleaseDC(hWnd, g_hDC);
		g_hDC = NULL;
	}
}

// Define sections

struct Section {
    std::string title;
    std::string description;
    bool optionA; 
    float settingValue; 
};

struct SectionConfig {
    const char *title;
    const char *description;
};

std::vector<Section> sections;

// Title + Description of Sections

static std::array<SectionConfig, section_amounts> SECTION_CONFIGS = {{
    {"Freeze", "Automatically Tab Glitch With a Button"},
    {"Item Desync", "Enable Item Collision (Hold Item Before Pressing)"},
    {"Wall Helicopter High Jump", "Use COM Offset to Catapult Yourself Into The Air by Aligning your Back Angled to the Wall and Jumping and Letting Your Character Turn"},
    {"Speedglitch", "Use COM offset to Massively Increase Midair Speed"},
    {"Item Unequip COM Offset", "Automatically Do a /e dance2 Item COM Offset Where You Unequip the Item"},
    {"Press a Button", "Whenever You Press Your Keybind, it Presses the Other Button for One Frame"},
    {"Wallhop/Rotation", "Automatically Flick and Jump to easily Wallhop On All FPS"},
    {"Walless LHJ", "Lag High Jump Without a Wall by Offsetting COM Downwards or to the Right"},
    {"Item Clip", "Clip through 2-3 Stud Walls Using Gears"},
    {"Laugh Clip", "Automatically Perform a Laugh Clip"},
    {"Wall-Walk", "Walk Across Wall Seams Without Jumping"},
    {"Spam a Key", "Whenever You Press Your Keybind, it Spams the Other Button"},
    {"Ledge Bounce", "Briefly Falls off a Ledge to Then Bounce Off it While Falling"},
	{"Smart Bunnyhop", "Intelligently enables or disables Bunnyhop for any Key"},
	{"Floor Bounce", "Jump much higher from flat ground"},
	{"Lag Switch", "Modify or disable your internet connection temporarily"}
}};

static void InitializeSections()
{
    sections.clear();
    if (shortdescriptions) {
        for (size_t i = 0; i < SECTION_CONFIGS.size(); ++i) {
			sections.push_back({
				SECTION_CONFIGS[i].title, "",
				false, // Fallback Option1
				50.0f  // Fallback Option2
			});
		}
	} else {
		for (size_t i = 0; i < SECTION_CONFIGS.size(); ++i) {
			sections.push_back({
				SECTION_CONFIGS[i].title, SECTION_CONFIGS[i].description,
				false, // Fallback Option1
				50.0f  // Fallback Option2
			});
		}
	}
}

struct BindingState {
    bool bindingMode = false;
    bool notBinding = true;
    std::chrono::steady_clock::time_point rebindTime;
    char keyBuffer[32] = "0x00";
    char keyBufferHuman[32] = "None";
    std::string buttonText = "Click to Bind Key";
    bool keyWasPressed[258] = {false};
    bool firstRun = true;
    int lastSelectedSection = -1;
};

// Global map to store binding states for each key variable
static std::unordered_map<unsigned int *, BindingState> g_bindingStates;

struct InstanceRemoveConfirmState {
	int stage = 0;
	float timer = 0.0f;
	int section_index = -1;
	int target_instance_index = -1;
};

static InstanceRemoveConfirmState g_instance_remove_confirm_state{};

static void ResetInstanceRemoveConfirmState() {
	g_instance_remove_confirm_state = {};
}

static std::string BuildMultiInstanceName(int sectionIndex, int oneBasedIndex) {
	if (sectionIndex >= 0 && sectionIndex < static_cast<int>(sections.size())) {
		return sections[sectionIndex].title + " " + std::to_string(oneBasedIndex);
	}
	return "Instance " + std::to_string(oneBasedIndex);
}

static int GetInstanceRemovalTargetIndex(size_t instanceCount, int selectedIndex) {
	if (instanceCount <= 1) {
		return -1;
	}

	const int lastIndex = static_cast<int>(instanceCount) - 1;
	if (selectedIndex > 0 && selectedIndex <= lastIndex) {
		return selectedIndex;
	}

	return lastIndex;
}

static int GetCurrentRemovalTargetForSection(int sectionIndex) {
	if (sectionIndex == 5) {
		return GetInstanceRemovalTargetIndex(presskey_instances.size(), selected_presskey_instance);
	}
	if (sectionIndex == 6) {
		return GetInstanceRemovalTargetIndex(wallhop_instances.size(), selected_wallhop_instance);
	}
	if (sectionIndex == 11) {
		return GetInstanceRemovalTargetIndex(spamkey_instances.size(), selected_spamkey_instance);
	}

	return -1;
}

static bool* GetDisableOutsideTogglePtr(int sectionIndex) {
	if (sectionIndex < 0 || sectionIndex >= section_amounts || sectionIndex == 15) {
		return nullptr;
	}

	if (sectionIndex == 5) {
		if (presskey_instances.empty() || selected_presskey_instance < 0 || selected_presskey_instance >= static_cast<int>(presskey_instances.size())) {
			return nullptr;
		}
		return &presskey_instances[selected_presskey_instance].presskeyinroblox;
	}

	if (sectionIndex == 6) {
		if (wallhop_instances.empty() || selected_wallhop_instance < 0 || selected_wallhop_instance >= static_cast<int>(wallhop_instances.size())) {
			return nullptr;
		}
		return &wallhop_instances[selected_wallhop_instance].disable_outside_roblox;
	}

	if (sectionIndex == 11) {
		if (spamkey_instances.empty() || selected_spamkey_instance < 0 || selected_spamkey_instance >= static_cast<int>(spamkey_instances.size())) {
			return nullptr;
		}
		return &spamkey_instances[selected_spamkey_instance].disable_outside_roblox;
	}

	return &disable_outside_roblox[sectionIndex];
}

static void CopyWallhopInstanceData(const WallhopInstance& src, WallhopInstance& dst) {
	dst.wallhop_dx = src.wallhop_dx;
	dst.wallhop_dy = src.wallhop_dy;
	dst.wallhop_vertical = src.wallhop_vertical;
	dst.WallhopDelay = src.WallhopDelay;
	dst.WallhopBonusDelay = src.WallhopBonusDelay;
	strncpy_s(dst.WallhopPixels, sizeof(dst.WallhopPixels), src.WallhopPixels, _TRUNCATE);
	strncpy_s(dst.WallhopVerticalChar, sizeof(dst.WallhopVerticalChar), src.WallhopVerticalChar, _TRUNCATE);
	strncpy_s(dst.WallhopDelayChar, sizeof(dst.WallhopDelayChar), src.WallhopDelayChar, _TRUNCATE);
	strncpy_s(dst.WallhopBonusDelayChar, sizeof(dst.WallhopBonusDelayChar), src.WallhopBonusDelayChar, _TRUNCATE);
	strncpy_s(dst.WallhopDegrees, sizeof(dst.WallhopDegrees), src.WallhopDegrees, _TRUNCATE);
	dst.wallhopswitch = src.wallhopswitch;
	dst.toggle_jump = src.toggle_jump;
	dst.toggle_flick = src.toggle_flick;
	dst.wallhopcamfix = src.wallhopcamfix;
	dst.disable_outside_roblox = src.disable_outside_roblox;
	dst.section_enabled = src.section_enabled;
	dst.vk_trigger = src.vk_trigger;
	dst.vk_jumpkey = src.vk_jumpkey;
	dst.thread_active.store(src.thread_active.load(std::memory_order_relaxed), std::memory_order_relaxed);
	dst.should_exit = false;
	dst.isRunning = src.isRunning;
}

static void CopyPresskeyInstanceData(const PresskeyInstance& src, PresskeyInstance& dst) {
	dst.vk_trigger = src.vk_trigger;
	dst.vk_presskey = src.vk_presskey;
	dst.PressKeyDelay = src.PressKeyDelay;
	dst.PressKeyBonusDelay = src.PressKeyBonusDelay;
	strncpy_s(dst.PressKeyDelayChar, sizeof(dst.PressKeyDelayChar), src.PressKeyDelayChar, _TRUNCATE);
	strncpy_s(dst.PressKeyBonusDelayChar, sizeof(dst.PressKeyBonusDelayChar), src.PressKeyBonusDelayChar, _TRUNCATE);
	dst.presskeyinroblox = src.presskeyinroblox;
	dst.section_enabled = src.section_enabled;
	dst.thread_active.store(src.thread_active.load(std::memory_order_relaxed), std::memory_order_relaxed);
	dst.should_exit = false;
	dst.isRunning = src.isRunning;
}

static void CopySpamkeyInstanceData(const SpamkeyInstance& src, SpamkeyInstance& dst) {
	dst.vk_trigger = src.vk_trigger;
	dst.vk_spamkey = src.vk_spamkey;
	dst.spam_delay = src.spam_delay;
	dst.real_delay = src.real_delay;
	strncpy_s(dst.SpamDelay, sizeof(dst.SpamDelay), src.SpamDelay, _TRUNCATE);
	dst.isspamswitch = src.isspamswitch;
	dst.disable_outside_roblox = src.disable_outside_roblox;
	dst.section_enabled = src.section_enabled;
	dst.thread_active.store(src.thread_active.load(std::memory_order_relaxed), std::memory_order_relaxed);
	dst.should_exit = false;
	dst.isRunning = src.isRunning;
}

template <typename T, typename CopyFn>
static void RemoveRequestedInstance(std::deque<T>& instances, std::deque<std::thread>& threads, int requestedIndex, CopyFn copyFn) {
	if (instances.size() <= 1 || threads.empty() || instances.size() != threads.size()) {
		return;
	}

	const int lastIndex = static_cast<int>(instances.size()) - 1;
	int removeIndex = requestedIndex;
	if (removeIndex <= 0 || removeIndex > lastIndex) {
		removeIndex = lastIndex;
	}

	for (int i = removeIndex; i < lastIndex; ++i) {
		copyFn(instances[i + 1], instances[i]);
	}

	instances[lastIndex].thread_active.store(false, std::memory_order_relaxed);
	instances[lastIndex].should_exit = true;
	if (threads[lastIndex].joinable()) {
		threads[lastIndex].join();
	}
	threads.pop_back();
	instances.pop_back();
}

static bool IsHotkeyPressed(unsigned int combinedKey) {
    // Extract the Virtual Key
    unsigned int vk = combinedKey & HOTKEY_KEY_MASK;
    if (vk == 0) return false;

    // Check if the primary key is pressed
    if (!IsKeyPressed(vk)) return false;

    // Check Modifiers
    // We only fail if a REQUIRED modifier is NOT present.
    
    // Win Key Check (Checks either Left or Right Win key)
    if ((combinedKey & HOTKEY_MASK_WIN) && !(IsKeyPressed(VK_LWIN) || IsKeyPressed(VK_RWIN))) return false;
    
    if ((combinedKey & HOTKEY_MASK_CTRL) && !IsKeyPressed(VK_CONTROL)) return false;
    if ((combinedKey & HOTKEY_MASK_ALT) && !IsKeyPressed(VK_MENU)) return false;
    if ((combinedKey & HOTKEY_MASK_SHIFT) && !IsKeyPressed(VK_SHIFT)) return false;

    return true;
}

static void FormatHexKeyString(unsigned int combinedCode, char* buffer, size_t size) {
    unsigned int vk = combinedCode & HOTKEY_KEY_MASK;
    std::string hexStr = "";

    // Append Generic Modifier Hex Codes in Order: Win -> Ctrl -> Alt -> Shift
    // 0x5B = VK_LWIN, 0x11 = VK_CONTROL, 0x12 = VK_MENU, 0x10 = VK_SHIFT
    if (combinedCode & HOTKEY_MASK_WIN)   hexStr += "0x5B + ";
    if (combinedCode & HOTKEY_MASK_CTRL)  hexStr += "0x11 + ";
    if (combinedCode & HOTKEY_MASK_ALT)   hexStr += "0x12 + ";
    if (combinedCode & HOTKEY_MASK_SHIFT) hexStr += "0x10 + ";
    
    char vkHex[16];
    std::snprintf(vkHex, sizeof(vkHex), "0x%X", vk);
    hexStr += vkHex;
    
    // Copy to buffer
    strncpy(buffer, hexStr.c_str(), size - 1);
    buffer[size - 1] = '\0';
}

static void GetKeyNameFromHex(unsigned int combinedKeyCode, char* buffer, size_t bufferSize)
{
    // 1. Check if THIS specific key is currently being bound
    for (auto &[keyPtr, state] : g_bindingStates) {
        if (state.bindingMode && *keyPtr == combinedKeyCode) {
            return; 
        }
    }

    // Clear the buffer
    memset(buffer, 0, bufferSize);

    unsigned int vk = combinedKeyCode & HOTKEY_KEY_MASK;

    // Build Prefix String in Order: Win -> Ctrl -> Alt -> Shift
    std::string prefix = "";
    if (combinedKeyCode & HOTKEY_MASK_WIN)   prefix += "Win + ";
    if (combinedKeyCode & HOTKEY_MASK_CTRL)  prefix += "Ctrl + ";
    if (combinedKeyCode & HOTKEY_MASK_ALT)   prefix += "Alt + ";
    if (combinedKeyCode & HOTKEY_MASK_SHIFT) prefix += "Shift + ";

    char keyNameBuffer[64] = {0};
    UINT scanCode = MapVirtualKey(vk, MAPVK_VK_TO_VSC);

    // Attempt to get the readable key name
    if (GetKeyNameTextA(scanCode << 16, keyNameBuffer, sizeof(keyNameBuffer)) > 0) {
        // Success
    } else {
        // Fallback
        auto it = vkToString.find(vk);
        if (it != vkToString.end()) {
            strncpy(keyNameBuffer, it->second.c_str(), sizeof(keyNameBuffer) - 1);
        } else {
            snprintf(keyNameBuffer, sizeof(keyNameBuffer) - 1, "0x%X", vk);
        }
    }

    // Combine Prefix + KeyName
    std::string finalName = prefix + keyNameBuffer;
    strncpy(buffer, finalName.c_str(), bufferSize - 1);
    buffer[bufferSize - 1] = '\0';
}

static unsigned int BindKeyMode(unsigned int* keyVar, unsigned int currentkey, int selected_section)
{
    BindingState& state = g_bindingStates[keyVar];
    
    if (state.bindingMode) {
        notbinding = false;
        state.rebindTime = std::chrono::steady_clock::now();
        
        if (state.firstRun) {
            for (int key = 1; key < 258; key++) {
                state.keyWasPressed[key] = IsKeyPressed(key);
            }
            state.firstRun = false;
        }

        // 1. Detect currently held modifiers
        unsigned int currentModifiers = 0;
        // Check both Left and Right Win keys
        if (IsKeyPressed(VK_LWIN) || IsKeyPressed(VK_RWIN)) currentModifiers |= HOTKEY_MASK_WIN;
        if (IsKeyPressed(VK_CONTROL)) currentModifiers |= HOTKEY_MASK_CTRL;
        if (IsKeyPressed(VK_MENU))    currentModifiers |= HOTKEY_MASK_ALT;
        if (IsKeyPressed(VK_SHIFT))   currentModifiers |= HOTKEY_MASK_SHIFT;

        // 2. Generate Live Preview Strings (Win -> Ctrl -> Alt -> Shift)
        state.buttonText = "Press a Key...";

        std::string previewHuman = "";
        if (currentModifiers & HOTKEY_MASK_WIN)   previewHuman += "Win + ";
        if (currentModifiers & HOTKEY_MASK_CTRL)  previewHuman += "Ctrl + ";
        if (currentModifiers & HOTKEY_MASK_ALT)   previewHuman += "Alt + ";
        if (currentModifiers & HOTKEY_MASK_SHIFT) previewHuman += "Shift + ";
        
        char previewHex[64] = {0};
        FormatHexKeyString(currentModifiers | 0, previewHex, sizeof(previewHex));
        
        std::string hexStr(previewHex);
        if (currentModifiers != 0 && hexStr.find(" + 0x0") != std::string::npos) {
            hexStr = hexStr.substr(0, hexStr.find(" + 0x0")) + " + ...";
            previewHuman += "...";
        } else if (currentModifiers == 0) {
            hexStr = "Waiting...";
            previewHuman = "Waiting...";
        }

        // 3. Update buffers
        strncpy(state.keyBufferHuman, previewHuman.c_str(), sizeof(state.keyBufferHuman) - 1);
        strncpy(state.keyBuffer, hexStr.c_str(), sizeof(state.keyBuffer) - 1);

        // 4. Check for Primary Key Press
        for (int key = 1; key < 258; key++) {
            // Skip Modifier Keys (Including Win keys)
            if (key == VK_CONTROL || key == VK_LCONTROL || key == VK_RCONTROL ||
                key == VK_SHIFT   || key == VK_LSHIFT   || key == VK_RSHIFT   ||
                key == VK_MENU    || key == VK_LMENU    || key == VK_RMENU    ||
                key == VK_LWIN    || key == VK_RWIN) {
                continue;
            }

            bool currentlyPressed = IsKeyPressed(key);
            if (state.keyWasPressed[key] && !currentlyPressed) {
                state.keyWasPressed[key] = false;
            }
            
            if (currentlyPressed && !state.keyWasPressed[key]) {
                state.bindingMode = false;
                state.firstRun = true;
                state.keyWasPressed[key] = true;

                unsigned int finalCombo = key | currentModifiers;

                GetKeyNameFromHex(finalCombo, state.keyBufferHuman, sizeof(state.keyBufferHuman));
                FormatHexKeyString(finalCombo, state.keyBuffer, sizeof(state.keyBuffer));
                state.buttonText = "Click to Bind Key"; 
                return finalCombo;
            }
            state.keyWasPressed[key] = currentlyPressed;
        }

        // 5. Handle Binding Just a Modifier (on Release)
        // Added LWIN/RWIN to this list
        static const int modifierKeys[] = { 
            VK_LWIN, VK_RWIN, 
            VK_LCONTROL, VK_RCONTROL, 
            VK_LMENU, VK_RMENU, 
            VK_LSHIFT, VK_RSHIFT 
        };

        for (int mod : modifierKeys) {
            bool pressed = IsKeyPressed(mod);
            if (!pressed && state.keyWasPressed[mod]) {
                state.bindingMode = false;
                state.firstRun = true;
                state.keyWasPressed[mod] = false;
                
                unsigned int finalCombo = mod | currentModifiers;
                
                GetKeyNameFromHex(finalCombo, state.keyBufferHuman, sizeof(state.keyBufferHuman));
                FormatHexKeyString(finalCombo, state.keyBuffer, sizeof(state.keyBuffer));
                state.buttonText = "Click to Bind Key";
                return finalCombo;
            }
            state.keyWasPressed[mod] = pressed;
        }

    } else {
        // Not binding state handling
        state.firstRun = true;
        if (selected_section != state.lastSelectedSection || selected_section == -1) {
            FormatHexKeyString(currentkey, state.keyBuffer, sizeof(state.keyBuffer));
            if (selected_section != -1) {
                state.lastSelectedSection = selected_section;
            }
        }
        
        state.buttonText = "Click to Bind Key";
        auto currentTime = std::chrono::steady_clock::now();
        std::chrono::duration<double> elapsedtime = currentTime - state.rebindTime;

        if (elapsedtime.count() >= 0.3) {
            state.notBinding = true;
            notbinding = true;
        }
    }
    
    return currentkey;
}

// Make the Title Bar Black
static bool SetTitleBarColor(HWND hwnd, COLORREF color) {
    BOOL value = TRUE;
    HRESULT hr = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &value, sizeof(value));
    if (FAILED(hr)) {
        return false;
    }
    hr = DwmSetWindowAttribute(hwnd, DWMWA_CAPTION_COLOR, &color, sizeof(color));
    if (FAILED(hr)) {
        return false;
    }
    return true;
}

// Disable windows auto-optimizations
static void DisablePowerThrottling() {
    PROCESS_POWER_THROTTLING_STATE state = {};
    state.Version = PROCESS_POWER_THROTTLING_CURRENT_VERSION;
    state.ControlMask = PROCESS_POWER_THROTTLING_EXECUTION_SPEED |
                        PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION;
    state.StateMask = 0;

    SetProcessInformation(GetCurrentProcess(),
                          ProcessPowerThrottling,
                          &state,
                          sizeof(state));
}

void CheckDisplayScale(HWND hwnd, int display_scale) {
    // Per-Monitor DPI Awareness
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

    UINT dpi = GetDpiForWindow(hwnd);
    int currentScalePercent = (int)(dpi * 100 / USER_DEFAULT_SCREEN_DPI); // 96 DPI base

    if (currentScalePercent != 100 && display_scale != currentScalePercent) {
        std::wstring msg = L"Your display scaling doesn't match the program's settings. "
                           L"Your current display scale is " + std::to_wstring(currentScalePercent) +
                           L"%, the Macro's display scale is " + std::to_wstring(display_scale) +
                           L"%. Make these two equal by either updating the Macro's Settings or your Windows Settings.";

        MessageBox(hwnd,
                   msg.c_str(),
                   L"Display Scaling Mismatch",
                   MB_OK | MB_ICONWARNING);
    }
}

static void SetWorkingDirectoryToExecutablePath() // Allows non-standard execution for save files
{
    wchar_t exePathW[MAX_PATH];
    if (GetModuleFileNameW(nullptr, exePathW, MAX_PATH)) {
        
        // Remove the executable name to get the directory
        wchar_t* lastSlash = wcsrchr(exePathW, L'\\');
        if (lastSlash) {
            *lastSlash = L'\0'; // Terminate the string at the last backslash
            SetCurrentDirectoryW(exePathW);
        }

        // --- Check if running from a temporary directory ---
        wchar_t fullExePathW[MAX_PATH];
        GetModuleFileNameW(nullptr, fullExePathW, MAX_PATH);

        // 1. Get the system's temporary directory
        wchar_t tempDirW[MAX_PATH];
        DWORD tempDirLen = GetTempPathW(MAX_PATH, tempDirW);
        if (tempDirLen > 0 && tempDirLen < MAX_PATH) {
            // Ensure it ends with a backslash for a proper prefix check
            if (tempDirW[tempDirLen - 1] != L'\\') {
                wcscat_s(tempDirW, L"\\");
            }

            // 2. Compare: Is the executable's path inside the temp directory?
            if (_wcsnicmp(fullExePathW, tempDirW, tempDirLen) == 0) {
                // We are running from a temp directory. This is a strong indicator of execution from a ZIP.
                int result = MessageBoxW(
                    nullptr,
                    L"It looks like you're running this program from inside a compressed (ZIP/RAR) file.\n\n"
                    L"This will prevent the program from saving your progress and break multiple features!\n\n"
                    L"Please extract ALL files from the archive first, then run the program from the extracted folder.\n\n"
                    L"Right-click the compressed file → 'Extract All...' or use software like 7-Zip/WinRAR.",
                    L"Warning: Running from Compressed Archive",
                    MB_ICONWARNING | MB_OK | MB_TOPMOST
                );
            }
        }
    }
}

// START OF GUI
static void RunGUI() {
	// Set working directory to correct path
    SetWorkingDirectoryToExecutablePath();

	G_SETTINGS_FILEPATH = ResolveSettingsFilePath();

	// Setup Linux Compatibility Layer if Needed
	InitLinuxCompatLayer();

	// Keep the GUI responsive even while macro worker threads are busy.
	SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL);

	// Load Default Themes into memory to be overwritten
	ThemeManager::Initialize();

	// Snapshot compile-time defaults as the read-only (default) profile (must come before loading any user profile)
	SaveDefaultProfile(G_SETTINGS_FILEPATH);

	// Load last active profile (handles legacy format conversion automatically)
	TryLoadLastActiveProfile(G_SETTINGS_FILEPATH);
	MaybeWarnForegroundDetectionFallback();

#if SMU_USE_SDL_UI
	constexpr int defaultWindowWidth = 1280;
	constexpr int defaultWindowHeight = 800;

	if (screen_width <= 0 || screen_width > 15360) {
		screen_width = defaultWindowWidth;
	}

	if (screen_height <= 0 || screen_height > 8640) {
		screen_height = defaultWindowHeight;
	}

	if (WindowPosX < 0 || WindowPosX > 15360) {
		WindowPosX = 0;
	}

	if (WindowPosY < 0 || WindowPosY > 8640) {
		WindowPosY = 0;
	}

	SDL_SetMainReady();
	if (!SDL_Init(SDL_INIT_VIDEO)) {
		LogCritical(std::string("Failed SDL initialization: ") + SDL_GetError());
		return;
	}

	SDL_GL_SetAttribute(SDL_GL_CONTEXT_PROFILE_MASK, SDL_GL_CONTEXT_PROFILE_COMPATIBILITY);
	SDL_GL_SetAttribute(SDL_GL_CONTEXT_MAJOR_VERSION, 3);
	SDL_GL_SetAttribute(SDL_GL_CONTEXT_MINOR_VERSION, 0);
	SDL_GL_SetAttribute(SDL_GL_DOUBLEBUFFER, 1);
	SDL_GL_SetAttribute(SDL_GL_DEPTH_SIZE, 24);
	SDL_GL_SetAttribute(SDL_GL_STENCIL_SIZE, 8);

	SDL_Window* sdlWindow = SDL_CreateWindow("Spencer Macro Client", screen_width, screen_height,
		SDL_WINDOW_OPENGL | SDL_WINDOW_RESIZABLE | SDL_WINDOW_HIDDEN);
	if (!sdlWindow) {
		LogCritical(std::string("Failed SDL window creation: ") + SDL_GetError());
		SDL_Quit();
		return;
	}

	SDL_SetWindowMinimumSize(sdlWindow, 1147, 780);
	if (WindowPosX != 0 || WindowPosY != 0) {
		SDL_SetWindowPosition(sdlWindow, WindowPosX, WindowPosY);
	}

	SDL_GLContext glContext = SDL_GL_CreateContext(sdlWindow);
	if (!glContext) {
		LogCritical(std::string("Failed OpenGL initialization: SDL_GL_CreateContext failed: ") + SDL_GetError());
		SDL_DestroyWindow(sdlWindow);
		SDL_Quit();
		return;
	}

	if (!SDL_GL_MakeCurrent(sdlWindow, glContext)) {
		LogCritical(std::string("Failed OpenGL initialization: SDL_GL_MakeCurrent failed: ") + SDL_GetError());
		SDL_GL_DestroyContext(glContext);
		SDL_DestroyWindow(sdlWindow);
		SDL_Quit();
		return;
	}

	SDL_GL_SetSwapInterval(0);
	SDL_SetWindowsMessageHook(SdlWindowsMessageHook, nullptr);

	HWND hwnd = GetNativeWindowHandle(sdlWindow);
	::hwnd = hwnd;
	if (hwnd) {
		SendMessage(hwnd, WM_SETICON, ICON_BIG, reinterpret_cast<LPARAM>(LoadIcon(GetModuleHandle(NULL), MAKEINTRESOURCE(IDI_ICON1))));
		SendMessage(hwnd, WM_SETICON, ICON_SMALL, reinterpret_cast<LPARAM>(LoadIcon(GetModuleHandle(NULL), MAKEINTRESOURCE(IDI_ICON1))));
		SetTitleBarColor(hwnd, RGB(0, 0, 0));
		g_mouseWheelRawInputEnabled = g_mouseWheel.Initialize(hwnd);
	}
	UpdateWindowMetrics(sdlWindow, hwnd);

	if (!SetGuiWindowOpacity(sdlWindow, hwnd, windowOpacityPercent)) {
		LogWarning("SDL window opacity could not be applied on this platform.");
	}

	if (ontoptoggle && !SetGuiWindowTopmost(sdlWindow, hwnd, true)) {
		LogWarning("SDL always-on-top could not be applied on this platform.");
	}

	SDL_ShowWindow(sdlWindow);
#else
	// Initialize a basic Win32 window
	WNDCLASSEX wc = { sizeof(WNDCLASSEX), CS_CLASSDC, WndProc, 0L, 0L, GetModuleHandle(NULL), NULL, NULL, NULL, NULL, _T("Spencer Macro Client"), NULL };

	// Load icons
	wc.hIcon = LoadIcon(wc.hInstance, MAKEINTRESOURCE(IDI_ICON1));
	wc.hIconSm = LoadIcon(wc.hInstance, MAKEINTRESOURCE(IDI_ICON1));

	RegisterClassEx(&wc);
	HWND hwnd = CreateWindow(wc.lpszClassName, _T("Spencer Macro Client"), WS_OVERLAPPEDWINDOW, 100, 100, 1280, 800, NULL, NULL, wc.hInstance, NULL);
	::hwnd = hwnd;
	g_mouseWheel.Initialize(hwnd);
	SetTitleBarColor(hwnd, RGB(0, 0, 0));

	// Load Window coordinates and remove invalid coordinates

	if (WindowPosX < 0 || WindowPosX > 15360) {
		WindowPosX = 0;
	}

	if (WindowPosY < 0 || WindowPosY > 8640) {
		WindowPosY = 0;
	}

	if (screen_width < 0 || screen_width > 15360) {
		screen_width = 0;
	}

	if (screen_height < 0 || screen_height > 8640) {
		screen_height = 0;
	}

	if (WindowPosX == 0 && WindowPosY == 0) {
		SetWindowPos(hwnd, NULL, 0, 0, screen_width, screen_height, SWP_NOZORDER | SWP_NOMOVE);
	} else {
		SetWindowPos(hwnd, NULL, WindowPosX, WindowPosY, screen_width, screen_height, SWP_NOZORDER);
	}

	// Get raw screen rect position
	RECT raw_screen_rect;
	GetWindowRect(hwnd, &raw_screen_rect);
	raw_window_width = raw_screen_rect.right - raw_screen_rect.left;
	raw_window_height = raw_screen_rect.bottom - raw_screen_rect.top;

    // Initialize OpenGL
	if (!CreateDeviceWGL(hwnd)) {
		CleanupDeviceWGL(hwnd);
		UnregisterClass(wc.lpszClassName, wc.hInstance);
		return;
	}

	if (!wglMakeCurrent(g_hDC, g_hRC)) {
		LogCritical("Failed OpenGL initialization: wglMakeCurrent failed.");
		CleanupDeviceWGL(hwnd);
		UnregisterClass(wc.lpszClassName, wc.hInstance);
		return;
	}
	PFNWGLSWAPINTERVALEXTPROC wglSwapIntervalEXTProc = reinterpret_cast<PFNWGLSWAPINTERVALEXTPROC>(wglGetProcAddress("wglSwapIntervalEXT"));
	if (wglSwapIntervalEXTProc) {
		wglSwapIntervalEXTProc(0);
	}

	// Show the window
    LONG_PTR style = GetWindowLongPtr(hwnd, GWL_EXSTYLE);
    SetWindowLongPtr(hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED);

    float alphaFraction = windowOpacityPercent / 100.0f;
    BYTE alphaByte = static_cast<BYTE>(alphaFraction * 255.0f);
    SetLayeredWindowAttributes(hwnd, 0, alphaByte, LWA_ALPHA); // Set to user opacity

	if (ontoptoggle) {
		SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
	}

    ShowWindow(hwnd, SW_SHOWDEFAULT);
    UpdateWindow(hwnd);
#endif

    // Initialize ImGui context
    IMGUI_CHECKVERSION();
    ImGui::CreateContext();
    ImGuiIO& io = ImGui::GetIO();
	io.IniFilename = NULL;
    (void)io;
    io.ConfigFlags |= ImGuiConfigFlags_NavEnableKeyboard; // Enable Keyboard Controls

	HRSRC hRes = FindResource(NULL, TEXT("LSANS_TTF"), RT_RCDATA);
    HGLOBAL hMem = LoadResource(NULL, hRes);
	LPVOID pData = LockResource(hMem);
    DWORD size = SizeofResource(NULL, hRes);

	ImFontConfig cfg;
	cfg.FontDataOwnedByAtlas = false;
	ImGui::GetIO().Fonts->AddFontFromMemoryTTF(pData, (int)size, 20.0f, &cfg);

#if SMU_USE_SDL_UI
    if (!ImGui_ImplSDL3_InitForOpenGL(sdlWindow, glContext)) {
		LogCritical("Failed ImGui initialization: SDL3 backend initialization failed.");
	}
#else
    // Initialize ImGui for Win32 and OpenGL
    ImGui_ImplWin32_Init(hwnd);
#endif
	if (!ImGui_ImplOpenGL3_Init("#version 130")) {
		LogCritical("Failed OpenGL initialization: ImGui OpenGL backend initialization failed.");
	}

	constexpr int guiTargetFps = 60;
	const auto targetFrameDuration = std::chrono::duration_cast<std::chrono::steady_clock::duration>(
		std::chrono::duration<double>(1.0 / static_cast<double>(guiTargetFps)));
	auto nextFrameTime = std::chrono::steady_clock::now();
	constexpr int maxMessagesPerIteration = 512;

	InitializeSections();

#if !SMU_USE_SDL_UI
	MSG msg;
#endif

	// Update Specific Variables on startup (Update int values from the corresponding char values)

	#define SAFE_CONVERT_INT(var, src) \
		try { var = std::stoi(src); } catch (...) {}

	#define SAFE_CONVERT_FLOAT(var, src) \
		try { var = std::stof(src); } catch (...) {}

	#define SAFE_CONVERT_DOUBLE(var, src) \
		try { var = static_cast<int>(std::stod(src)); } catch (...) {}

	SAFE_CONVERT_INT(RobloxFPS, RobloxFPSChar);
	SAFE_CONVERT_INT(WallhopDelay, WallhopDelayChar);
	SAFE_CONVERT_INT(WallhopBonusDelay, WallhopBonusDelayChar);
	SAFE_CONVERT_INT(clip_slot, ItemClipSlot);
	SAFE_CONVERT_INT(desync_slot, ItemDesyncSlot);
	SAFE_CONVERT_INT(clip_delay, ItemClipDelay);
	SAFE_CONVERT_INT(RobloxFPS, RobloxFPSChar);
	SAFE_CONVERT_INT(AntiAFKTime, AntiAFKTimeChar);
	SAFE_CONVERT_INT(PressKeyDelay, PressKeyDelayChar);
	SAFE_CONVERT_INT(PressKeyBonusDelay, PressKeyBonusDelayChar);
	SAFE_CONVERT_INT(PasteDelay, PasteDelayChar);
	SAFE_CONVERT_INT(HHJLength, HHJLengthChar);
	SAFE_CONVERT_INT(HHJFreezeDelayOverride, HHJFreezeDelayOverrideChar);
	SAFE_CONVERT_INT(HHJFreezeDelayOverride, HHJFreezeDelayOverrideChar);
	SAFE_CONVERT_INT(HHJDelay1, HHJDelay1Char);
	SAFE_CONVERT_INT(HHJDelay2, HHJDelay2Char);
	SAFE_CONVERT_INT(HHJDelay3, HHJDelay3Char);
	SAFE_CONVERT_INT(FloorBounceDelay1, FloorBounceDelay1Char);
	SAFE_CONVERT_INT(FloorBounceDelay2, FloorBounceDelay2Char);
	SAFE_CONVERT_INT(FloorBounceDelay3, FloorBounceDelay3Char);

	SAFE_CONVERT_DOUBLE(BunnyHopDelay, BunnyHopDelayChar);

	// Special Cases
	try {
		speed_strengthx = std::stoi(RobloxPixelValueChar);
		speed_strengthy = -speed_strengthx;
	} catch (...) {}

	try {
		spam_delay = std::stof(SpamDelay);
		real_delay = static_cast<int>((spam_delay + 0.5f) / 2);
	} catch (...) {}

	// Overwrite default RobloxPlayerBeta to sober if we're on linux
	if (strcmp(settingsBuffer, "RobloxPlayerBeta.exe") == 0 && g_isLinuxWine) {
		std::snprintf(settingsBuffer, sizeof(settingsBuffer), "sober");
	}

	// I have no clue what causes this bug to occur when theres no save file, it saves the default value as this
	if (strcmp(settingsBuffer, "RobloxPlayerddddddBeta.exe") == 0) {
		std::snprintf(settingsBuffer, sizeof(settingsBuffer), "RobloxPlayerBeta.exe");
	}

	// Change pasting delay to be more consistent if we're on linux
	if (strcmp(PasteDelayChar, "1") == 0 && g_isLinuxWine) {
		std::snprintf(PasteDelayChar, sizeof(PasteDelayChar), "35");
		PasteDelay = 35;
		chatoverride = false;
	}

	// Modify HHJ timings to switch to linux if we see regular timings active
	if (HHJDelay1 == 9 && HHJDelay2 == 17 && HHJDelay3 == 16 && g_isLinuxWine) {
		HHJDelay1 = 0;
		std::snprintf(HHJDelay1Char, sizeof(HHJDelay1Char), "0");
		HHJDelay2 = 0;
		std::snprintf(HHJDelay2Char, sizeof(HHJDelay2Char), "0");
		HHJDelay3 = 17;
		std::snprintf(HHJDelay3Char, sizeof(HHJDelay3Char), "17");
	}

    // Attach the GUI thread to the input of the main thread
    DWORD mainThreadId = hwnd ? GetWindowThreadProcessId(hwnd, NULL) : 0;
    DWORD guiThreadId = GetCurrentThreadId();
    if (mainThreadId != 0 && mainThreadId != guiThreadId) {
	    AttachThreadInput(mainThreadId, guiThreadId, TRUE);
	}

    // Set window flags to disable resizing, moving, and title bar
    ImGuiWindowFlags window_flags = ImGuiWindowFlags_NoResize |
                                        ImGuiWindowFlags_NoMove |
                                        ImGuiWindowFlags_NoTitleBar |
										ImGuiWindowFlags_NoScrollbar |
										ImGuiWindowFlags_NoScrollWithMouse |
										ImGuiWindowFlags_NoBringToFrontOnFocus;

	// Check if the display scale is accurate to the macro
	if (hwnd) {
		CheckDisplayScale(hwnd, display_scale);
	}

	bool amIFocused = true;
	bool processFoundOld = false;
	bool lagswitchOld = false;
	int startupWarmupFramesRemaining = 8;

	bool currentBlock = false;

	while (running) {
		// Process all pending messages first
		int processedMessages = 0;
#if SMU_USE_SDL_UI
		SDL_Event event;
		while (processedMessages < maxMessagesPerIteration && SDL_PollEvent(&event)) {
			ImGui_ImplSDL3_ProcessEvent(&event);

			if (event.type == SDL_EVENT_QUIT ||
				event.type == SDL_EVENT_WINDOW_CLOSE_REQUESTED) {
				done = true;
				running = false;
				break;
			}

			if (event.type == SDL_EVENT_WINDOW_RESIZED ||
				event.type == SDL_EVENT_WINDOW_PIXEL_SIZE_CHANGED ||
				event.type == SDL_EVENT_WINDOW_MOVED) {
				UpdateWindowMetrics(sdlWindow, hwnd);
			}

			if (event.type == SDL_EVENT_MOUSE_WHEEL && !g_mouseWheelRawInputEnabled) {
				g_mouseWheel.ReportWheelDelta(event.wheel.y);
			}

			processedMessages++;
		}
#else
		while (processedMessages < maxMessagesPerIteration && PeekMessage(&msg, NULL, 0U, 0U, PM_REMOVE)) {
			if (msg.message == WM_QUIT) {
				running = false;
				break;
			}
			TranslateMessage(&msg);
			DispatchMessage(&msg);
			processedMessages++;
		}
#endif

		if (!running) break;

		// RENDER ONLY IF YOU'RE FOCUSED OR PROCESSFOUND CHANGES
#if SMU_USE_SDL_UI
		amIFocused = IsSdlWindowFocused(sdlWindow);
#else
		if (hwnd) {
			amIFocused = (GetForegroundWindow() == hwnd);
		}
#endif

		if ((processFoundOld != processFound) || (lagswitchOld != g_windivert_blocking)) {
			amIFocused = true;
		}

		processFoundOld = processFound;

		lagswitchOld = g_windivert_blocking;

		if (startupWarmupFramesRemaining > 0) {
			amIFocused = true;
		}

		// Makes sure the log thread is synced to whether lagswitching should target Roblox
		if (lagswitchtargetroblox && g_windivert_running) {
			g_log_thread_running.store(lagswitchtargetroblox);
		}

		if (!amIFocused && !g_isLinuxWine) {
			std::this_thread::sleep_for(std::chrono::milliseconds(200));
			continue; // Skip the rest of this iteration
		}

		// Check if it's time to render
		auto currentTime = std::chrono::steady_clock::now();
		if (currentTime >= nextFrameTime) {
			// Start ImGui frame

			ImGui_ImplOpenGL3_NewFrame();
#if SMU_USE_SDL_UI
			ImGui_ImplSDL3_NewFrame();
#else
			ImGui_ImplWin32_NewFrame();
#endif
			ImGui::NewFrame();
			// Apply the current theme styles every frame
			ThemeManager::ApplyTheme();

            // ImGui window dimensions
            ImVec2 display_size = ImGui::GetIO().DisplaySize;

            // Set the size of the main ImGui window to fill the screen, fitting to the top left
            ImGui::SetNextWindowSize(display_size, ImGuiCond_Always);
            ImGui::SetNextWindowPos(ImVec2(0, 0), ImGuiCond_Always);

            // Create the main window with the specified flags

			// Remove rounding on main window
			ImGui::PushStyleVar(ImGuiStyleVar_WindowRounding, 0);
            ImGui::Begin("Main SMU Window", nullptr, window_flags); // Main ImGui window
			ImGui::PopStyleVar();

			// Manage remove-instance confirmation timer and context consistency.
			if (g_instance_remove_confirm_state.stage != 0) {
				g_instance_remove_confirm_state.timer -= io.DeltaTime;
				bool should_reset = (g_instance_remove_confirm_state.timer <= 0.0f);

				const int currentTarget = GetCurrentRemovalTargetForSection(g_instance_remove_confirm_state.section_index);
				if (currentTarget < 0 || currentTarget != g_instance_remove_confirm_state.target_instance_index) {
					should_reset = true;
				}
				if (selected_section != g_instance_remove_confirm_state.section_index) {
					should_reset = true;
				}

				if (should_reset) {
					ResetInstanceRemoveConfirmState();
				}
			}

			RenderPlatformCriticalNotifications();
			RenderPlatformWarningNotifications();

			// Admin Warning Popup
			if (bShowAdminPopup) {
				ImGui::OpenPopup("Administrator Required");
			}

			// Center the popup
			ImVec2 center = ImGui::GetMainViewport()->GetCenter();
			ImGui::SetNextWindowPos(center, ImGuiCond_Appearing, ImVec2(0.5f, 0.5f));

			if (ImGui::BeginPopupModal("Administrator Required", &bShowAdminPopup, ImGuiWindowFlags_AlwaysAutoResize)) {
				ImGui::Text("WinDivert features require Administrator privileges.");
				ImGui::Text("This process involves:");
				ImGui::BulletText("Extracting SMCWinDivert.dll and WinDivert64.sys to the current folder.");
				ImGui::BulletText("Restarting this application as Administrator.");
        
				ImGui::Separator();
        
				// The Persistent Checkbox
				ImGui::Checkbox("Do not show this again", &DontShowAdminWarning);
        
				ImGui::Separator();

				if (ImGui::Button("Restart as Admin", ImVec2(180, 0))) {
					ImGui::CloseCurrentPopup();
					bShowAdminPopup = false;
					RestartAsAdmin(); // Triggers UAC and closes app
				}
        
				ImGui::SetItemDefaultFocus();
				ImGui::SameLine();
        
				if (ImGui::Button("Cancel", ImVec2(120, 0))) {
					ImGui::CloseCurrentPopup();
					bShowAdminPopup = false;
				}
        
				ImGui::EndPopup();
			}

			// Enable ImGUI Debug Mode
			// ImGui::ShowMetricsWindow();

            // Top settings child window
            float settings_panel_height = 140;
            ImGui::BeginChild("GlobalSettings", ImVec2(display_size.x - 16, settings_panel_height), true);

            // Start of Global Settings
			ImGui::AlignTextToFramePadding();
            ImGui::TextWrapped("Global Settings");
			if (UserOutdated) {
				ImGui::SameLine(135);
				ImGui::PushStyleColor(ImGuiCol_Text, GetCurrentTheme().error_color);
				ImGui::TextWrapped("(OUTDATED VERSION)");
				ImGui::PopStyleColor();
			}

			ImGui::SameLine(ImGui::GetWindowWidth() - 795);
			ImGui::TextWrapped("DISCLAIMER: THIS IS NOT A CHEAT, IT NEVER INTERACTS WITH ROBLOX MEMORY.");

			// Macro Toggle Checkbox
			ImGui::PushStyleColor(ImGuiCol_Text, macrotoggled ? GetCurrentTheme().success_color : GetCurrentTheme().error_color);
			ImGui::AlignTextToFramePadding();
            ImGui::Checkbox("Macro Toggle (Anti-AFK remains!)", &macrotoggled); // Checkbox for toggling
			ImGui::PopStyleColor();

			ImGui::SameLine(ImGui::GetWindowWidth() - 790);
			ImGui::TextWrapped("The ONLY official source for this is");
			ImGui::PushStyleColor(ImGuiCol_Text, GetCurrentTheme().accent_secondary);
			ImGui::SameLine(ImGui::GetWindowWidth() - 499);
			ImGui::TextWrapped("https://github.com/Spencer0187/Spencer-Macro-Utilities");
			if (ImGui::IsItemHovered()) {
				ImGui::SetMouseCursor(ImGuiMouseCursor_Hand);
				if (ImGui::IsItemClicked()) {
						ShellExecuteA(NULL, "open", "https://github.com/Spencer0187/Spencer-Macro-Utilities", NULL, NULL, SW_SHOWNORMAL);
				}
			}

			ImGui::PopStyleColor();
			ImGui::AlignTextToFramePadding();
			ImGui::TextWrapped(g_isLinuxWine ? "Roblox Executable Name/PIDs (Space Separated):" : "Roblox Executable Name:");
			ImGui::SameLine();
			ImGui::SetNextItemWidth(250.0f);

			if (ImGui::InputText("##SettingsTextbox", settingsBuffer, sizeof(settingsBuffer))) {
				TrimWhitespace(settingsBuffer);
			}

			// Button to reset Roblox EXE name
			ImGui::SameLine();
			if (ImGui::Button("R", ImVec2(25, 0))) {
				if (!g_isLinuxWine) {
					std::snprintf(settingsBuffer, sizeof(settingsBuffer), "RobloxPlayerBeta.exe");
				} else {
					std::snprintf(settingsBuffer, sizeof(settingsBuffer), "sober");
				}
			}

			ImGui::SameLine();

			ImDrawList* drawList = ImGui::GetWindowDrawList();
			ImVec2 pos = ImGui::GetCursorScreenPos();
			pos.y += ImGui::GetTextLineHeight() / 2 - 3;
			ImU32 color = processFound ? ImGui::ColorConvertFloat4ToU32(GetCurrentTheme().success_color) : ImGui::ColorConvertFloat4ToU32(GetCurrentTheme().error_color);

			drawList->AddCircleFilled(ImVec2(pos.x + 5, pos.y + 6), 5, color);
			ImGui::Dummy(ImVec2(5 * 2, 5 * 2));

			if (!processFound) {
				ImGui::SameLine();
				ImGui::SetCursorPosY(ImGui::GetCursorPosY() - 1);
				if (!g_isLinuxWine){ImGui::TextWrapped("Roblox Not Found");}
			}

			ImGui::SameLine(ImGui::GetWindowWidth() - 360);
			ImVec2 tooltipcursorpos = ImGui::GetCursorScreenPos();
			ImGui::Text("Toggle Anti-AFK ("); ImGui::SameLine(0, 0);
			ImGui::PushStyleColor(ImGuiCol_Text, GetCurrentTheme().accent_primary);
			ImGui::Text("?"); 
			ImGui::PopStyleColor();
			ImGui::SameLine(0, 0);
			ImGui::Text("):");
			ImGui::SetCursorScreenPos(tooltipcursorpos);
			ImVec2 TextSizeCalc = ImGui::CalcTextSize("Toggle Anti-AFK (?)");
			ImGui::InvisibleButton("##tooltip", TextSizeCalc);
			if (ImGui::IsItemHovered(ImGuiHoveredFlags_AllowWhenDisabled))
				ImGui::SetTooltip("Anti-AFK only works on Windows right now due to Linux restraints.\nAnti-AFK functions by counting up a timer constantly. If you are tabbed into roblox\n"
									"and you press any key on your keyboard, the timer resets.\nIf the timer expires, if the current active window is Roblox, it will press the \"\\\" key\n"
									"two times, which will toggle on and off UI navigation.\n\nIf you are outside of Roblox, it will manually tab into Roblox, and then perform the action.\n"
									"You may change the key anti-afk presses. If you find a less instrusive\nkey that still works against Roblox's AFK timeout, please join and tell the discord.");

			ImGui::SetCursorScreenPos(ImVec2(tooltipcursorpos.x + TextSizeCalc.x, tooltipcursorpos.y));
			ImGui::SameLine(ImGui::GetCursorScreenPos().x + 5);
			ImGui::Checkbox("##AntiAFKToggle", &antiafktoggle);

			ImGui::SameLine(ImGui::GetWindowWidth() - 130);
			ImGui::Text(("VERSION " + localVersion).c_str());

			ImGui::AlignTextToFramePadding();
			ImGui::TextWrapped("Roblox Sensitivity (0-4):");
			ImGui::SameLine();
			ImGui::SetNextItemWidth(70.0f);
			if (ImGui::InputText("##Sens", RobloxSensValue, sizeof(RobloxSensValue), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
				PreviousSensValue = -1;
				// Update Wallhop pixels for ALL instances whenever sensitivity changes
				float sensValue = static_cast<float>(std::atof(RobloxSensValue));
				if (sensValue != 0.0f) {
					for (auto& winst : wallhop_instances) {
						float pixels = static_cast<float>(std::atof(winst.WallhopDegrees)) * (camfixtoggle ? 1000.0f : 720.0f) / (360.0f * sensValue);
						snprintf(winst.WallhopPixels, sizeof(WallhopInstance::WallhopPixels), "%.0f", pixels);
						try {
							winst.wallhop_dx = static_cast<int>(std::round(std::stoi(winst.WallhopPixels)));
							winst.wallhop_dy = -static_cast<int>(std::round(std::stoi(winst.WallhopPixels)));
						} catch (...) {}
					}
				}
			}
			ImGui::SameLine();
			ImGui::TextWrapped("Your Roblox FPS:");
			ImGui::SameLine();
			ImGui::SetNextItemWidth(40.0f);
			if (ImGui::InputText("##FPS", RobloxFPSChar, sizeof(RobloxFPSChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
				try {
					RobloxFPS = std::stoi(RobloxFPSChar);
				} catch (const std::invalid_argument &e) {
				} catch (const std::out_of_range &e) {
				}
			}
			ImGui::SameLine();
			ImGui::Text("Game Uses Cam-Fix:");
			ImGui::SameLine();
			
			if (ImGui::Checkbox("##CamfixToggle", &camfixtoggle) || PreviousSensValue == -1) {
				wallhopupdate = false;
				if (PreviousSensValue != -1) { // Localize wallhop updates only to cam-fix
					wallhopupdate = true;
				}
				PreviousSensValue = -1;

				PreviousWallWalkValue = -1;
				if (wallhopupdate) {
					float factor = camfixtoggle ? 1.388888889f : 1.0f / 1.388888889f;
					for (auto& winst : wallhop_instances) {
						try {
							if (winst.wallhopswitch) {
								winst.wallhop_dx = static_cast<int>(std::round(std::stoi(winst.WallhopPixels) * (camfixtoggle ? -factor : factor)));
								winst.wallhop_dy = static_cast<int>(std::round(std::stoi(winst.WallhopPixels) * (camfixtoggle ? factor : -factor)));
							} else {
								winst.wallhop_dx = static_cast<int>(std::round(std::stoi(winst.WallhopPixels) * factor));
								winst.wallhop_dy = static_cast<int>(std::round(std::stoi(winst.WallhopPixels) * -factor));
								snprintf(winst.WallhopPixels, sizeof(WallhopInstance::WallhopPixels), "%d", winst.wallhop_dx);
							}
						} catch (...) {}
					}
				}

				float CurrentWallWalkValue = static_cast<float>(atof(RobloxSensValue)); // Wallwalk

				float baseValue = camfixtoggle ? 500.0f : 360.0f;
				wallwalk_strengthx = -static_cast<int>(std::round((baseValue / CurrentWallWalkValue) * 0.13f));
				wallwalk_strengthy = static_cast<int>(std::round((baseValue / CurrentWallWalkValue) * 0.13f));

				sprintf(RobloxWallWalkValueChar, "%d", wallwalk_strengthx);

				float CurrentSensValue = static_cast<float>(atof(RobloxSensValue)); // Speedglitch

				try {
					float baseValue = camfixtoggle ? 500.0f : 360.0f;
					float multiplier = (359.0f / 360.0f) * (359.0f / 360.0f);
					RobloxPixelValue = static_cast<int>(std::round((baseValue / CurrentSensValue) * multiplier));
				} catch (const std::invalid_argument &) {
				} catch (const std::out_of_range &) {
				}

				PreviousSensValue = CurrentSensValue;
				sprintf(RobloxPixelValueChar, "%d", RobloxPixelValue);
				try { // Error Handling
					speed_strengthx = std::stoi(RobloxPixelValueChar);
					speed_strengthy = -std::stoi(RobloxPixelValueChar);
				} catch (const std::invalid_argument &e) {
				} catch (const std::out_of_range &e) {
				}

			}

			static bool show_settings_menu = false;

			ImGui::SameLine(ImGui::GetWindowWidth() - 107);
			if (show_settings_menu ? ImGui::Button("Settings <-") : ImGui::Button("Settings ->")) {
				show_settings_menu = !show_settings_menu;
			}

			if (show_settings_menu) {
				// Get the main window size
				ImVec2 main_window_size = ImGui::GetIO().DisplaySize;
				float child_width = main_window_size.x * 0.5f;
				float child_height = main_window_size.y * 0.5f;

				// Calculate position to center the child window
				ImVec2 child_pos = ImVec2(
					(main_window_size.x * 0.4f),
					(main_window_size.y - child_height - 90) * 0.5f
				);

				// Set the next window's position and size
				ImGui::SetNextWindowPos(child_pos, ImGuiCond_Once);
				ImGui::SetNextWindowSize(ImVec2(child_width, child_height), ImGuiCond_Always);

				// Begin the child window (non-draggable)
				ImGuiWindowFlags window_flags = ImGuiWindowFlags_NoResize | ImGuiWindowFlags_NoCollapse;

				if (ImGui::Begin("Settings Menu", &show_settings_menu, window_flags)) {
					// Begin a scrollable child region for the settings list
					ImGui::BeginChild("SettingsList", ImVec2(0, 0), true);

					// Setting: Windows Display Scale
					ImGui::Text("Your Current Windows Display Scale Value (10-500%):");
					ImGui::SetNextItemWidth(150);

					if (ImGui::InputInt("##DisplayScale", &display_scale)) {
						if (display_scale < 10)
							display_scale = 10;
						if (display_scale > 500)
							display_scale = 500;
					}
					ImGui::SameLine();
					ImGui::Text("%%");

					ImGui::Separator();

					BindingState& shiftState = g_bindingStates[&vk_shiftkey];
    
					ImGui::TextWrapped("Custom Shiftlock Key:");
    
					if (ImGui::Button((shiftState.buttonText + "##ShiftKey").c_str())) {
						shiftState.bindingMode = true;
						shiftState.notBinding = false;
						shiftState.buttonText = "Press a Key...";
					}
    
					ImGui::SameLine();
					vk_shiftkey = BindKeyMode(&vk_shiftkey, vk_shiftkey, selected_section);
    
					// Human readable display
					ImGui::SetNextItemWidth(150.0f);
					GetKeyNameFromHex(vk_shiftkey, shiftState.keyBufferHuman, sizeof(shiftState.keyBufferHuman));
					ImGui::InputText("##ShiftKeyHuman", shiftState.keyBufferHuman, sizeof(shiftState.keyBufferHuman), ImGuiInputTextFlags_ReadOnly);
					ImGui::SameLine();
					ImGui::TextWrapped("Key Binding");
    
					// Hexadecimal display
					ImGui::SetNextItemWidth(50.0f);
					ImGui::PushID("ShiftKeyHex");
					ImGui::SameLine();
					ImGui::InputText("##ShiftKeyHex", shiftState.keyBuffer, sizeof(shiftState.keyBuffer), ImGuiInputTextFlags_CharsNoBlank | ImGuiInputTextFlags_CharsHexadecimal | ImGuiInputTextFlags_ReadOnly);
					ImGui::PopID();
					ImGui::SameLine();
					ImGui::TextWrapped("Hex");

					ImGui::Separator();

					ImGui::Checkbox("Force-Set Chat Open Key to \"/\" (Most Stable)", &chatoverride);

					ImGui::Separator();

					BindingState& chatState = g_bindingStates[&vk_chatkey];

					ImGui::TextWrapped("Custom Chat Key (Must disable Force-Set):");

					// Bind button
					if (ImGui::Button((chatState.buttonText + "##ChatKey").c_str())) {
						chatState.bindingMode = true;
						chatState.notBinding = false;
						chatState.buttonText = "Press a Key...";
					}

					ImGui::SameLine();
					vk_chatkey = BindKeyMode(&vk_chatkey, vk_chatkey, selected_section);

					// Human readable display
					ImGui::SetNextItemWidth(150.0f);
					GetKeyNameFromHex(vk_chatkey, chatState.keyBufferHuman, sizeof(chatState.keyBufferHuman));
					ImGui::InputText("##ChatKeyHuman", chatState.keyBufferHuman, sizeof(chatState.keyBufferHuman), ImGuiInputTextFlags_ReadOnly);
					ImGui::SameLine();
					ImGui::TextWrapped("Key Binding");

					// Hexadecimal display
					ImGui::SameLine();
					ImGui::SetNextItemWidth(50.0f);
					ImGui::PushID("ChatKeyHex");
					ImGui::InputText("##ChatKeyHex", chatState.keyBuffer, sizeof(chatState.keyBuffer), 
									 ImGuiInputTextFlags_CharsNoBlank | ImGuiInputTextFlags_CharsHexadecimal | ImGuiInputTextFlags_ReadOnly);
					ImGui::PopID();
					ImGui::SameLine();
					ImGui::TextWrapped("Hex");

					ImGui::Separator();

					ImGui::Checkbox("##Oldpaste", &useoldpaste);
					ImGui::SameLine();
					ImGui::SetCursorPosX(ImGui::GetCursorPosX() - 4);
					ImGui::TextWrapped("Old Unicode Chat-Typing (Works across languages but may be blocked by some anticheats)");

					ImGui::Separator();

					ImGui::AlignTextToFramePadding();
					ImGui::TextWrapped("Delay between every key press in chat (ms):");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(30.0f);
					if (ImGui::InputText("##PasteDelay", PasteDelayChar, sizeof(PasteDelayChar), ImGuiInputTextFlags_CharsDecimal |ImGuiInputTextFlags_CharsNoBlank)) {
						try {
							PasteDelay = std::stoi(PasteDelayChar);
						} catch (const std::invalid_argument &e) {
						} catch (const std::out_of_range &e) {
						}
					}

					ImGui::Separator();

					BindingState& afkState = g_bindingStates[&vk_afkkey];

					ImGui::TextWrapped("Custom Anti-AFK Key (That the macro uses):");

					// Bind button
					if (ImGui::Button((afkState.buttonText + "##Afkkey").c_str())) {
						afkState.bindingMode = true;
						afkState.notBinding = false;
						afkState.buttonText = "Press a Key...";
					}

					ImGui::SameLine();
					vk_afkkey = BindKeyMode(&vk_afkkey, vk_afkkey, selected_section);

					// Human readable display
					ImGui::SetNextItemWidth(150.0f);
					GetKeyNameFromHex(vk_afkkey, afkState.keyBufferHuman, sizeof(afkState.keyBufferHuman));
					ImGui::InputText("##AFKKeyHuman", afkState.keyBufferHuman, sizeof(afkState.keyBufferHuman), ImGuiInputTextFlags_ReadOnly);
					ImGui::SameLine();
					ImGui::TextWrapped("Key Binding");

					// Hexadecimal display
					ImGui::SameLine();
					ImGui::SetNextItemWidth(50.0f);
					ImGui::PushID("AfkkeyHex");
					ImGui::InputText("##AfkkeyHex", afkState.keyBuffer, sizeof(afkState.keyBuffer), ImGuiInputTextFlags_CharsNoBlank | ImGuiInputTextFlags_CharsHexadecimal | ImGuiInputTextFlags_ReadOnly);
					ImGui::PopID();
					ImGui::SameLine();
					ImGui::TextWrapped("Hex");

					ImGui::Separator();

					ImGui::AlignTextToFramePadding();
					ImGui::Text("Amount of Minutes Between Anti-AFK Runs:");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(30.0f);
					if (ImGui::InputText("##AntiAFKTime", AntiAFKTimeChar, sizeof(AntiAFKTimeChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						try {
							AntiAFKTime = std::stoi(AntiAFKTimeChar);
						} catch (const std::invalid_argument &e) {
						} catch (const std::out_of_range &e) {
						}
					}

					ImGui::Separator();

					ImGui::Checkbox("Replace shiftlock with zooming in", &globalzoomin);
					ImGui::SameLine();
					ImGui::Checkbox("Reverse Direction?", &globalzoominreverse);

					ImGui::Separator();

					ImGui::Checkbox("Double-Press AFK keybind during Anti-AFK", &doublepressafkkey);

					ImGui::Separator();
					
					if (ImGui::Checkbox("Remove Side-Bar Macro Descriptions", &shortdescriptions)) {
						InitializeSections();
					}

					ImGui::Separator();

					ImGui::Dummy(ImVec2(0.0f, ImGui::GetTextLineHeight() * 0.5f));

					ImGui::PushStyleColor(ImGuiCol_Text, GetCurrentTheme().accent_primary);
					ImGui::Text("%s", "Want to Donate directly to my Github?");
					if (ImGui::IsItemHovered()) {
						ImGui::SetMouseCursor(ImGuiMouseCursor_Hand);
						if (ImGui::IsItemClicked()) {
							ShellExecuteA(NULL, "open", "https://github.com/sponsors/Spencer0187", NULL, NULL, SW_SHOWNORMAL);
						}
					}
					ImGui::PopStyleColor();

					ImGui::Text("%s", "Github Doesn't take any of the profits.");

					// End the scrollable child region
					ImGui::EndChild();
				}

				ImGui::End();
			}

			ImGui::SameLine(ImGui::GetWindowWidth() - 257);
			if (show_theme_menu ? ImGui::Button("Theme Editor <-") : ImGui::Button("Theme Editor ->")) {
				show_theme_menu = !show_theme_menu;
			}

			ThemeManager::RenderThemeMenu(&show_theme_menu);

            ImGui::EndChild(); // End Global Settings child window

			// Calculate left panel width and height
			float left_panel_width = ImGui::GetWindowSize().x * 0.3f - 23;

			ImGui::BeginChild("LeftScrollSection", ImVec2(left_panel_width, ImGui::GetWindowSize().y - settings_panel_height - 20), true);

			for (size_t display_index = 0; display_index < section_amounts; ++display_index) {

				int i = section_order[display_index]; // Get section index from order array

				ImGui::PushID(i);

				float buttonWidth = left_panel_width - ImGui::GetStyle().FramePadding.x * 2;

				// CUSTOM THEME BEHAVIOR (Programmatic Intensity)
				Globals::Theme& theme = Globals::GetCurrentTheme();
					
				// Helper lambda to brighten a color
				auto Brighten = [](ImVec4 col, float factor) -> ImVec4 {
					return ImVec4(std::min(col.x * factor, 1.0f), 
									std::min(col.y * factor, 1.0f), 
									std::min(col.z * factor, 1.0f), 
									col.w);
				};

				// For duplicatable sections, the main sidebar entry reflects instance 0 only.
				bool sectionActive = section_toggles[i];
				if (i == 5) {
					sectionActive = !presskey_instances.empty() && presskey_instances[0].section_enabled;
				} else if (i == 6) {
					sectionActive = !wallhop_instances.empty() && wallhop_instances[0].section_enabled;
				} else if (i == 11) {
					sectionActive = !spamkey_instances.empty() && spamkey_instances[0].section_enabled;
				}

				if (sectionActive) {
					// ACTIVE (Toggled On) - Use Accent Primary
					if (selected_section == i) {
						// Selected: Make it significantly brighter
						ImGui::PushStyleColor(ImGuiCol_Button,        Brighten(theme.accent_primary, 1.4f));
						ImGui::PushStyleColor(ImGuiCol_ButtonHovered, Brighten(theme.accent_primary, 1.6f));
						ImGui::PushStyleColor(ImGuiCol_ButtonActive,  Brighten(theme.accent_primary, 1.7f));
					} else {
						// Not Selected: Use base accent
						ImGui::PushStyleColor(ImGuiCol_Button,        theme.accent_primary);
						ImGui::PushStyleColor(ImGuiCol_ButtonHovered, Brighten(theme.accent_primary, 1.3f));
						ImGui::PushStyleColor(ImGuiCol_ButtonActive,  Brighten(theme.accent_primary, 0.8f));
					}
				} else {
					// INACTIVE (Toggled Off)
					if (selected_section == i) {
						// Selected: Make it brighter
						ImGui::PushStyleColor(ImGuiCol_Button,        Brighten(theme.disabled_color, 1.6f));
						ImGui::PushStyleColor(ImGuiCol_ButtonHovered, Brighten(theme.disabled_color, 1.9f));
						ImGui::PushStyleColor(ImGuiCol_ButtonActive,  Brighten(theme.disabled_color, 2.1f));
					} else {
						// Not Selected: Use base BG Light
						ImGui::PushStyleColor(ImGuiCol_Button,        theme.disabled_color);
						ImGui::PushStyleColor(ImGuiCol_ButtonHovered, Brighten(theme.disabled_color, 1.3f));
						ImGui::PushStyleColor(ImGuiCol_ButtonActive,  Brighten(theme.disabled_color, 1.1f));
					}
				}

				// Calculate button height based on text
				ImVec2 titleSize = ImGui::CalcTextSize(sections[i].title.c_str(), nullptr, true);
				ImVec2 descriptionSize = ImGui::CalcTextSize(sections[i].description.c_str(), nullptr, true, buttonWidth - 20);
				float buttonHeight = titleSize.y + descriptionSize.y + ImGui::GetStyle().FramePadding.y * 2;


				// Create the button with a custom layout
				if (ImGui::GetScrollMaxY() == 0) {
					if (ImGui::Button("", ImVec2(buttonWidth - 7, buttonHeight))) {
						selected_section = i;
						// Reset instance selection when clicking the section header
						if (i == 5) selected_presskey_instance = 0;
						else if (i == 6) selected_wallhop_instance = 0;
						else if (i == 11) selected_spamkey_instance = 0;
					}
				} else {
					if (ImGui::Button("", ImVec2(buttonWidth - 18, buttonHeight))) {
						selected_section = i;
						if (i == 5) selected_presskey_instance = 0;
						else if (i == 6) selected_wallhop_instance = 0;
						else if (i == 11) selected_spamkey_instance = 0;
					}
				}


				// Drag and Drop Source
				if (ImGui::BeginDragDropSource(ImGuiDragDropFlags_None)) {
					ImGui::SetDragDropPayload("DND_SECTION", &display_index, sizeof(int)); // Dragging by visual index
					ImGui::EndDragDropSource();
				}

				if (ImGui::BeginDragDropTarget()) {
					if (const ImGuiPayload* payload = ImGui::AcceptDragDropPayload("DND_SECTION")) {
						int payload_index = *(const int *)payload->Data;
						std::swap(section_order[payload_index], section_order[display_index]);
					}
					ImGui::EndDragDropTarget();
				}

				// Custom text rendering on buttons

				ImVec2 buttonPos = ImGui::GetItemRectMin();
				ImVec2 textPos = ImVec2(buttonPos.x + ImGui::GetStyle().FramePadding.x, buttonPos.y + ImGui::GetStyle().FramePadding.y);
				ImDrawList* drawList = ImGui::GetWindowDrawList();
				drawList->AddText(textPos, ImGui::ColorConvertFloat4ToU32(GetCurrentTheme().text_primary), sections[i].title.c_str());

				// Wrap and draw description
				std::stringstream ss(sections[i].description);
				std::string word, currentLine;
				textPos.y += titleSize.y;
				while (ss >> word) {
					std::string potentialLine = currentLine + (currentLine.empty() ? "" : " ") + word;
					ImVec2 potentialLineSize = ImGui::CalcTextSize(potentialLine.c_str());

					if (ImGui::GetScrollMaxY() == 0) { // No scrollbar
						if (potentialLineSize.x > buttonWidth - 11) {
							// Draw the current line and move to the next
							drawList->AddText(textPos, ImGui::ColorConvertFloat4ToU32(GetCurrentTheme().text_primary), currentLine.c_str());
							textPos.y += potentialLineSize.y;
							currentLine = word;
						} else {
							currentLine = potentialLine;
						}
					} else {
						if (potentialLineSize.x > buttonWidth - 22) { // Scrollbar
							// Draw the current line and move to the next
							drawList->AddText(textPos, ImGui::ColorConvertFloat4ToU32(GetCurrentTheme().text_primary), currentLine.c_str());
							textPos.y += potentialLineSize.y;
							currentLine = word;
						} else {
							currentLine = potentialLine;
						}
					}
				}

				if (!currentLine.empty()) {
					drawList->AddText(textPos, ImGui::ColorConvertFloat4ToU32(GetCurrentTheme().text_primary), currentLine.c_str());
				}

				ImGui::PopStyleColor(3);
				ImGui::PopID();

				// ---- Sub-buttons for duplicatable sections ----
				if (i == 5 || i == 6 || i == 11) {
					float indentW = 0.0f;
					float subBtnW = (ImGui::GetScrollMaxY() == 0) ? (buttonWidth - 7 - indentW) : (buttonWidth - 18 - indentW);

					// Determine instance count and which instance is currently selected
					size_t instCount = (i == 5) ? presskey_instances.size()
					                 : (i == 6) ? wallhop_instances.size()
					                 :             spamkey_instances.size();
					int& selInst    = (i == 5) ? selected_presskey_instance
					                : (i == 6) ? selected_wallhop_instance
					                :             selected_spamkey_instance;

					// Show sub-button for instances 1+  (instance 0 = the section header above)
					for (size_t j = 1; j < instCount; ++j) {
						ImGui::PushID(static_cast<int>(j) * 1000 + i);
						ImGui::SetCursorPosX(ImGui::GetCursorPosX() + indentW);

						bool instEnabled = (i == 5) ? presskey_instances[j].section_enabled
						                : (i == 6) ? wallhop_instances[j].section_enabled
						                :             spamkey_instances[j].section_enabled;

						if (instEnabled) {
							bool isSelJ = (selected_section == i && selInst == (int)j);
							if (isSelJ) ImGui::PushStyleColor(ImGuiCol_Button, Brighten(theme.accent_primary, 1.4f));
							else        ImGui::PushStyleColor(ImGuiCol_Button, theme.accent_primary);
						} else {
							bool isSelJ = (selected_section == i && selInst == (int)j);
							if (isSelJ) ImGui::PushStyleColor(ImGuiCol_Button, Brighten(theme.disabled_color, 1.6f));
							else        ImGui::PushStyleColor(ImGuiCol_Button, theme.disabled_color);
						}
						ImGui::PushStyleColor(ImGuiCol_ButtonHovered, Brighten(theme.accent_primary, 1.3f));
						ImGui::PushStyleColor(ImGuiCol_ButtonActive,  Brighten(theme.accent_primary, 0.8f));

						std::string subLabel = BuildMultiInstanceName(i, static_cast<int>(j) + 1);
						if (ImGui::Button(subLabel.c_str(), ImVec2(subBtnW, 0))) {
							selected_section = i;
							selInst = static_cast<int>(j);
						}
						ImGui::PopStyleColor(3);
						ImGui::PopID();
					}

					// '+' button to add a new instance
					ImGui::PushID(9999 + i);
					ImGui::SetCursorPosX(ImGui::GetCursorPosX() + indentW);
					ImGui::PushStyleColor(ImGuiCol_Button,        theme.bg_light);
					ImGui::PushStyleColor(ImGuiCol_ButtonHovered, Brighten(theme.bg_light, 1.3f));
					ImGui::PushStyleColor(ImGuiCol_ButtonActive,  Brighten(theme.bg_light, 1.1f));
					if (ImGui::Button("+", ImVec2(subBtnW, 0))) {
						if (i == 5)  request_new_presskey_instance = true;
						else if (i == 6) request_new_wallhop_instance  = true;
						else if (i == 11) request_new_spamkey_instance = true;
					}
					if (ImGui::IsItemHovered()) ImGui::SetTooltip("Add a new independent instance of this macro");
					ImGui::PopStyleColor(3);
					ImGui::PopID();
				}

				ImGui::Separator();
			}

			ImGui::EndChild();

            // Right section
            ImGui::SameLine(); // Move to the right of the left section

			ImVec2 rightSectionSize(
				display_size.x - 23 - left_panel_width,
				display_size.y - settings_panel_height - 20 - 30
			);

            // Right child window with dynamic sizing
			ImGui::BeginChild("RightSection", rightSectionSize, true);

			// Display different content based on the selected section
			if (selected_section >= 0 && selected_section < sections.size()) {
				// For duplicatable sections (5, 6, 11) use the selected instance's trigger key;
				// for all other sections use the shared section_to_key map.
				unsigned int* currentKey = nullptr;
				bool* instanceEnabledPtr = nullptr;
				if (selected_section == 5 && !presskey_instances.empty()) {
					auto& inst = presskey_instances[selected_presskey_instance];
					currentKey         = &inst.vk_trigger;
					instanceEnabledPtr = &inst.section_enabled;
				} else if (selected_section == 6 && !wallhop_instances.empty()) {
					auto& inst = wallhop_instances[selected_wallhop_instance];
					currentKey         = &inst.vk_trigger;
					instanceEnabledPtr = &inst.section_enabled;
				} else if (selected_section == 11 && !spamkey_instances.empty()) {
					auto& inst = spamkey_instances[selected_spamkey_instance];
					currentKey         = &inst.vk_trigger;
					instanceEnabledPtr = &inst.section_enabled;
				} else {
					currentKey = section_to_key.at(selected_section);
				}
				BindingState& state = g_bindingStates[currentKey];
    
				// Display section title and keybind UI
				if (selected_section == 5 && presskey_instances.size() > 1) {
					std::string activeLabel = BuildMultiInstanceName(selected_section, selected_presskey_instance + 1);
					ImGui::TextWrapped("Settings for %s", activeLabel.c_str());
				} else if (selected_section == 6 && wallhop_instances.size() > 1) {
					std::string activeLabel = BuildMultiInstanceName(selected_section, selected_wallhop_instance + 1);
					ImGui::TextWrapped("Settings for %s", activeLabel.c_str());
				} else if (selected_section == 11 && spamkey_instances.size() > 1) {
					std::string activeLabel = BuildMultiInstanceName(selected_section, selected_spamkey_instance + 1);
					ImGui::TextWrapped("Settings for %s", activeLabel.c_str());
				} else {
					ImGui::TextWrapped("Settings for %s", sections[selected_section].title.c_str());
				}
				ImGui::Separator();
				ImGui::NewLine();
				ImGui::TextWrapped("Keybind:");
				ImGui::SameLine();

				// Keybind button - use the state's buttonText
				if (ImGui::Button(state.buttonText.c_str())) {
					state.bindingMode = true;
					state.notBinding = false;
					state.buttonText = "Press a Key...";
				}

				ImGui::SameLine();

				// Handle key bindings for all sections
				*currentKey = BindKeyMode(currentKey, *currentKey, selected_section);
				ImGui::SetNextItemWidth(170.0f);
				GetKeyNameFromHex(*currentKey, state.keyBufferHuman, sizeof(state.keyBufferHuman));

				// Use the state's keyBuffer for display
				ImGui::InputText("##KeyBufferHuman", state.keyBufferHuman, sizeof(state.keyBufferHuman), ImGuiInputTextFlags_CharsNoBlank | ImGuiInputTextFlags_ReadOnly);
				ImGui::SameLine();
				ImGui::TextWrapped("Key Binding");
				ImGui::SameLine();
				ImGui::SetNextItemWidth(130.0f);

				ImGui::InputText("##KeyBuffer", state.keyBuffer, sizeof(state.keyBuffer), ImGuiInputTextFlags_CharsNoBlank | ImGuiInputTextFlags_CharsHexadecimal | ImGuiInputTextFlags_ReadOnly);

				ImGui::SameLine();
				ImGui::TextWrapped("(Hexadecimal)");

				// Enable/disable toggle — per-instance for sections 5, 6, 11
				bool toggleVal = instanceEnabledPtr ? *instanceEnabledPtr : section_toggles[selected_section];
				ImGui::PushStyleColor(ImGuiCol_Text, toggleVal ? GetCurrentTheme().success_color : GetCurrentTheme().error_color);
				ImGui::TextWrapped("Enable This Macro:");
				ImGui::PopStyleColor();
				ImGui::SameLine();
				if (selected_section >= 0 && selected_section < section_amounts) {
					if (instanceEnabledPtr) {
						ImGui::Checkbox(("##SectionToggle" + std::to_string(selected_section)).c_str(), instanceEnabledPtr);
					} else {
						ImGui::Checkbox(("##SectionToggle" + std::to_string(selected_section)).c_str(),
										&section_toggles[selected_section]);
					}
				}

				bool* disableOutsidePtr = GetDisableOutsideTogglePtr(selected_section);
				if (disableOutsidePtr) {
					std::string disableOutsideId = "##DisableOutsideRoblox_" + std::to_string(selected_section);
					if (selected_section == 5) {
						disableOutsideId += "_" + std::to_string(selected_presskey_instance);
					} else if (selected_section == 6) {
						disableOutsideId += "_" + std::to_string(selected_wallhop_instance);
					} else if (selected_section == 11) {
						disableOutsideId += "_" + std::to_string(selected_spamkey_instance);
					}

					ImGui::SameLine();
					RenderForegroundDependentCheckbox("Disable outside of Roblox:", disableOutsideId.c_str(), disableOutsidePtr);
				}

				if (selected_section == 0) { // Freeze Macro
					ImGui::TextWrapped("Automatically Unfreeze after this amount of seconds (Anti-Internet-Kick)");
					ImGui::SetNextItemWidth(60.0f);
					ImGui::InputFloat("##FreezeFloat", &maxfreezetime, 0.0f, 0.0f, "%.2f");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(300.0f);
					ImGui::SliderFloat("##FreezeSlider", &maxfreezetime, 0.0f, 9.8f, "%.2f Seconds");

					char maxfreezeoverrideBuffer[16];
					std::snprintf(maxfreezeoverrideBuffer, sizeof(maxfreezeoverrideBuffer), "%d", maxfreezeoverride);

					ImGui::SetNextItemWidth(50.0f);
					if (ImGui::InputText("Modify 50ms Default Unfreeze Time (MS)", maxfreezeoverrideBuffer, sizeof(maxfreezeoverrideBuffer), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						maxfreezeoverride = std::atoi(maxfreezeoverrideBuffer);
					}

					ImGui::Checkbox("Switch from Hold Key to Toggle Key", &isfreezeswitch);
					if (isfreezeswitch || takeallprocessids) {
						disable_outside_roblox[0] = false;
					}

					ImGui::Checkbox("Freeze all Found Processes Instead of Newest", &takeallprocessids);

					ImGui::SameLine();
					ImGui::TextWrapped("(ONLY EVER USE FOR COMPATIBILITY ISSUES WITH NON-ROBLOX GAMES)");
					ImGui::Separator();
					ImGui::TextWrapped("Explanation:");
					ImGui::NewLine();
					ImGui::TextWrapped("Hold the hotkey to freeze your game, let go of it to release it. Suspending your game also pauses "
										"ALL network and physics activity that the server sends or recieves from you.");

				}

				if (selected_section == 1) { // Item Desync
					ImGui::TextWrapped("Gear Slot:");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(30.0f);
					ImGui::InputText("##ItemDesync", ItemDesyncSlot, sizeof(ItemDesyncSlot), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank);
					try {
						desync_slot = std::stoi(ItemDesyncSlot);
						SetDesyncItem(desync_slot);
					} catch (const std::invalid_argument &e) {
					} catch (const std::out_of_range &e) {
					}
					ImGui::Separator();
					ImGui::TextWrapped("Equip your item inside of the slot you have picked here, then hold the keybind for 4-7 seconds");
					ImGui::Separator();
					ImGui::TextWrapped("Explanation:");
					ImGui::NewLine();
					ImGui::TextWrapped("This Macro rapidly sends number inputs to your roblox client, enough that the server begins to throttle "
										"you. The item that you're holding must not make a serverside sound, else desyncing yourself will be "
										"very buggy, and you will be unable to send any physics data to the server. Once you have desynced, "
										"the server will assume you're not holding an item, but your client will, which permanently enables "
										"client-side collision on the item.");
					ImGui::Separator();
					ImGui::TextWrapped(
									"If 'Disable outside of Roblox' is enabled, this module will only run while tabbed into Roblox. "
									"Turning it off allows use in other windows (use carefully). ");
				}

				if (selected_section == 2) { // HHJ
					ImGui::Checkbox("Automatically time inputs", &autotoggle);
					ImGui::SameLine();
					ImGui::TextWrapped("(EXTREMELY BUGGY/EXPERIMENTAL, WORKS BEST ON HIGH FPS AND SHALLOW ANGLE TO WALL)");
					// Speedrunner mode: decrease freeze duration
					ImGui::Checkbox("Decrease Freeze Duration (Speedrunner Mode)", &fasthhj);
					if (ImGui::Button("R##HHJLength", ImVec2(25, 0))) {
						HHJLength = 243;
						sprintf(HHJLengthChar, "%d", HHJLength);
					}
					ImGui::SameLine();
					ImGui::TextWrapped("Length of HHJ flicks (ms):");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(52);
					if (ImGui::InputText("##HHJLength", HHJLengthChar, sizeof(HHJLengthChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						try {
							HHJLength = std::stoi(HHJLengthChar);
						} catch (const std::invalid_argument &e) {
						} catch (const std::out_of_range &e) {
						}
					}

					if (ImGui::CollapsingHeader("Advanced HHJ Options", showadvancedhhj ? ImGuiTreeNodeFlags_DefaultOpen : 0))
					{
						showadvancedhhj = true;

						ImGui::Spacing();
						ImGui::Indent();
						if (ImGui::Button("R##HHJFreezeDelayOverride", ImVec2(25, 0))) {
							HHJFreezeDelayOverride = 500;
							sprintf(HHJFreezeDelayOverrideChar, "%d", HHJFreezeDelayOverride);
							HHJFreezeDelayApply = false;
						}
						ImGui::SameLine();
						ImGui::TextWrapped("Set freeze delay (ms): ");
						ImGui::SameLine();
						ImGui::SetNextItemWidth(50);
						if (ImGui::InputText("##HHJFreezeDelayOverride", HHJFreezeDelayOverrideChar, sizeof(HHJFreezeDelayOverrideChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
							try { HHJFreezeDelayOverride = std::stoi(HHJFreezeDelayOverrideChar); } catch (...) { }
						}
						ImGui::SameLine();
						ImGui::Checkbox("Apply##HHJFreezeDelayApply", &HHJFreezeDelayApply);
						ImGui::Unindent();

						ImGui::Indent();
						if (ImGui::Button("R##HHJDelay1", ImVec2(25, 0))) {
							HHJDelay1 = 9;
							sprintf(HHJDelay1Char, "%d", HHJDelay1);
						}
						ImGui::SameLine();
						ImGui::TextWrapped("Delay after freezing before shiftlock is held (ms): ");
						ImGui::SameLine();
						ImGui::SetNextItemWidth(50);
						if (ImGui::InputText("##HHJDelay1", HHJDelay1Char, sizeof(HHJDelay1Char), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
							try { HHJDelay1 = std::stoi(HHJDelay1Char); } catch (...) { }
						}
						ImGui::Unindent();

						ImGui::Indent();
						if (ImGui::Button("R##HHJDelay2", ImVec2(25, 0))) {
							HHJDelay2 = 17;
							sprintf(HHJDelay2Char, "%d", HHJDelay2);
						}
						ImGui::SameLine();
						ImGui::TextWrapped("Time shiftlock is held before spinning (ms): ");
						ImGui::SameLine();
						ImGui::SetNextItemWidth(50);
						if (ImGui::InputText("##HHJDelay2", HHJDelay2Char, sizeof(HHJDelay2Char), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
							try { HHJDelay2 = std::stoi(HHJDelay2Char); } catch (...) { }
						}
						ImGui::Unindent();

						ImGui::Indent();
						if (ImGui::Button("R##HHJDelay3", ImVec2(25, 0))) {
							HHJDelay3 = 16;
							sprintf(HHJDelay3Char, "%d", HHJDelay3);
						}
						ImGui::SameLine();
						ImGui::TextWrapped("Time shiftlock is held after freezing (ms): ");
						ImGui::SameLine();
						ImGui::SetNextItemWidth(50);
						if (ImGui::InputText("##HHJDelay3", HHJDelay3Char, sizeof(HHJDelay3Char), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
							try { HHJDelay3 = std::stoi(HHJDelay3Char); } catch (...) { }
						}
						ImGui::Unindent();
					}

					if (ImGui::CollapsingHeader("Customize Automatic HHJ", showautomatichhj ? ImGuiTreeNodeFlags_DefaultOpen : 0))
					{
						showautomatichhj = true;

						ImGui::Spacing();
						ImGui::TextWrapped("First Key Press:");
						ImGui::Indent();

						BindingState& autohhjkey1State = g_bindingStates[&vk_autohhjkey1];

						if (ImGui::Button("R##AutoHHJKey1Reset", ImVec2(25, 0))) {
							vk_autohhjkey1 = VK_SPACE;
							FormatHexKeyString(vk_autohhjkey1, autohhjkey1State.keyBuffer, sizeof(autohhjkey1State.keyBuffer));
							GetKeyNameFromHex(vk_autohhjkey1, autohhjkey1State.keyBufferHuman, sizeof(autohhjkey1State.keyBufferHuman));
							autohhjkey1State.buttonText = "Click to Bind Key";
						}
						ImGui::SameLine();

						if (ImGui::Button((autohhjkey1State.buttonText + "##AutoHHJKey1").c_str())) {
							autohhjkey1State.bindingMode = true;
							autohhjkey1State.notBinding = false;
							autohhjkey1State.buttonText = "Press a Key...";
						}
						ImGui::SameLine();
						vk_autohhjkey1 = BindKeyMode(&vk_autohhjkey1, vk_autohhjkey1, selected_section);
						
						ImGui::SetNextItemWidth(150.0f);
						GetKeyNameFromHex(vk_autohhjkey1, autohhjkey1State.keyBufferHuman, sizeof(autohhjkey1State.keyBufferHuman));
						ImGui::InputText("##AutoHHJKey1Human", autohhjkey1State.keyBufferHuman, sizeof(autohhjkey1State.keyBufferHuman), ImGuiInputTextFlags_ReadOnly);
						ImGui::SameLine();
						ImGui::TextWrapped("Key");
						
						ImGui::SetNextItemWidth(50.0f);
						ImGui::SameLine();
						ImGui::PushID("AutoHHJKey1Hex");
						ImGui::InputText("##AutoHHJKey1Hex", autohhjkey1State.keyBuffer, sizeof(autohhjkey1State.keyBuffer), ImGuiInputTextFlags_CharsNoBlank | ImGuiInputTextFlags_CharsHexadecimal | ImGuiInputTextFlags_ReadOnly);
						ImGui::PopID();
						ImGui::SameLine();
						ImGui::TextWrapped("Hex");
						
						if (ImGui::Button("R##AutoHHJKey1Time", ImVec2(25, 0))) {
							AutoHHJKey1Time = 550;
							sprintf(AutoHHJKey1TimeChar, "%d", AutoHHJKey1Time);
						}
						ImGui::SameLine();
						ImGui::TextWrapped("Hold time (ms): ");
						ImGui::SameLine();
						ImGui::SetNextItemWidth(50);
						if (ImGui::InputText("##AutoHHJKey1Time", AutoHHJKey1TimeChar, sizeof(AutoHHJKey1TimeChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
							try { AutoHHJKey1Time = std::stoi(AutoHHJKey1TimeChar); } catch (...) { }
						}
						ImGui::Unindent();

						ImGui::Spacing();
						ImGui::TextWrapped("Second Key Press:");
						ImGui::Indent();

						BindingState& autohhjkey2State = g_bindingStates[&vk_autohhjkey2];

						if (ImGui::Button("R##AutoHHJKey2Reset", ImVec2(25, 0))) {
							vk_autohhjkey2 = VkKeyScanEx('W', GetKeyboardLayout(0)) & 0xFF;
							FormatHexKeyString(vk_autohhjkey2, autohhjkey2State.keyBuffer, sizeof(autohhjkey2State.keyBuffer));
							GetKeyNameFromHex(vk_autohhjkey2, autohhjkey2State.keyBufferHuman, sizeof(autohhjkey2State.keyBufferHuman));
							autohhjkey2State.buttonText = "Click to Bind Key";
						}
						ImGui::SameLine();

						if (ImGui::Button((autohhjkey2State.buttonText + "##AutoHHJKey2").c_str())) {
							autohhjkey2State.bindingMode = true;
							autohhjkey2State.notBinding = false;
							autohhjkey2State.buttonText = "Press a Key...";
						}
						ImGui::SameLine();
						vk_autohhjkey2 = BindKeyMode(&vk_autohhjkey2, vk_autohhjkey2, selected_section);
						
						ImGui::SetNextItemWidth(150.0f);
						GetKeyNameFromHex(vk_autohhjkey2, autohhjkey2State.keyBufferHuman, sizeof(autohhjkey2State.keyBufferHuman));
						ImGui::InputText("##AutoHHJKey2Human", autohhjkey2State.keyBufferHuman, sizeof(autohhjkey2State.keyBufferHuman), ImGuiInputTextFlags_ReadOnly);
						ImGui::SameLine();
						ImGui::TextWrapped("Key");
						
						ImGui::SetNextItemWidth(50.0f);
						ImGui::SameLine();
						ImGui::PushID("AutoHHJKey2Hex");
						ImGui::InputText("##AutoHHJKey2Hex", autohhjkey2State.keyBuffer, sizeof(autohhjkey2State.keyBuffer), ImGuiInputTextFlags_CharsNoBlank | ImGuiInputTextFlags_CharsHexadecimal | ImGuiInputTextFlags_ReadOnly);
						ImGui::PopID();
						ImGui::SameLine();
						ImGui::TextWrapped("Hex");
						
						if (ImGui::Button("R##AutoHHJKey2Time", ImVec2(25, 0))) {
							AutoHHJKey2Time = 68;
							sprintf(AutoHHJKey2TimeChar, "%d", AutoHHJKey2Time);
						}
						ImGui::SameLine();
						ImGui::TextWrapped("Hold time (ms): ");
						ImGui::SameLine();
						ImGui::SetNextItemWidth(50);
						if (ImGui::InputText("##AutoHHJKey2Time", AutoHHJKey2TimeChar, sizeof(AutoHHJKey2TimeChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
							try { AutoHHJKey2Time = std::stoi(AutoHHJKey2TimeChar); } catch (...) { }
						}
						ImGui::Unindent();
					}

					ImVec2 tooltipcursorpos = ImGui::GetCursorScreenPos();
					ImGui::PushStyleColor(ImGuiCol_Text, GetCurrentTheme().accent_primary);
					ImGui::Text("What is this ("); 
					ImGui::PopStyleColor();
					ImGui::SameLine(0, 0);

					ImGui::Text("?"); ImGui::SameLine(0, 0);

					ImGui::PushStyleColor(ImGuiCol_Text, GetCurrentTheme().accent_primary);
					ImGui::Text(")");
					ImGui::PopStyleColor();

					ImGui::SetCursorScreenPos(tooltipcursorpos);
					ImGui::InvisibleButton("##tooltip", ImGui::CalcTextSize("What is this (?)"));
					if (ImGui::IsItemHovered(ImGuiHoveredFlags_AllowWhenDisabled))
						ImGui::SetTooltip("ADVANCED HHJ OPTIONS\n"
										  "These are advanced options configurable for the manual HHJ execution.\n"
										  "They modify the logic of what occurs after the unfreeze.\n\n"
										  "If you figure out a combination of these values that consistently leads to higher HHJ's, please tell the Roblox Glitching Community Discord.\n\n"
										  "Set freeze delay: when 'Apply' is unchecked the program uses the dynamic default behavior (non-speedrun = 500 ms total; fast HHJ = 200 ms).\n"
										  "If 'Apply' is checked the numeric value is used; setting it to 0 or negative disables freezing entirely.\n\n"
										  "Other timing defaults (Windows): 9, 17, 16. (Linux: 0, 0, 17).\n\n"
										  "CUSTOMIZE AUTOMATIC HHJ\n"
										  "These options allow you to customize the key sequence when using 'Automatically time inputs'.\n\n"
										  "For each key press in the sequence, you can customize:\n"
										  "- Key: Click 'Press a Key...' to bind any key (including combos)\n"
										  "- Hold time (ms): How long to hold the key before moving to the next press\n\n"
										  "Default sequence: Spacebar (550ms) -> W (68ms).\n"
										  "Adjust these values to optimize timing based on your FPS and game state.\n");
					ImGui::SetCursorScreenPos(ImVec2(tooltipcursorpos.x + ImGui::CalcTextSize("What is this (?)").x, tooltipcursorpos.y));
					ImGui::NewLine();

					ImGui::Separator();
					ImGui::TextWrapped("This module abuses Roblox's conversion from angular velocity to regular velocity, and its flawed centre of mass calculation.");
					ImGui::Separator();
					ImGui::TextWrapped("IMPORTANT:");
					ImGui::TextWrapped("Have your Sensitivity and Cam-Fix options set before using this module.");
					ImGui::Separator();
					ImGui::TextWrapped("Explanation:");
					ImGui::NewLine();
					ImGui::TextWrapped("Assuming unequip com offset (Patched by Roblox) set to /e dance2 is used prior to offset com, to perform a Helicopter High Jump, "
										"you want to align yourself with your back against the wall, and rotate slightly to the left (around 5-15 degrees). "
										"Now, turn your camera to face directly towards the wall, turn it towards the left a similar amount (5-15 degrees), "
										"in such a way that when you hold W, you turn INTO the wall, instead of away from it (the smaller the angle, the more "
										"successful you'll be). Now, still keeping the alignment and camera angle, perform a normal lag high jump without "
										"holding any movement keys. Instead of lagging, hold w, and press the assigned hotkey.");

					ImGui::Separator();
					ImGui::TextWrapped("If you are struggling with the lag high jump timing part, you can try using the \"Automatically time inputs\" feature. "
										"Align in the exact same way as stated above, but instead doing the lhj motion, just press the assigned key. This should time "
										"the two jumps, as well as the w tap for you. This can also act as a demonstration for what to do, when using manual activation of the module.");

				}

				if (selected_section == 3) { // Speedglitch

					float CurrentSensValue = static_cast<float>(atof(RobloxSensValue));
					if (CurrentSensValue != PreviousSensValue) {
						if (camfixtoggle) {
							try {
								RobloxPixelValue = static_cast<int>(((500.0f / CurrentSensValue) * (static_cast<float>(359) / 360)) + 0.5f);
							} catch (const std::invalid_argument &e) {
							} catch (const std::out_of_range &e) {
							}
							
						} else {
							try {
								RobloxPixelValue = static_cast<int>(((360.0f / CurrentSensValue) * (static_cast<float>(359) / 360)) + 0.5f);
							} catch (const std::invalid_argument &e) {
							} catch (const std::out_of_range &e) {
							}
						}
						PreviousSensValue = CurrentSensValue;
						sprintf(RobloxPixelValueChar, "%d", RobloxPixelValue);
					}

					ImGui::TextWrapped("Pixel Value for 180 Degree Turn BASED ON SENSITIVITY:");
					ImGui::SetNextItemWidth(90.0f);
					ImGui::SameLine();
					if (ImGui::InputText("##PixelValue", RobloxPixelValueChar, sizeof(RobloxPixelValueChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						try {
							speed_strengthx = std::stoi(RobloxPixelValueChar);
							speed_strengthy = -std::stoi(RobloxPixelValueChar);
						} catch (const std::invalid_argument &e) {
						} catch (const std::out_of_range &e) {
						}
					}

					ImGui::Checkbox("Switch from Toggle Key to Hold Key", &isspeedswitch);
					ImGui::TextWrapped("This module abuses Roblox's conversion from angular velocity to regular velocity, and its flawed centre of mass calculation.");
					ImGui::Separator();
					ImGui::TextWrapped("IMPORTANT: Have your Sensitivity and Cam-Fix options set before using this module.");
					ImGui::Separator();
					ImGui::TextWrapped("Explanation:");
					ImGui::NewLine();

					ImGui::TextWrapped("Assuming unequip \"/e dance2\" is used prior to offset com, to activate a speed glitch, enable shiftlock mode "
										"(found in roblox settings), and press the keybind once to start the macro (or hold down if you are using the hold key option). "
										"Note that the macro should rotate you exactly 180 degrees. If not, verify your Roblox sensitivity in the settings matches the Macros sensitivity value, "
										"also, test out speedglitch with the \"Cam-Fix\" at the top left set to both true and false. Once the macro is activated, simply jump, and hold w. "
										"As long as you are in the air, you will start to gain immense velocity towards the direction you are facing (assuming shiftlock has been held, and "
										", and you are holding w). "
);
				}

				if (selected_section == 4) { // Gear Unequip COM offset
					ImGui::TextWrapped("Gear Slot:");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(30.0f);
					if (ImGui::InputText("##Gearslot", ItemSpeedSlot, sizeof(ItemSpeedSlot), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						try {
							speed_slot = std::stoi(ItemSpeedSlot);
						} catch (const std::invalid_argument &e) {
						} catch (const std::out_of_range &e) {
						}
					}

					ImGui::TextWrapped("Type in a custom chat message! (Disables gear equipping, just pastes your message in chat)");
					ImGui::TextWrapped("(Leave this blank if you don't want a custom message)");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(ImGui::GetContentRegionAvail().x);
					ImGui::InputText("##CustomText", CustomTextChar, sizeof(CustomTextChar));

					ImGui::SetNextItemWidth(150.0f);
					if (ImGui::BeginCombo("Select Emote", optionsforoffset[selected_dropdown])) {
						for (int i = 0; i < IM_ARRAYSIZE(optionsforoffset); i++) {
							bool is_selected = (selected_dropdown == i);
							if (ImGui::Selectable(optionsforoffset[i], is_selected)) {
								selected_dropdown = i;  // Update the selected option
								text = optionsforoffset[selected_dropdown];
							}
							if (is_selected) {
								ImGui::SetItemDefaultFocus();  // Ensure the selected item has focus
							}
						}
						ImGui::EndCombo();
					}

					// Get the binding state for vk_dkey
					BindingState& enterkeyState = g_bindingStates[&vk_enterkey];
    
					ImGui::AlignTextToFramePadding();
					ImGui::TextWrapped("Key to Press After Message/Emote paste:");
					ImGui::SameLine();
    
					if (ImGui::Button((enterkeyState.buttonText + "##EnterKey").c_str())) {
						enterkeyState.bindingMode = true;
						enterkeyState.notBinding = false;
						enterkeyState.buttonText = "Press a Key...";
					}
    
					ImGui::SameLine();
					vk_enterkey = BindKeyMode(&vk_enterkey, vk_enterkey, selected_section);
					ImGui::SetNextItemWidth(150.0f);
					GetKeyNameFromHex(vk_enterkey, enterkeyState.keyBufferHuman, sizeof(enterkeyState.keyBufferHuman));
					ImGui::InputText("Hex:", enterkeyState.keyBufferHuman, sizeof(enterkeyState.keyBufferHuman), ImGuiInputTextFlags_CharsNoBlank | ImGuiInputTextFlags_ReadOnly);
					ImGui::SameLine();
					ImGui::SetNextItemWidth(50.0f);
					ImGui::PushID("Press2");
					// Use the state's keyBuffer for hex display
					ImGui::InputText("##HumanKey", enterkeyState.keyBuffer, sizeof(enterkeyState.keyBuffer), ImGuiInputTextFlags_CharsNoBlank | ImGuiInputTextFlags_CharsHexadecimal);
					ImGui::PopID();

					ImGui::Checkbox("Let the macro Keep the item equipped", &unequiptoggle);

					ImGui::Separator();
					ImGui::TextWrapped("IMPORTANT: This glitch has been patched by Roblox. This is currently deprecated. You may get a COM offset\n"
							            "manually by equipping an item without doing any animations, but it will be very small.");
					ImGui::Separator();

					ImGui::Separator();
					ImGui::TextWrapped("This module allows you to trick Roblox into thinking your centre of mass is elsewhere. This is used in the Helicopter "
										"High Jump, Speed Glitch and Walless LHJ modules.");
					ImGui::Separator();
					ImGui::TextWrapped("IMPORTANT: This ONLY works in R6. Although the glitch is possible in R15, the macro isn't built around that rig type. "
										"An Item is also required, and must be placed in the corresponding gear slot (3 by default).");
					ImGui::NewLine();
					ImGui::TextWrapped("Usage:");
					ImGui::TextWrapped("Assuming you have a gear ready, to get an offset com, put the gear into the corresponding gear slot (set above), "
										"and press the keybind (F8 is used with the fn key). Note if the emote bugs out such as restarting halfway through, "
										"or starting late due to a delay, your com may not be in its most offset state. Reusing the macro until the emote "
										"plays out error free will fix this.");
					ImGui::NewLine();
					ImGui::TextWrapped(
						"In most cases, you will be using the \"/e dance2\" emote, as that provides you with the furthest offset, although the other emotes "
						"are still useful occasionaly, such as \"/e laugh\" for wraparounds, and \"/e cheer\" for walless lhjs.");
				}

				if (selected_section == 5) { // Presskey / Press a Key
					if (presskey_instances.empty()) { ImGui::TextWrapped("No presskey instances."); }
					else {
					PresskeyInstance& inst = presskey_instances[selected_presskey_instance];
					std::string sid = "_pk" + std::to_string(selected_presskey_instance);

					// Instance management bar
					if (presskey_instances.size() > 1) {
						std::string activeLabel = BuildMultiInstanceName(5, selected_presskey_instance + 1);
						ImGui::TextWrapped("%s of %d", activeLabel.c_str(), static_cast<int>(presskey_instances.size()));
						ImGui::SameLine();
					}
					if (ImGui::Button(("+ Add New##AddPK" + sid).c_str())) request_new_presskey_instance = true;
					if (presskey_instances.size() > 1) {
						const int removeTarget = GetInstanceRemovalTargetIndex(presskey_instances.size(), selected_presskey_instance);
						const bool removingMainSlot = (selected_presskey_instance == 0);
						const bool isConfirmArmed = g_instance_remove_confirm_state.stage == 1 &&
							g_instance_remove_confirm_state.section_index == 5 &&
							g_instance_remove_confirm_state.target_instance_index == removeTarget;

						ImGui::SameLine();
						if (isConfirmArmed) {
							ImGui::PushStyleColor(ImGuiCol_Text, ImVec4(0.0f, 0.0f, 0.0f, 1.0f));
							ImGui::PushStyleColor(ImGuiCol_Button, ImVec4(1.0f, 1.0f, 0.0f, 1.0f));
							ImGui::PushStyleColor(ImGuiCol_ButtonHovered, ImVec4(1.0f, 1.0f, 0.0f, 1.0f));
							ImGui::PushStyleColor(ImGuiCol_ButtonActive, ImVec4(1.0f, 1.0f, 0.0f, 1.0f));
						}

						std::string removeButtonLabel = removingMainSlot ? "- Remove Last Instance" : "- Remove Current Instance";
						bool removeClicked = ImGui::Button((removeButtonLabel + "##RemPK" + sid).c_str());

						if (isConfirmArmed) {
							ImGui::PopStyleColor(4);
						}

						if (removeClicked) {
							if (isConfirmArmed) {
								request_remove_presskey_instance_index.store(removeTarget, std::memory_order_release);
								ResetInstanceRemoveConfirmState();
							} else {
								g_instance_remove_confirm_state.stage = 1;
								g_instance_remove_confirm_state.timer = 0.7f;
								g_instance_remove_confirm_state.section_index = 5;
								g_instance_remove_confirm_state.target_instance_index = removeTarget;
							}
						}
					}
					ImGui::Separator();

					// Get the binding state for vk_presskey of this instance
					BindingState& dkeyState = g_bindingStates[&inst.vk_presskey];
    
					ImGui::TextWrapped("Key to Press:");
					ImGui::SameLine();
    
					if (ImGui::Button((dkeyState.buttonText + "##DKey" + sid).c_str())) {
						dkeyState.bindingMode = true;
						dkeyState.notBinding = false;
						dkeyState.buttonText = "Press a Key...";
					}
    
					ImGui::SameLine();
					inst.vk_presskey = BindKeyMode(&inst.vk_presskey, inst.vk_presskey, selected_section);
					ImGui::SetNextItemWidth(170.0f);
					GetKeyNameFromHex(inst.vk_presskey, dkeyState.keyBufferHuman, sizeof(dkeyState.keyBufferHuman));
					ImGui::InputText(("Key to Press##" + sid).c_str(), dkeyState.keyBufferHuman, sizeof(dkeyState.keyBufferHuman), ImGuiInputTextFlags_CharsNoBlank | ImGuiInputTextFlags_ReadOnly);
					ImGui::SameLine();
					ImGui::SetNextItemWidth(130.0f);
					ImGui::PushID(("PK_Press2" + sid).c_str());
					ImGui::InputText("(Hexadecimal)", dkeyState.keyBuffer, sizeof(dkeyState.keyBuffer), ImGuiInputTextFlags_CharsNoBlank | ImGuiInputTextFlags_CharsHexadecimal);
					ImGui::PopID();
					ImGui::Text("Length of Second Button Press (ms):");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(80);
					if (ImGui::InputText(("##PressKeyDelayChar" + sid).c_str(), inst.PressKeyDelayChar, sizeof(PresskeyInstance::PressKeyDelayChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						try { inst.PressKeyDelay = std::stoi(inst.PressKeyDelayChar); } catch (...) {}
					}

					ImGui::Text("Delay Before Second Press (ms):");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(80);
					if (ImGui::InputText(("##PressKeyBonusDelayChar" + sid).c_str(), inst.PressKeyBonusDelayChar, sizeof(PresskeyInstance::PressKeyBonusDelayChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						try { inst.PressKeyBonusDelay = std::stoi(inst.PressKeyBonusDelayChar); } catch (...) {}
					}

					ImGui::Separator();
					ImGui::TextWrapped("Explanation:");
					ImGui::NewLine();
					ImGui::TextWrapped("It will press the second keybind for a single frame whenever you press the first keybind. "
										"This is most commonly used for micro-adjustments while moving, especially if you do this while jumping.");
					} // end if !empty
				}

				if (selected_section == 6) { // Wallhop
					if (wallhop_instances.empty()) { ImGui::TextWrapped("No wallhop instances."); }
					else {
					WallhopInstance& inst = wallhop_instances[selected_wallhop_instance];
					std::string sid = "_wh" + std::to_string(selected_wallhop_instance);

					// Instance management bar
					if (wallhop_instances.size() > 1) {
						std::string activeLabel = BuildMultiInstanceName(6, selected_wallhop_instance + 1);
						ImGui::TextWrapped("%s of %d", activeLabel.c_str(), static_cast<int>(wallhop_instances.size()));
						ImGui::SameLine();
					}
					if (ImGui::Button(("+ Add New##AddWH" + sid).c_str())) request_new_wallhop_instance = true;
					if (wallhop_instances.size() > 1) {
						const int removeTarget = GetInstanceRemovalTargetIndex(wallhop_instances.size(), selected_wallhop_instance);
						const bool removingMainSlot = (selected_wallhop_instance == 0);
						const bool isConfirmArmed = g_instance_remove_confirm_state.stage == 1 &&
							g_instance_remove_confirm_state.section_index == 6 &&
							g_instance_remove_confirm_state.target_instance_index == removeTarget;

						ImGui::SameLine();
						if (isConfirmArmed) {
							ImGui::PushStyleColor(ImGuiCol_Text, ImVec4(0.0f, 0.0f, 0.0f, 1.0f));
							ImGui::PushStyleColor(ImGuiCol_Button, ImVec4(1.0f, 1.0f, 0.0f, 1.0f));
							ImGui::PushStyleColor(ImGuiCol_ButtonHovered, ImVec4(1.0f, 1.0f, 0.0f, 1.0f));
							ImGui::PushStyleColor(ImGuiCol_ButtonActive, ImVec4(1.0f, 1.0f, 0.0f, 1.0f));
						}

						std::string removeButtonLabel = removingMainSlot ? "- Remove Last Instance" : "- Remove Current Instance";
						bool removeClicked = ImGui::Button((removeButtonLabel + "##RemWH" + sid).c_str());

						if (isConfirmArmed) {
							ImGui::PopStyleColor(4);
						}

						if (removeClicked) {
							if (isConfirmArmed) {
								request_remove_wallhop_instance_index.store(removeTarget, std::memory_order_release);
								ResetInstanceRemoveConfirmState();
							} else {
								g_instance_remove_confirm_state.stage = 1;
								g_instance_remove_confirm_state.timer = 0.7f;
								g_instance_remove_confirm_state.section_index = 6;
								g_instance_remove_confirm_state.target_instance_index = removeTarget;
							}
						}
					}
					ImGui::Separator();

					ImGui::TextWrapped("Flick Degrees (Estimated):");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(70.0f);
					float sensValue = static_cast<float>(std::atof(RobloxSensValue));
					if (sensValue != 0.0f) {
						snprintf(inst.WallhopDegrees, sizeof(WallhopInstance::WallhopDegrees), "%d",
							static_cast<int>(360 * (std::atof(inst.WallhopPixels) * std::atof(RobloxSensValue)) / (camfixtoggle ? 1000 : 720)));
					}
					
					if (ImGui::InputText(("##WallhopDegrees" + sid).c_str(), inst.WallhopDegrees, sizeof(WallhopInstance::WallhopDegrees), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						float pixels = static_cast<float>(std::atof(inst.WallhopDegrees) * (camfixtoggle ? 1000.0f : 720.0f) / (360.0f * std::atof(RobloxSensValue)));
						snprintf(inst.WallhopPixels, sizeof(WallhopInstance::WallhopPixels), "%.0f", pixels);
						try {
							inst.wallhop_dx = static_cast<int>(std::round(std::stoi(inst.WallhopPixels)));
							inst.wallhop_dy = -static_cast<int>(std::round(std::stoi(inst.WallhopPixels)));
						} catch (...) {}
					}

					ImGui::TextWrapped("Flick Pixel Amount:");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(70.0f);
					if (ImGui::InputText(("##WallhopPixels" + sid).c_str(), inst.WallhopPixels, sizeof(WallhopInstance::WallhopPixels), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						try {
							inst.wallhop_dx = static_cast<int>(std::round(std::stoi(inst.WallhopPixels)));
							inst.wallhop_dy = -static_cast<int>(std::round(std::stoi(inst.WallhopPixels)));
						} catch (...) {}
					}

					ImGui::SameLine();
					ImGui::Text("Vertical Pixel Movement:");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(70.0f);
					if (ImGui::InputText(("##WallhopVertical" + sid).c_str(), &inst.WallhopVerticalChar[0], sizeof(inst.WallhopVerticalChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						try { inst.wallhop_vertical = static_cast<int>(std::round(std::stoi(inst.WallhopVerticalChar))); } catch (...) {}
					}

					ImGui::TextWrapped("Wallhop Length (ms):");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(70.0f);
					if (ImGui::InputText(("##WallhopDelay" + sid).c_str(), inst.WallhopDelayChar, sizeof(WallhopInstance::WallhopDelayChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						try { inst.WallhopDelay = static_cast<int>(std::round(std::stoi(inst.WallhopDelayChar))); } catch (...) {}
					}

					ImGui::TextWrapped("Bonus Wallhop Delay Before Jumping (ms):");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(70.0f);
					if (ImGui::InputText(("##WallhopBonusDelay" + sid).c_str(), inst.WallhopBonusDelayChar, sizeof(WallhopInstance::WallhopBonusDelayChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						try { inst.WallhopBonusDelay = static_cast<int>(std::round(std::stoi(inst.WallhopBonusDelayChar))); } catch (...) {}
					}

					ImGui::Checkbox(("Switch to Left-Flick Wallhop##" + sid).c_str(), &inst.wallhopswitch);
					ImGui::Checkbox(("Jump During Wallhop##" + sid).c_str(), &inst.toggle_jump);
					ImGui::Checkbox(("Flick-Back During Wallhop##" + sid).c_str(), &inst.toggle_flick);

					if (ImGui::CollapsingHeader(("Hotkeys##WallhopHotkeys" + sid).c_str(), ImGuiTreeNodeFlags_DefaultOpen)) {
						BindingState& wallhopJumpState = g_bindingStates[&inst.vk_jumpkey];

						if (ImGui::Button(("R##WallhopJumpKeyReset" + sid).c_str(), ImVec2(25, 0))) {
							inst.vk_jumpkey = VK_SPACE;
							FormatHexKeyString(inst.vk_jumpkey, wallhopJumpState.keyBuffer, sizeof(wallhopJumpState.keyBuffer));
							GetKeyNameFromHex(inst.vk_jumpkey, wallhopJumpState.keyBufferHuman, sizeof(wallhopJumpState.keyBufferHuman));
							wallhopJumpState.buttonText = "Click to Bind Key";
						}
						ImGui::SameLine();
						ImGui::TextWrapped("Jump Key:");
						ImGui::SameLine();

						if (ImGui::Button((wallhopJumpState.buttonText + "##WallhopJumpKey" + sid).c_str())) {
							wallhopJumpState.bindingMode = true;
							wallhopJumpState.notBinding = false;
							wallhopJumpState.buttonText = "Press a Key...";
						}
						ImGui::SameLine();
						inst.vk_jumpkey = BindKeyMode(&inst.vk_jumpkey, inst.vk_jumpkey, selected_section);

						ImGui::SetNextItemWidth(150.0f);
						GetKeyNameFromHex(inst.vk_jumpkey, wallhopJumpState.keyBufferHuman, sizeof(wallhopJumpState.keyBufferHuman));
						ImGui::InputText(("##WallhopJumpKeyHuman" + sid).c_str(), wallhopJumpState.keyBufferHuman, sizeof(wallhopJumpState.keyBufferHuman), ImGuiInputTextFlags_ReadOnly);
						ImGui::SameLine();
						ImGui::TextWrapped("Key");

						ImGui::SetNextItemWidth(50.0f);
						ImGui::SameLine();
						ImGui::InputText(("##WallhopJumpKeyHex" + sid).c_str(), wallhopJumpState.keyBuffer, sizeof(wallhopJumpState.keyBuffer), ImGuiInputTextFlags_CharsNoBlank | ImGuiInputTextFlags_CharsHexadecimal | ImGuiInputTextFlags_ReadOnly);
						ImGui::SameLine();
						ImGui::TextWrapped("Hex");
					}

					ImGui::Separator();
					ImGui::TextWrapped("IMPORTANT:");
					ImGui::TextWrapped("THE ANGLE THAT YOU TURN IS DIRECTLY RELATED TO YOUR ROBLOX SENSITIVITY. "
										"If you want to pick a SPECIFIC ANGLE, heres how. "
										"For games without the cam-fix module, 180 degrees is equal to 360 divided by your Roblox Sensitivity. "
										"For games with the cam-fix module, 180 degrees is equal to 500 divided by your Roblox Sensitivity. "
										"Ex: 0.6 sens with no cam fix = 600 pixels, which means 600 / 4 (150) is equal to a 45 degree turn.");
					ImGui::TextWrapped("INTEGERS ONLY!");
					ImGui::Separator();
					ImGui::TextWrapped("Explanation:");
					ImGui::NewLine();
					ImGui::TextWrapped("This Macro automatically flicks your screen AND jumps at the same time, performing a wallhop.");
					} // end if !empty
				}

				if (selected_section == 7) { // Walless LHJ
					ImGui::Checkbox("Switch to Left-Sided LHJ", &wallesslhjswitch); // Left Sided lhj switch
					ImGui::Separator();
					ImGui::TextWrapped("Explanation:");
					ImGui::NewLine();
					ImGui::TextWrapped("If you offset your center of mass to any direction EXCEPT directly upwards, you will be able to perform "
										"14 stud jumps using this macro. However, you need at LEAST one FULL FOOT on the platform "
										"in order to do it.");
				}

				if (selected_section == 8) { // Item Clip
					ImGui::TextWrapped("Item Clip Slot:");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(30.0f);

					if (ImGui::InputText("##ItemClipSlot", ItemClipSlot, sizeof(ItemClipSlot), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						try {
							clip_slot = std::stoi(ItemClipSlot);
						} catch (const std::invalid_argument &e) {
						} catch (const std::out_of_range &e) {
						}
					}

					ImGui::TextWrapped("Item Clip Delay in Milliseconds (Default 34ms):");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(120.0f);
					if (ImGui::InputText("##ItemClipDelay", ItemClipDelay, sizeof(ItemClipDelay), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						try {
							clip_delay = std::stoi(ItemClipDelay);
						} catch (const std::invalid_argument &e) {
						} catch (const std::out_of_range &e) {
						}
					}

					ImGui::Checkbox("Switch from Toggle Key to Hold Key", &isitemclipswitch);

					ImGui::Separator();
					ImGui::TextWrapped("Explanation:");
					ImGui::NewLine();
					ImGui::TextWrapped("This macro will equip and unequip your item in the amount of milliseconds you put in. "
										"It's recommended to shiftlock, jump, and hold W while staying at the wall. "
										"This lets you clip through walls in both R6 and R15, however, it is EXTREMELY RNG. "
										"There are way too factors that control this, the delay, fps, the item's size, your animation, etc. "
										"The item in the best scenario should be big and stretch far into the wall. ");
					ImGui::TextWrapped("If 'Disable outside of Roblox' is enabled, item clip only runs while tabbed into Roblox.");
				}

				if (selected_section == 9) { // Laugh Clip
					ImGui::Checkbox("Disable S being pressed (Slightly weaker laugh clips, but interferes with movement less)", &laughmoveswitch);
					ImGui::TextWrapped("Explanation:");
					ImGui::NewLine();
					ImGui::TextWrapped("MUST BE ABOVE 60 FPS AND IN R6!");
					ImGui::TextWrapped("Go against a wall unshiftlocked and angle your camera DIRECTLY OPPOSITE TO THE WALL. "
										"The Macro will Automatically type out /e laugh using the settings inside of the \"Unequip Com\" section. "
										"It will automatically time your shiftlock and jump to laugh clip through up to ~1.3 studs.");
				}

				if (selected_section == 10) { // Wall-Walk

					float CurrentWallWalkValue = static_cast<float>(atof(RobloxSensValue));
					float CurrentWallwalkSide = camfixtoggle;


					if (CurrentWallWalkValue != PreviousWallWalkValue) {
						if (camfixtoggle) {
							wallwalk_strengthx = static_cast<int>(round((500.0f / CurrentWallWalkValue) * 0.13f));
							wallwalk_strengthy = -static_cast<int>(round((500.0f / CurrentWallWalkValue) * 0.13f));
						} else {
							wallwalk_strengthx = static_cast<int>(round((360.0f / CurrentWallWalkValue) * 0.13f));
							wallwalk_strengthy = -static_cast<int>(round((360.0f / CurrentWallWalkValue) * 0.13f));
						}
					}

					PreviousWallWalkValue = CurrentWallWalkValue;
					sprintf(RobloxWallWalkValueChar, "%d", wallwalk_strengthx);

					ImGui::TextWrapped("Wall-Walk Pixel Value BASED ON SENSITIVITY (meant to be low):");
					ImGui::SetNextItemWidth(90.0f);
					ImGui::SameLine();
					ImGui::InputText("##PixelValue", RobloxWallWalkValueChar, sizeof(RobloxWallWalkValueChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank);

					ImGui::Checkbox("Switch to Left-Flick Wallwalk", &wallwalktoggleside);

					ImGui::SetNextItemWidth(100.0f);
					ImGui::InputText("Delay Between Flicks (Don't change from 72720 unless neccessary):", RobloxWallWalkValueDelayChar, sizeof(RobloxWallWalkValueDelayChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank);

					try {
						RobloxWallWalkValueDelay = static_cast<int>(atof(RobloxWallWalkValueDelayChar));
					} catch (const std::invalid_argument &e) {
					} catch (const std::out_of_range &e) {
					}

					try { // Error Handling
						wallwalk_strengthx = std::stoi(RobloxWallWalkValueChar);
						wallwalk_strengthy = -std::stoi(RobloxWallWalkValueChar);
					} catch (const std::invalid_argument &e) {
					} catch (const std::out_of_range &e) {
					}

					ImGui::Checkbox("Switch from Toggle Key to Hold Key", &iswallwalkswitch);
					ImGui::Separator();
					ImGui::TextWrapped("IMPORTANT: FOR MOST OPTIMAL RESULTS, INPUT YOUR ROBLOX INGAME SENSITIVITY!");
					ImGui::TextWrapped("THE HIGHER FPS YOU ARE, THE MORE STABLE IT GETS, HOWEVER 60 FPS IS ENOUGH FOR INFINITE DISTANCE");
					ImGui::TextWrapped("TICK OR UNTICK THE CHECKBOX DEPENDING ON WHETHER THE GAME USES CAM-FIX MODULE OR NOT. "
										"If you don't know, do BOTH and check which one provides you with a 180 degree rotation. "
										"You can also toggle whether it's right facing or left facing (Makes its respective side easier) "
										"Use 'Disable outside of Roblox' to control whether wallwalk only runs in Roblox.");
					ImGui::Separator();
					ImGui::TextWrapped("Explanation:");
					ImGui::NewLine();
					ImGui::TextWrapped("This macro abuses the way leg raycast physics work to permanently keep wallhopping, without jumping "
										"you can walk up to a wall, maybe at a bit of an angle, and hold W and D or A to slowly walk across.");
			}

				if (selected_section == 11) { // Spamkey
					if (spamkey_instances.empty()) { ImGui::TextWrapped("No spamkey instances."); }
					else {
					SpamkeyInstance& inst = spamkey_instances[selected_spamkey_instance];
					std::string sid = "_sk" + std::to_string(selected_spamkey_instance);

					// Instance management bar
					if (spamkey_instances.size() > 1) {
						std::string activeLabel = BuildMultiInstanceName(11, selected_spamkey_instance + 1);
						ImGui::TextWrapped("%s of %d", activeLabel.c_str(), static_cast<int>(spamkey_instances.size()));
						ImGui::SameLine();
					}
					if (ImGui::Button(("+ Add New##AddSK" + sid).c_str())) request_new_spamkey_instance = true;
					if (spamkey_instances.size() > 1) {
						const int removeTarget = GetInstanceRemovalTargetIndex(spamkey_instances.size(), selected_spamkey_instance);
						const bool removingMainSlot = (selected_spamkey_instance == 0);
						const bool isConfirmArmed = g_instance_remove_confirm_state.stage == 1 &&
							g_instance_remove_confirm_state.section_index == 11 &&
							g_instance_remove_confirm_state.target_instance_index == removeTarget;

						ImGui::SameLine();
						if (isConfirmArmed) {
							ImGui::PushStyleColor(ImGuiCol_Text, ImVec4(0.0f, 0.0f, 0.0f, 1.0f));
							ImGui::PushStyleColor(ImGuiCol_Button, ImVec4(1.0f, 1.0f, 0.0f, 1.0f));
							ImGui::PushStyleColor(ImGuiCol_ButtonHovered, ImVec4(1.0f, 1.0f, 0.0f, 1.0f));
							ImGui::PushStyleColor(ImGuiCol_ButtonActive, ImVec4(1.0f, 1.0f, 0.0f, 1.0f));
						}

						std::string removeButtonLabel = removingMainSlot ? "- Remove Last Instance" : "- Remove Current Instance";
						bool removeClicked = ImGui::Button((removeButtonLabel + "##RemSK" + sid).c_str());

						if (isConfirmArmed) {
							ImGui::PopStyleColor(4);
						}

						if (removeClicked) {
							if (isConfirmArmed) {
								request_remove_spamkey_instance_index.store(removeTarget, std::memory_order_release);
								ResetInstanceRemoveConfirmState();
							} else {
								g_instance_remove_confirm_state.stage = 1;
								g_instance_remove_confirm_state.timer = 0.7f;
								g_instance_remove_confirm_state.section_index = 11;
								g_instance_remove_confirm_state.target_instance_index = removeTarget;
							}
						}
					}
					ImGui::Separator();

					BindingState& spamState = g_bindingStates[&inst.vk_spamkey];
    
					ImGui::TextWrapped("Key to Press:");
					ImGui::SameLine();
					if (ImGui::Button((spamState.buttonText + "##SpamKey" + sid).c_str())) {
						spamState.bindingMode = true;
						spamState.notBinding = false;
						spamState.buttonText = "Press a Key...";
					}
					ImGui::SameLine();
					inst.vk_spamkey = BindKeyMode(&inst.vk_spamkey, inst.vk_spamkey, selected_section);
					ImGui::SetNextItemWidth(170.0f);
					GetKeyNameFromHex(inst.vk_spamkey, spamState.keyBufferHuman, sizeof(spamState.keyBufferHuman));
					ImGui::InputText(("Key to Press##" + sid).c_str(), spamState.keyBufferHuman, sizeof(spamState.keyBufferHuman), ImGuiInputTextFlags_CharsNoBlank | ImGuiInputTextFlags_ReadOnly);
					ImGui::SameLine();
					ImGui::SetNextItemWidth(130.0f);
					ImGui::PushID(("SK_Press2" + sid).c_str());
					ImGui::InputText("(Hexadecimal)", spamState.keyBuffer, sizeof(spamState.keyBuffer), ImGuiInputTextFlags_CharsNoBlank | ImGuiInputTextFlags_CharsHexadecimal);
					ImGui::PopID();
					ImGui::TextWrapped("Spam Delay (Milliseconds):");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(120.0f);
					if (ImGui::InputText(("##SpamDelay" + sid).c_str(), inst.SpamDelay, sizeof(SpamkeyInstance::SpamDelay), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						try {
							inst.spam_delay = std::stof(inst.SpamDelay);
							inst.real_delay  = static_cast<int>((inst.spam_delay + 0.5f) / 2);
						} catch (...) {}
					}

					ImGui::TextWrapped("I do not take any responsibility if you set the delay to 0ms");
					ImGui::Checkbox(("Switch from Toggle Key to Hold Key##" + sid).c_str(), &inst.isspamswitch);
					ImGui::Separator();
					ImGui::TextWrapped("Explanation:");
					ImGui::NewLine();
					ImGui::TextWrapped("This macro will spam the second key with a millisecond delay. "
										"This can be used as an autoclicker for any games you want, or a key-spam.");
					} // end if !empty
				}

				if (selected_section == 12) { // Ledge Bounce
					ImGui::Checkbox("Switch Ledge Bounce to Left-Sided", &bouncesidetoggle);
					ImGui::Checkbox("Stay Horizontal After Bounce", &bouncerealignsideways);
					ImGui::Checkbox("Automatically Hold Movement Keys", &bounceautohold);
					ImGui::Separator();
					ImGui::TextWrapped("IMPORTANT:");
					ImGui::TextWrapped("PLEASE SET YOUR ROBLOX SENS AND CAM-FIX CORRECTLY SO IT CAN ACTUALLY DO THE PROPER TURNS! It works best at high FPS (120+).");
					ImGui::TextWrapped("Also, if you set it to automatically hold movement keys, PLEASE HOLD THE KEY AT THE PROPER TIME, else it will keep moving forever.");
					ImGui::Separator();
					ImGui::TextWrapped("Explanation:");
					ImGui::NewLine();
					ImGui::TextWrapped(
									"Walk up to a ledge with your camera sideways, about half of your left foot should be on the platform. "
									"The Macro will Automatically flick your camera 90 degrees, let you fall, and then flick back. "
									"This will boost you up slightly into the air, and you can even jump after it. This lets you \"jump\" without jumping.");
				}

				if (selected_section == 13) { // Bunnyhop
					ImGui::TextWrapped("Bunnyhop Delay in Milliseconds (Default 10ms):");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(120.0f);
					if (ImGui::InputText("##BunnyhopDelay", BunnyHopDelayChar, sizeof(BunnyHopDelayChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						try {
							BunnyHopDelay = static_cast<int>(atof(BunnyHopDelayChar));
							if (g_isLinuxWine) {
								SetBhopDelay(BunnyHopDelay);
							}

						} catch (const std::invalid_argument &e) {
						} catch (const std::out_of_range &e) {
						}
					}

					ImGui::Checkbox("Enable Intelligent Auto-Toggle", &bunnyhopsmart);

					ImGui::Separator();
					ImGui::TextWrapped("If Intelligent Auto-Toggle is on, pressing your chat key will temporarily disable bhop "
									   "until you press left click or enter to leave the chat.");
					ImGui::Separator();

					ImGui::TextWrapped("Explanation:");
					ImGui::NewLine();
					ImGui::TextWrapped(
									"This Macro will automatically spam your key (typically space) with a specified delay whenever space is held down. "
									"This is created as a more functional Spamkey implementation specifically for Bhop/Bunnyhop.");

					ImGui::TextWrapped("This will only be restricted to Roblox when 'Disable outside of Roblox' is enabled.");
				}

				if (selected_section == 14) { // Floor Bounce
					ImGui::Checkbox("Attempt to HHJ for potentially slightly more height (EXPERIMENTAL, NOT RECOMMENDED)", &floorbouncehhj);
					if (ImGui::CollapsingHeader("Advanced Floor Bounce HHJ Options", showadvancedhhjbounce ? ImGuiTreeNodeFlags_DefaultOpen : 0))
					{
						showadvancedhhjbounce = true;

						// Add some vertical spacing for better visual separation
						ImGui::Spacing();
    
						// First option with reset button on the left
						ImGui::Indent();
						if (ImGui::Button("R##FloorBounceDelay1", ImVec2(25, 0))) {
							FloorBounceDelay1 = 5;
							sprintf(FloorBounceDelay1Char, "%d", FloorBounceDelay1);
						}
						ImGui::SameLine();
						ImGui::TextWrapped("Delay (Milliseconds) after unfreezing and before shiftlocking");
						ImGui::SameLine();
						ImGui::SetNextItemWidth(50);
						if (ImGui::InputText("##FloorBounceDelay1", FloorBounceDelay1Char, sizeof(FloorBounceDelay1Char), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
							try {
								FloorBounceDelay1 = std::stoi(FloorBounceDelay1Char);
							} catch (const std::invalid_argument &e) {
							} catch (const std::out_of_range &e) {
							}
						}
						ImGui::Unindent();
    
						// Second option with reset button on the left
						ImGui::Indent();
						if (ImGui::Button("R##FloorBounceDelay2", ImVec2(25, 0))) {
							FloorBounceDelay2 = 8;
							sprintf(FloorBounceDelay2Char, "%d", FloorBounceDelay2);
						}
						ImGui::SameLine();
						ImGui::TextWrapped("Delay (Milliseconds) before enabling helicoptering");
						ImGui::SameLine();
						ImGui::SetNextItemWidth(50);
						if (ImGui::InputText("##FloorBounceDelay2", FloorBounceDelay2Char, sizeof(FloorBounceDelay2Char), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
							try {
								FloorBounceDelay2 = std::stoi(FloorBounceDelay2Char);
							} catch (const std::invalid_argument &e) {
							} catch (const std::out_of_range &e) {
							}
						}
						ImGui::Unindent();
    
						// Third option with reset button on the left
						ImGui::Indent();
						if (ImGui::Button("R##FloorBounceDelay3", ImVec2(25, 0))) {
							FloorBounceDelay3 = 100;
							sprintf(FloorBounceDelay3Char, "%d", FloorBounceDelay3);
						}
						ImGui::SameLine();
						ImGui::TextWrapped("Time (Milliseconds) spent helicoptering");
						ImGui::SameLine();
						ImGui::SetNextItemWidth(50);
						if (ImGui::InputText("##FloorBounceDelay3", FloorBounceDelay3Char, sizeof(FloorBounceDelay3Char), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
							try {
								FloorBounceDelay3 = std::stoi(FloorBounceDelay3Char);
							} catch (const std::invalid_argument &e) {
							} catch (const std::out_of_range &e) {
							}
						}
						ImGui::Unindent();
    
						// Optional: Add some spacing at the bottom
						ImGui::Spacing();
					}

					ImGui::TextWrapped("IMPORTANT:");
					ImGui::TextWrapped("This module only works at default Roblox gravity. If you can find an equation to allow conversion to different gravities, be my guest.");
					ImGui::TextWrapped("Only works in R6 for now.");
					ImGui::TextWrapped("FPS must be set to 160 or more to function properly. Higher is better.");
					ImGui::Separator();
					ImGui::TextWrapped("Explanation:");
					ImGui::NewLine();
					ImGui::TextWrapped(
									"This Macro will automate an extremely precise glitch involving basically doing a lag high jump off the floor. "
									"It will jump up, lag into the floor, and then perform a lag high jump off of the floor. "
									"This will boost you up significantly into the air.");
				}

				if (selected_section == 15) { // Lag Switch

                    ImGui::Checkbox("Switch from Hold Key to Toggle Key", &islagswitchswitch);

					ImGui::Separator();

					ImVec2 tooltipcursorpos = ImGui::GetCursorScreenPos();

					ImGui::Checkbox("Prevent Roblox Disconnection (", &prevent_disconnect);

					ImGui::SameLine(0, 0);

					ImGui::PushStyleColor(ImGuiCol_Text, GetCurrentTheme().accent_primary);
					ImGui::Text("?");
					ImGui::PopStyleColor();

					ImGui::SameLine(0, 0);

					ImGui::Text(")");
					ImGui::SetCursorScreenPos(tooltipcursorpos);
					ImVec2 TextSizeCalc = ImGui::CalcTextSize("Prevent Roblox Disconnection (?)      ");
					ImGui::InvisibleButton("##tooltip", TextSizeCalc);
					if (ImGui::IsItemHovered(ImGuiHoveredFlags_AllowWhenDisabled))
						ImGui::SetTooltip("Prevents Timeout past the usual 10s threshold.\n"
											"Experimental, may break or kick. This is actively being worked on.\n"
											"This version has a chance to leak some of your movements to the server (you will appear to be extremely laggy/teleporting)\n");

					ImGui::SetCursorScreenPos(ImVec2(tooltipcursorpos.x, tooltipcursorpos.y + 6));
					ImGui::NewLine();
					
                    bool filter_changed = false;

                    if (ImGui::Checkbox("Only Lag Switch Roblox", &lagswitchtargetroblox)) {
						filter_changed = true;
					}

                    if (ImGui::IsItemHovered()) {
                        ImGui::SetTooltip("Filters only Roblox traffic.");
					}

					if (ImGui::Checkbox("Also Block TCP (Websites)", &lagswitchusetcp)) {
						filter_changed = true;
					}

					ImGui::Separator();

                    ImGui::Checkbox("Auto-Unlag (Anti-Kick) (Non-Roblox Games Only)", &lagswitch_autounblock);
                    
                    // Disable inputs if toggle is off
                    if (!lagswitch_autounblock) ImGui::BeginDisabled();

                    ImGui::TextWrapped("Automatically stops lagging after this amount of seconds");
                    
                    ImGui::SetNextItemWidth(60.0f);
                    ImGui::InputFloat("##LagFloat", &lagswitch_max_duration, 0.0f, 0.0f, "%.2f");
                    ImGui::SameLine();
                    ImGui::SetNextItemWidth(300.0f);
                    ImGui::SliderFloat("##LagSlider", &lagswitch_max_duration, 0.0f, 15.0f, "%.2f Seconds");

                    char lagUnblockBuffer[16];
                    std::snprintf(lagUnblockBuffer, sizeof(lagUnblockBuffer), "%d", lagswitch_unblock_ms);

                    ImGui::SetNextItemWidth(50.0f);
                    if (ImGui::InputText("Modify 50ms Default Unlag Time (MS)", lagUnblockBuffer, sizeof(lagUnblockBuffer), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
                        lagswitch_unblock_ms = std::atoi(lagUnblockBuffer);
                    }

                    if (!lagswitch_autounblock) ImGui::EndDisabled();

					ImGui::Separator();
					if (ImGui::Checkbox("Block Outbound (Upload/Send) (Players won't be able to see you move)", &lagswitchoutbound)) filter_changed = true;
					if (ImGui::Checkbox("Block Inbound (Download/Recv) (You won't be able to see other players move)", &lagswitchinbound)) filter_changed = true;
					
                    // Warning if both disabled
                    if (!lagswitchoutbound && !lagswitchinbound) {
                        ImGui::PushStyleColor(ImGuiCol_Text, GetCurrentTheme().warning_color);
                        ImGui::TextWrapped("WARNING: Both Inbound and Outbound are unchecked.\nThe Lag Switch will not block any packets.");
                        ImGui::PopStyleColor();
                    }

					// Fake Lag Section
                    ImGui::Separator();

                    if (ImGui::Checkbox("Fake Lag (Simulate High Ping)", &lagswitchlag)) {
                        // Toggling this might require a filter update if the sub-options (Inbound/Outbound) are set
                        filter_changed = true;
                    }

                    if (!lagswitchlag) ImGui::BeginDisabled();
                    
                    ImGui::Indent();
                    
                    ImGui::Text("Delay Amount (Milliseconds):");
                    ImGui::SameLine();
                    ImGui::SetNextItemWidth(150.0f);
                    // Step of 10ms, Fast Step of 100ms
                    ImGui::InputInt("##LagDelayInput", &lagswitchlagdelay, 10, 100);

                    // Direction Toggles for Fake Lag
                    if (ImGui::Checkbox("Lag Inbound (Recv)##FakeLag", &lagswitchlaginbound)) filter_changed = true;
                    ImGui::SameLine();
                    if (ImGui::Checkbox("Lag Outbound (Send)##FakeLag", &lagswitchlagoutbound)) filter_changed = true;

                    // Warning if enabled but no directions selected
                    if (lagswitchlag && !lagswitchlaginbound && !lagswitchlagoutbound) {
                         ImGui::TextColored(GetCurrentTheme().warning_color, "Select at least one direction to lag!");
                    }

					if (lagswitchlag && ((lagswitchoutbound && lagswitchlagoutbound) || (lagswitchinbound && lagswitchlaginbound))) {
                        ImGui::PushStyleColor(ImGuiCol_Text, GetCurrentTheme().warning_color);
                        ImGui::TextWrapped("WARNING: Both Blocking and Lagging are enabled for Inbound/Outbound packets.\nBlocking takes priority over lagging.");
                        ImGui::PopStyleColor();
                    }

                    ImGui::Unindent();

                    if (!lagswitchlag) ImGui::EndDisabled();

					ImGui::Separator();

                    // If filters changed and driver is active, force restart
                    if (filter_changed && bWinDivertEnabled) {
						RestartLagSwitchCapture();
                    } else {
						SyncLagSwitchBackendConfig();
                    }

					ImGui::Checkbox("Show Lagswitch Status Overlay", &show_lag_overlay);

					if (!show_lag_overlay) ImGui::BeginDisabled();
					ImGui::Indent();
    
					ImGui::Checkbox("Hide When Not Actively Lagswitching", &overlay_hide_inactive);
    
					// Get Screen resolution for sliders
					int screenW = GetSystemMetrics(SM_CXSCREEN);
					int screenH = GetSystemMetrics(SM_CYSCREEN);

					// Initialize position to top-right 20% if first time
					if (overlay_x == -1) { overlay_x = (int)(screenW * 0.8f); }

					ImGui::PushItemWidth(500);

					ImGui::SliderInt("Overlay X", &overlay_x, 0, screenW);
					ImGui::SliderInt("Overlay Y", &overlay_y, 0, screenH);
					ImGui::SliderInt("Text Size", &overlay_size, 10, 100);
    
					ImGui::Checkbox("Add Background", &overlay_use_bg);
					if (!overlay_use_bg) ImGui::BeginDisabled();
					float colors[3] = { overlay_bg_r, overlay_bg_g, overlay_bg_b };
					if (ImGui::ColorEdit3("Background Color", colors)) {
						overlay_bg_r = colors[0];
						overlay_bg_g = colors[1];
						overlay_bg_b = colors[2];
					}
					ImGui::PopItemWidth();

					if (!overlay_use_bg) ImGui::EndDisabled();

					ImGui::Unindent();
					if (!show_lag_overlay) ImGui::EndDisabled();

					ImGui::Separator();
					ImGui::NewLine();
					ImGui::Separator();

                    // The Admin/Load Button Logic
                    if (ImGui::Button(bWinDivertEnabled ? "Disable WinDivert" : "Enable WinDivert")) {
                        if (!bWinDivertEnabled) {
                            if (IsRunAsAdmin()) {
								std::string backendError;
                                if (InitializeLagSwitchBackend(&backendError)) {
                                } else {
									if (!backendError.empty()) {
										LogCritical(backendError);
									}
                                    std::cerr << "Failed to load WinDivert files." << std::endl;
                                }
                            } else {
                                if (DontShowAdminWarning) {
                                    RestartAsAdmin();
                                } else {
                                    bShowAdminPopup = true;
                                }
                            }
                        } else {
                            ShutdownLagSwitchBackend();
                        }
                    }

                    ImGui::SameLine();
                    ImGui::TextColored(bWinDivertEnabled ? GetCurrentTheme().success_color : GetCurrentTheme().error_color, 
                                       bWinDivertEnabled ? "Driver Running" : "Driver Not Running");
                    
                    if (bWinDivertEnabled) {
                        ImGui::SameLine();
                        ImGui::Text(" |  Status: ");
                        ImGui::SameLine();
                        ImGui::TextColored(g_windivert_blocking ? GetCurrentTheme().error_color : GetCurrentTheme().success_color, 
                            g_windivert_blocking ? "LAGGING" : "Clear");
                        
                        // Debug: Show filter string
                        // ImGui::TextDisabled("Filter: %s", g_current_windivert_filter.c_str());
                    }
				}

            } else {
                ImGui::TextWrapped("Select a section to see its settings.");
            }

            ImGui::EndChild(); // End right section
            

            // BOTTOM CONTROLS SETUP

			ImVec2 childPos = ImGui::GetItemRectMin();
			ImVec2 childSize = ImGui::GetItemRectSize();

			ImDrawList *draw_list = ImGui::GetWindowDrawList();

			ImU32 bg_color = ImGui::GetColorU32(ImGuiCol_ChildBg);
			ImU32 border_color = ImGui::GetColorU32(ImGuiCol_Border);
			float rounding = ImGui::GetStyle().ChildRounding;

            // Draw bottom controls
            ImVec2 windowSize = ImGui::GetWindowSize();
            // Align BottomControls exactly below RightSection
            ImGui::SetCursorPosY(windowSize.y - 30 - ImGui::GetStyle().WindowPadding.y);
            ImGui::SetCursorPosX(childPos.x);

            // Force Square Corners for BottomControls
            ImGui::PushStyleVar(ImGuiStyleVar_ChildRounding, 0.0f); 
            
            if (ImGui::BeginChild("BottomControls", ImVec2(childSize.x - 1, 30), false, // 'false' here removes the border from BottomControls
                ImGuiWindowFlags_NoScrollbar | ImGuiWindowFlags_NoScrollWithMouse))
            {
				ImGui::SameLine(childSize.x - 616);
                ImGui::AlignTextToFramePadding();
				// Adjust text starting point by 2 pixels downwards
				ImGui::SetCursorPosY(ImGui::GetCursorPosY() + 3);
                ImGui::Text("Always On-Top");
                ImGui::SameLine();
        
				ImGui::SetCursorPosY(ImGui::GetCursorPosY() + 3);
                if (ImGui::Checkbox("##OnTopToggle", &ontoptoggle))
                {
#if SMU_USE_SDL_UI
                    if (!SetGuiWindowTopmost(sdlWindow, hwnd, ontoptoggle)) {
						LogWarning("Always-on-top could not be applied to the SDL window on this platform.");
					}
#else
                    SetGuiWindowTopmost(hwnd, ontoptoggle);
#endif
                }

                ImGui::SameLine();
				// Adjust text starting point by 2 pixels downwards
				ImGui::SetCursorPosY(ImGui::GetCursorPosY() + 3);
                ImGui::Text("Opacity");
                ImGui::SameLine();
                ImGui::SetNextItemWidth(100.0f);
				// Adjust text starting point by 2 pixels downwards
				ImGui::SetCursorPosY(ImGui::GetCursorPosY() + 3);
                if (ImGui::SliderFloat("##OpacitySlider", &windowOpacityPercent, 20.0f, 100.0f, "%.0f%%"))
                {
#if SMU_USE_SDL_UI
                    if (!SetGuiWindowOpacity(sdlWindow, hwnd, windowOpacityPercent)) {
						LogWarning("Window opacity could not be applied to the SDL window on this platform.");
					}
#else
                    SetGuiWindowOpacity(hwnd, windowOpacityPercent);
#endif
                }

				// Patch the Background (Fill the empty rounded gaps with background color)
				// Bottom Left Corner Patch
				draw_list->AddRectFilled(
					ImVec2(childPos.x, childPos.y + childSize.y - rounding), 
					ImVec2(childPos.x + rounding, childPos.y + childSize.y), 
					bg_color
				);

				// Bottom Right Corner Patch
				draw_list->AddRectFilled(
					ImVec2(childPos.x + childSize.x - rounding, childPos.y + childSize.y - rounding), 
					ImVec2(childPos.x + childSize.x, childPos.y + childSize.y), 
					bg_color
				);

				// Re-draw the Borders (Square them off)
				// Draw a vertical line segment on the left to cover the curve
				draw_list->AddLine(
					ImVec2(childPos.x, childPos.y + childSize.y - rounding - 1),
					ImVec2(childPos.x, childPos.y + childSize.y + 29),
					border_color
				);

				// Draw a vertical line segment on the right to cover the curve
				draw_list->AddLine(
					ImVec2(childPos.x + childSize.x - 1, childPos.y + childSize.y - rounding - 1),
					ImVec2(childPos.x + childSize.x - 1, childPos.y + childSize.y + 29),
					border_color
				);

				// Draw the Separator Line
				// This is the line that sits between RightSection and BottomControls.
				// We draw it over the bottom border of RightSection to make it look like a seamless divider.
				draw_list->AddLine(
					ImVec2(childPos.x + 1, childPos.y + childSize.y - 1), 
					ImVec2(childPos.x + childSize.x - 1, childPos.y + childSize.y - 1), 
					bg_color, // Use BG color to "erase" the existing border
					1.0f
				);

				// Draw the Bottom Border
				draw_list->AddLine(
					ImVec2(childPos.x, childPos.y + childSize.y + 29), 
					ImVec2(childPos.x + childSize.x, childPos.y + childSize.y + 29), 
					border_color,
					1.0f
				);

				ImGui::SameLine();

				// Adjust text starting point by 2 pixels downwards
				ImGui::SetCursorPosY(ImGui::GetCursorPosY() + 3);
                ProfileUI::DrawProfileManagerUI();
            }

			ImGui::PopStyleVar();

            ImGui::EndChild();

            // Finish the main window
            ImGui::End(); // End main ImGui window

            // Render
            ImGui::Render();

#if SMU_USE_SDL_UI
			UpdateWindowMetrics(sdlWindow, hwnd);
#endif
            glViewport(0, 0, screen_width, screen_height);
            glClearColor(0.45f, 0.55f, 0.60f, 1.00f);
            glClear(GL_COLOR_BUFFER_BIT);

            ImGui_ImplOpenGL3_RenderDrawData(ImGui::GetDrawData());

#if SMU_USE_SDL_UI
            SDL_GL_SwapWindow(sdlWindow);
#else
            SwapBuffers(g_hDC);
#endif
			UpdateLagswitchOverlay();
			if (startupWarmupFramesRemaining > 0) {
				startupWarmupFramesRemaining--;
			}

			nextFrameTime += targetFrameDuration;
			auto nowAfterRender = std::chrono::steady_clock::now();
			if (nextFrameTime <= nowAfterRender) {
				nextFrameTime = nowAfterRender + targetFrameDuration;
			}
			std::this_thread::sleep_until(nextFrameTime);

		} else {
			// No rendering needed
			std::this_thread::sleep_for(std::chrono::milliseconds(1));
		}
	}

	ImGui_ImplOpenGL3_Shutdown();
#if SMU_USE_SDL_UI
    ImGui_ImplSDL3_Shutdown();
#else
    ImGui_ImplWin32_Shutdown();
#endif
    ImGui::DestroyContext();
#if SMU_USE_SDL_UI
	SDL_SetWindowsMessageHook(nullptr, nullptr);
	SDL_GL_DestroyContext(glContext);
	SDL_DestroyWindow(sdlWindow);
	SDL_Quit();
#else
    CleanupDeviceWGL(hwnd);

    UnregisterClass(wc.lpszClassName, wc.hInstance);
#endif
    if (mainThreadId != 0 && mainThreadId != guiThreadId) {
	    AttachThreadInput(mainThreadId, guiThreadId, FALSE);
	}
}

void DbgPrintf(const char* format, ...) {
    char buffer[256];
    va_list args;
    va_start(args, format);
    vsnprintf(buffer, sizeof(buffer), format, args); // Use vsnprintf for safety
    va_end(args);
    OutputDebugStringA(buffer); // Send to output
}

void CreateDebugConsole() {
    if (AllocConsole()) {
        FILE* pCout;
        freopen_s(&pCout, "CONOUT$", "w", stdout); // Redirect stdout
        FILE* pCerr;
        freopen_s(&pCerr, "CONOUT$", "w", stderr); // Redirect stderr
        FILE* pCin;
        freopen_s(&pCin, "CONIN$", "r", stdin);   // Redirect stdin

        SetConsoleTitle(L"SMC Debug Console");

        // Apply high-quality app icon to the console window
        HWND hConsole = GetConsoleWindow();
        if (hConsole) {
            HICON hIconLarge = (HICON)LoadImage(GetModuleHandle(NULL), MAKEINTRESOURCE(IDI_ICON1), IMAGE_ICON, 32, 32, LR_DEFAULTCOLOR);
            HICON hIconSmall = (HICON)LoadImage(GetModuleHandle(NULL), MAKEINTRESOURCE(IDI_ICON1), IMAGE_ICON, 16, 16, LR_DEFAULTCOLOR);
            SendMessage(hConsole, WM_SETICON, ICON_BIG,   (LPARAM)hIconLarge);
            SendMessage(hConsole, WM_SETICON, ICON_SMALL, (LPARAM)hIconSmall);
        }

        std::cout.sync_with_stdio(true);
    }
}

static void UpdateSMCThread() {
	// Check for any updates
    std::string remoteVersion = GetRemoteVersion();

    if (!remoteVersion.empty()) 
    {
        remoteVersion = Trim(remoteVersion);

        if (remoteVersion != localVersion) 
        {
            std::wstring_convert<std::codecvt_utf8_utf16<wchar_t>> converter;
            std::wstring wRemoteVersion = converter.from_bytes(remoteVersion);
            std::wstring wLocalVersion = converter.from_bytes(localVersion);
            
            std::wstring message = L"An update is available!\n\n"
                                   L"Current version: " + wLocalVersion + L"\n"
                                   L"Latest version:  " + wRemoteVersion + L"\n\n"
                                   L"Would you like to update now?";

            int result = MessageBoxW(NULL,
                message.c_str(),
                L"Update Available",
                MB_YESNOCANCEL | MB_ICONINFORMATION | MB_DEFBUTTON1 | MB_APPLMODAL);

            if (result == IDYES) 
            {
				PerformUpdate(remoteVersion, localVersion);
            }

			if (result == IDNO) 
			{
				UserOutdated = true;
			}
		
			if (result == IDCANCEL) 
			{
				exit(0);
			}
        }
    }
}

// START OF CODE THREAD

int WINAPI wWinMain(_In_ HINSTANCE hInstance, _In_opt_ HINSTANCE hPrevInstance, _In_ LPWSTR lpCmdLine, _In_ int nCmdShow)
{
    DisablePowerThrottling();

	// Create Debug Console, use DbgPrintf, printf, or cout to use
    char debugbuffer[16];
    if (GetEnvironmentVariableA("DEBUG", debugbuffer, sizeof(debugbuffer)) && std::string(debugbuffer) == "1") {
        CreateDebugConsole();
    } else {
		// Comment this back in to see the console on regular builds
		// CreateDebugConsole();
	}

	// Run timers with max precision
    timeBeginPeriod(1);

	// Do not let the main polling loop starve the GUI thread.
    SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_NORMAL);

	// Setup suspension

	smu::platform::SetNetworkLagBackend(CreateWinDivertNetworkLagBackend());

	// Initialize duplicatable macro instances with default values (settings loaded later by GUI thread)
	wallhop_instances.emplace_back();
	wallhop_instances[0].vk_trigger = vk_xbutton2;
	wallhop_instances[0].disable_outside_roblox = disable_outside_roblox[6];

	presskey_instances.emplace_back();
	presskey_instances[0].vk_trigger  = vk_zkey;
	presskey_instances[0].vk_presskey = vk_dkey;
	presskey_instances[0].presskeyinroblox = disable_outside_roblox[5];

	spamkey_instances.emplace_back();
	spamkey_instances[0].vk_trigger = vk_leftbracket;
	spamkey_instances[0].vk_spamkey = vk_spamkey;
	spamkey_instances[0].disable_outside_roblox = disable_outside_roblox[11];

	std::thread actionThread(Speedglitchloop); // Start a separate thread for item desync loop, lets functions run alongside
	std::thread actionThread2(ItemDesyncLoop);
	std::thread actionThread3(SpeedglitchloopHHJ);
	spamkey_threads.emplace_back(SpamKeyLoop, &spamkey_instances[0]);
	std::thread actionThread5(ItemClipLoop);
	std::thread actionThread6(WallWalkLoop);
	std::thread actionThread7(BhopLoop);
	wallhop_threads.emplace_back(WallhopThread, &wallhop_instances[0]);
	presskey_threads.emplace_back(PressKeyThread, &presskey_instances[0]);
	std::thread actionThread10(FloorBounceThread);
	std::thread logScannerThread(RobloxLogScannerThread);

	std::thread updaterThread(UpdateSMCThread);
	
	std::thread guiThread(RunGUI);

	std::thread KeyboardThread;

	if (!g_isLinuxWine) {
		KeyboardThread = std::thread(KeyboardHookThread);
	}

	MSG msg;

	TrimWhitespace(settingsBuffer);
	targetPIDs = FindProcessIdsByName_Compat(settingsBuffer, takeallprocessids);

    if (targetPIDs.empty()) {
        processFound = false;
    } else {
		processFound = true;
	}

	hProcess = GetProcessHandles_Compat(targetPIDs, PROCESS_SUSPEND_RESUME | PROCESS_QUERY_LIMITED_INFORMATION);
	std::vector<HWND> rbxhwnd = FindWindowByProcessHandle(hProcess); // SET ROBLOX WINDOW HWND RAHHHHH

	// These variables are used for "one-click" functionalies for macros, so you can press a key and it runs every time that key is pressed (without overlapping itself)
	bool isdesync = false;
	bool isSuspended = false; 
	bool islhj = false;
	bool ispresskey = false;
	bool iswallhop = false;
	bool isspeedglitch = false;
	bool isunequipspeed = false;
	bool HHJ = false;
	bool isspam = false;
	bool isclip = false;
	bool iswallwalk = false;
	bool islaugh = false;
	bool isbounce = false;
	bool isbhop = false;
	bool bhoplocked = false;
	bool isfloorbounce = false;
	static const float targetFrameTime = 1.0f / 90.0f; // Targeting 90 FPS
	auto lastPressTime = std::chrono::steady_clock::now();
	auto lastProcessCheck = std::chrono::steady_clock::now();
	auto startTime = std::chrono::steady_clock::now();
	auto now = std::chrono::steady_clock::now();
	auto processchecktime = std::chrono::steady_clock::now();
	static int counter = 0;

	while (!done) {
	bool tabbedintoroblox = IsForegroundWindowProcess(hProcess);
    {
		tabbedintoroblox = IsForegroundWindowProcess(hProcess);

		// --- Handle new/remove instance requests from GUI thread ---
		// Wallhop
		if (request_new_wallhop_instance.exchange(false)) {
			wallhop_instances.emplace_back();
			auto& src = wallhop_instances.front();
			auto& dst = wallhop_instances.back();
			dst.vk_trigger = src.vk_trigger;
			dst.WallhopDelay = src.WallhopDelay;
			dst.WallhopBonusDelay = src.WallhopBonusDelay;
			dst.wallhop_vertical = src.wallhop_vertical;
			strncpy_s(dst.WallhopPixels, sizeof(WallhopInstance::WallhopPixels), src.WallhopPixels, _TRUNCATE);
			strncpy_s(dst.WallhopVerticalChar, sizeof(WallhopInstance::WallhopVerticalChar), src.WallhopVerticalChar, _TRUNCATE);
			strncpy_s(dst.WallhopDelayChar, sizeof(WallhopInstance::WallhopDelayChar), src.WallhopDelayChar, _TRUNCATE);
			strncpy_s(dst.WallhopBonusDelayChar, sizeof(WallhopInstance::WallhopBonusDelayChar), src.WallhopBonusDelayChar, _TRUNCATE);
			strncpy_s(dst.WallhopDegrees, sizeof(WallhopInstance::WallhopDegrees), src.WallhopDegrees, _TRUNCATE);
			dst.wallhop_dx = src.wallhop_dx;  dst.wallhop_dy = src.wallhop_dy;
			dst.wallhopswitch = src.wallhopswitch;  dst.toggle_jump = src.toggle_jump;
			dst.toggle_flick = src.toggle_flick;    dst.wallhopcamfix = src.wallhopcamfix;
			dst.disable_outside_roblox = src.disable_outside_roblox;
			dst.vk_jumpkey = src.vk_jumpkey;
			dst.section_enabled = false; // start disabled; user enables explicitly
			wallhop_threads.emplace_back(WallhopThread, &wallhop_instances.back());
			selected_wallhop_instance = static_cast<int>(wallhop_instances.size()) - 1;
		}
		const int remove_wallhop_index = request_remove_wallhop_instance_index.exchange(-1, std::memory_order_acq_rel);
		if (remove_wallhop_index >= 0 && wallhop_instances.size() > 1) {
			RemoveRequestedInstance(wallhop_instances, wallhop_threads, remove_wallhop_index, CopyWallhopInstanceData);
			if (selected_wallhop_instance >= static_cast<int>(wallhop_instances.size()))
				selected_wallhop_instance = static_cast<int>(wallhop_instances.size()) - 1;
		}
		// Presskey
		if (request_new_presskey_instance.exchange(false)) {
			presskey_instances.emplace_back();
			auto& src = presskey_instances.front();
			auto& dst = presskey_instances.back();
			dst.vk_trigger = src.vk_trigger;  dst.vk_presskey = src.vk_presskey;
			dst.PressKeyDelay = src.PressKeyDelay;  dst.PressKeyBonusDelay = src.PressKeyBonusDelay;
			strncpy_s(dst.PressKeyDelayChar, sizeof(PresskeyInstance::PressKeyDelayChar), src.PressKeyDelayChar, _TRUNCATE);
			strncpy_s(dst.PressKeyBonusDelayChar, sizeof(PresskeyInstance::PressKeyBonusDelayChar), src.PressKeyBonusDelayChar, _TRUNCATE);
			dst.presskeyinroblox = src.presskeyinroblox;
			dst.section_enabled = false;
			presskey_threads.emplace_back(PressKeyThread, &presskey_instances.back());
			selected_presskey_instance = static_cast<int>(presskey_instances.size()) - 1;
		}
		const int remove_presskey_index = request_remove_presskey_instance_index.exchange(-1, std::memory_order_acq_rel);
		if (remove_presskey_index >= 0 && presskey_instances.size() > 1) {
			RemoveRequestedInstance(presskey_instances, presskey_threads, remove_presskey_index, CopyPresskeyInstanceData);
			if (selected_presskey_instance >= static_cast<int>(presskey_instances.size()))
				selected_presskey_instance = static_cast<int>(presskey_instances.size()) - 1;
		}
		// Spamkey
		if (request_new_spamkey_instance.exchange(false)) {
			spamkey_instances.emplace_back();
			auto& src = spamkey_instances.front();
			auto& dst = spamkey_instances.back();
			dst.vk_trigger = src.vk_trigger;  dst.vk_spamkey = src.vk_spamkey;
			dst.spam_delay = src.spam_delay;   dst.real_delay = src.real_delay;
			strncpy_s(dst.SpamDelay, sizeof(SpamkeyInstance::SpamDelay), src.SpamDelay, _TRUNCATE);
			dst.isspamswitch = src.isspamswitch;
			dst.disable_outside_roblox = src.disable_outside_roblox;
			dst.section_enabled = false;
			spamkey_threads.emplace_back(SpamKeyLoop, &spamkey_instances.back());
			selected_spamkey_instance = static_cast<int>(spamkey_instances.size()) - 1;
		}
		const int remove_spamkey_index = request_remove_spamkey_instance_index.exchange(-1, std::memory_order_acq_rel);
		if (remove_spamkey_index >= 0 && spamkey_instances.size() > 1) {
			RemoveRequestedInstance(spamkey_instances, spamkey_threads, remove_spamkey_index, CopySpamkeyInstanceData);
			if (selected_spamkey_instance >= static_cast<int>(spamkey_instances.size()))
				selected_spamkey_instance = static_cast<int>(spamkey_instances.size()) - 1;
		}
		// Start threads for extra instances that were loaded from a save file
		if (g_extra_instances_loaded.exchange(false)) {
			while (wallhop_threads.size() < wallhop_instances.size())
				wallhop_threads.emplace_back(WallhopThread, &wallhop_instances[wallhop_threads.size()]);
			while (presskey_threads.size() < presskey_instances.size())
				presskey_threads.emplace_back(PressKeyThread, &presskey_instances[presskey_threads.size()]);
			while (spamkey_threads.size() < spamkey_instances.size())
				spamkey_threads.emplace_back(SpamKeyLoop, &spamkey_instances[spamkey_threads.size()]);
		}
		// Freeze
		if ((macrotoggled && notbinding && section_toggles[0])) {
			bool isMButtonPressed = IsHotkeyPressed(vk_mbutton);

			if (isfreezeswitch) {  // Toggle mode
				if (isMButtonPressed && !wasMButtonPressed && ForegroundRestrictionAllows(disable_outside_roblox[0], tabbedintoroblox)) {  // Detect button press edge
					isSuspended = !isSuspended;  // Toggle the freeze state
					SuspendOrResumeProcesses_Compat(targetPIDs, hProcess, isSuspended);

					if (isSuspended) {
						suspendStartTime = std::chrono::steady_clock::now();  // Start the timer
					}
				}
			} else {  // Hold mode
				if (isMButtonPressed && ForegroundRestrictionAllows(disable_outside_roblox[0], tabbedintoroblox)) {
					if (!isSuspended) {
						SuspendOrResumeProcesses_Compat(targetPIDs, hProcess, true);  // Freeze on hold
						isSuspended = true;
						suspendStartTime = std::chrono::steady_clock::now();  // Start the timer
					}
				} else if (isSuspended) {
					SuspendOrResumeProcesses_Compat(targetPIDs, hProcess, false);  // Unfreeze on release
					isSuspended = false;
				}
			}

			// Common timer logic for both toggle and hold modes
			if (isSuspended) {
				auto elapsed = std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now() - suspendStartTime).count();

				if (elapsed >= (maxfreezetime * 1000)) {
					// Unsuspend for 50 ms
					SuspendOrResumeProcesses_Compat(targetPIDs, hProcess, false);
					std::this_thread::sleep_for(std::chrono::milliseconds(maxfreezeoverride));
					SuspendOrResumeProcesses_Compat(targetPIDs, hProcess, true);

					// Reset the timer
					suspendStartTime = std::chrono::steady_clock::now();
				}
			}

			// Update the previous state
			wasMButtonPressed = isMButtonPressed;
		}

		// Item Desync Macro with anti-idiot design
		if (IsHotkeyPressed(vk_f5) && ForegroundRestrictionAllows(disable_outside_roblox[1], tabbedintoroblox) && macrotoggled && notbinding && section_toggles[1]) {
			if (!isdesync) {
				isdesyncloop.store(true, std::memory_order_relaxed);
				isdesync = true;
			}
		} else {
			isdesync = false;
			isdesyncloop.store(false, std::memory_order_relaxed);
		}

		// PressKey — loop over all instances
		for (auto& inst : presskey_instances) {
			if (IsHotkeyPressed(inst.vk_trigger) && macrotoggled && notbinding && inst.section_enabled && ForegroundRestrictionAllows(inst.presskeyinroblox, tabbedintoroblox)) {
				if (!inst.isRunning) {
					inst.thread_active.store(true, std::memory_order_relaxed);
					inst.isRunning = true;
				}
			} else {
				inst.isRunning = false;
				inst.thread_active.store(false, std::memory_order_relaxed);
			}
		}

		// Wallhop — loop over all instances
		for (auto& inst : wallhop_instances) {
			if (IsHotkeyPressed(inst.vk_trigger) && macrotoggled && notbinding && inst.section_enabled && ForegroundRestrictionAllows(inst.disable_outside_roblox, tabbedintoroblox)) {
				if (!inst.isRunning) {
					inst.thread_active.store(true, std::memory_order_relaxed);
					inst.isRunning = true;
				}
			} else {
				inst.thread_active.store(false, std::memory_order_relaxed);
				inst.isRunning = false;
			}
		}

		// Walless LHJ (REQUIRES COM OFFSET AND .5 STUDS OF A FOOT ON A PLATFORM)
		if (IsHotkeyPressed(vk_f6) && macrotoggled && notbinding && section_toggles[7] && ForegroundRestrictionAllows(disable_outside_roblox[7], tabbedintoroblox)) {
			if (!islhj) {
				if (wallesslhjswitch) {
					HoldKey(0x1E);
				} else {
					HoldKey(0x20);
				}
				std::this_thread::sleep_for(std::chrono::milliseconds(15));
				HoldKey(0x39);
				std::this_thread::sleep_for(std::chrono::milliseconds(30));
				SuspendOrResumeProcesses_Compat(targetPIDs, hProcess, true);
				std::this_thread::sleep_for(std::chrono::milliseconds(50));
				if (wallesslhjswitch) {
					ReleaseKey(0x1E);
				} else {
					ReleaseKey(0x20);
				}
				std::this_thread::sleep_for(std::chrono::milliseconds(500));

				if (!globalzoomin) {
					HoldKeyBinded(vk_shiftkey);
				} else {
					HoldKeyBinded(globalzoominreverse ? VK_MOUSE_WHEEL_DOWN : VK_MOUSE_WHEEL_UP);
				}
				SuspendOrResumeProcesses_Compat(targetPIDs, hProcess, false);
				std::this_thread::sleep_for(std::chrono::milliseconds(50));
				if (!globalzoomin) {
					ReleaseKeyBinded(vk_shiftkey);
				}

				ReleaseKey(0x39);
				islhj = true;
			}
		} else {
			islhj = false;
		}

		// Speedglitch
		if (IsHotkeyPressed(vk_xkey) && ForegroundRestrictionAllows(disable_outside_roblox[3], tabbedintoroblox) && macrotoggled && notbinding && section_toggles[3]) {
			if (!isspeedglitch) {
				isspeed = !isspeed;
				isspeedglitch = true;
			}
		} else {
			isspeedglitch = false;
			if (isspeedswitch) {
				isspeed = false;
			}
		}

		// Gear Unequip COM Offset
		if (IsHotkeyPressed(vk_f8) && macrotoggled && notbinding && section_toggles[4] && ForegroundRestrictionAllows(disable_outside_roblox[4], tabbedintoroblox)) {
			if (!isunequipspeed) {
				if (chatoverride) {
					HoldKey(0x35);
				} else {
					HoldKeyBinded(vk_chatkey);
					std::this_thread::sleep_for(std::chrono::milliseconds(17));
					ReleaseKeyBinded(vk_chatkey);
				}

				std::this_thread::sleep_for(std::chrono::milliseconds(17));
				if (chatoverride) {
					ReleaseKey(0x35);
				}

				if (strlen(CustomTextChar) >= 1) {
					PasteText(CustomTextChar);
					std::this_thread::sleep_for(std::chrono::milliseconds(25));
					HoldKeyBinded(vk_enterkey);

					std::this_thread::sleep_for(std::chrono::milliseconds(35));
					ReleaseKeyBinded(vk_enterkey);
					isunequipspeed = true;
					continue;
				}

				PasteText(text);
				std::this_thread::sleep_for(std::chrono::milliseconds(25));
				HoldKeyBinded(vk_enterkey);

				std::this_thread::sleep_for(std::chrono::milliseconds(35));
				ReleaseKeyBinded(vk_enterkey);
				if (selected_dropdown == 2) { // Cheer
					std::this_thread::sleep_for(std::chrono::milliseconds(16));
				} else {
					std::this_thread::sleep_for(std::chrono::milliseconds(65));
				}

				if (selected_dropdown == 0) { // Dance
					std::this_thread::sleep_for(std::chrono::milliseconds(815));
				}
				if (selected_dropdown == 1) { // Laugh
					std::this_thread::sleep_for(std::chrono::milliseconds(175));
				}
				HoldKey(speed_slot + 1);
				std::this_thread::sleep_for(std::chrono::milliseconds(4));
				ReleaseKey(speed_slot + 1);
				std::this_thread::sleep_for(std::chrono::milliseconds(4));
				if (!unequiptoggle) {
					HoldKey(speed_slot + 1);
				}
				ReleaseKey(speed_slot + 1);
				isunequipspeed = true;
			}
		} else {
			isunequipspeed = false;
		}

		// Helicopter High jump
		if (IsHotkeyPressed(vk_xbutton1) && macrotoggled && notbinding && section_toggles[2] && ForegroundRestrictionAllows(disable_outside_roblox[2], tabbedintoroblox)) {
			if (!HHJ) {
				if (autotoggle) { // Auto-Key-Timer
					HoldKeyBinded(vk_autohhjkey1);
					std::this_thread::sleep_for(std::chrono::milliseconds(AutoHHJKey1Time));
					HoldKeyBinded(vk_autohhjkey2);
					std::this_thread::sleep_for(std::chrono::milliseconds(AutoHHJKey2Time));
				}

				SuspendOrResumeProcesses_Compat(targetPIDs, hProcess, true);

				// Freeze timing logic: if not applying a manual override, use dynamic defaults.
				if (!HHJFreezeDelayApply) {
					// FastHHJ lowers freeze time by 300 ms
					if (!fasthhj) {
						std::this_thread::sleep_for(std::chrono::milliseconds(300));
					}
					std::this_thread::sleep_for(std::chrono::milliseconds(200));
				} else {
					// If user set override <= 0, freezing will not occur.
					if (HHJFreezeDelayOverride <= 0) {
						// no freeze
					} else {
						std::this_thread::sleep_for(std::chrono::milliseconds(HHJFreezeDelayOverride));
					}
				}

				if (autotoggle) { // Auto-Key-Timer
					ReleaseKeyBinded(vk_autohhjkey1);
					ReleaseKeyBinded(vk_autohhjkey2);
				}

				SuspendOrResumeProcesses_Compat(targetPIDs, hProcess, false);

				std::this_thread::sleep_for(std::chrono::milliseconds(HHJDelay1));
				if (!globalzoomin) {
					HoldKeyBinded(vk_shiftkey);
				} else {
					HoldKeyBinded(globalzoominreverse ? VK_MOUSE_WHEEL_DOWN : VK_MOUSE_WHEEL_UP);
				}
				std::this_thread::sleep_for(std::chrono::milliseconds(HHJDelay2));
				// Only enable the HHJ "flick" movement when a positive length is configured.
				// If HHJLength <= 0 we skip setting isHHJ to avoid a one-frame flick.
				if (HHJLength > 0) {
					isHHJ.store(true, std::memory_order_relaxed);
					std::this_thread::sleep_for(std::chrono::milliseconds(HHJDelay3));
				} else {
					// Still honor the delay even when not enabling the flick movement
					std::this_thread::sleep_for(std::chrono::milliseconds(HHJDelay3));
				}
				if (!globalzoomin) {
					ReleaseKeyBinded(vk_shiftkey);
				}
				if (HHJLength > 0) {
					std::this_thread::sleep_for(std::chrono::milliseconds(HHJLength));
					isHHJ.store(false, std::memory_order_relaxed);
				}
				HHJ = true;
			}
		} else {
			HHJ = false;
		}

		// Spamkey — loop over all instances
		for (auto& inst : spamkey_instances) {
			const bool shouldRunSpam =
				IsHotkeyPressed(inst.vk_trigger) && macrotoggled && notbinding && inst.section_enabled && ForegroundRestrictionAllows(inst.disable_outside_roblox, tabbedintoroblox);

			if (shouldRunSpam) {
				if (!inst.isRunning) {
					inst.thread_active.store(!inst.thread_active.load(std::memory_order_relaxed), std::memory_order_relaxed);
					inst.isRunning = true;
				}
			} else {
				inst.isRunning = false;
				if (!ForegroundRestrictionAllows(inst.disable_outside_roblox, tabbedintoroblox) || inst.isspamswitch) {
					inst.thread_active.store(false, std::memory_order_relaxed);
				}
			}
		}

		// Laughkey
		if (IsHotkeyPressed(vk_laughkey) && ForegroundRestrictionAllows(disable_outside_roblox[9], tabbedintoroblox) && macrotoggled && notbinding && section_toggles[9]) {
			if (!islaugh) {
				if (chatoverride) {
					HoldKey(0x35);
				} else {
					HoldKeyBinded(vk_chatkey);
					std::this_thread::sleep_for(std::chrono::milliseconds(17));
					ReleaseKeyBinded(vk_chatkey);
				}

				std::this_thread::sleep_for(std::chrono::milliseconds(50));
				if (chatoverride) {
					ReleaseKey(0x35);
				}

				PasteText("/e laugh");
				std::this_thread::sleep_for(std::chrono::milliseconds(50));
				HoldKey(0x1C);

				std::this_thread::sleep_for(std::chrono::milliseconds(35));
				ReleaseKey(0x1C);

				std::this_thread::sleep_for(std::chrono::milliseconds(248));

				HoldKey(0x39); // Jump
				if (!globalzoomin) {
					HoldKeyBinded(vk_shiftkey);
				} else {
					HoldKeyBinded(globalzoominreverse ? VK_MOUSE_WHEEL_DOWN : VK_MOUSE_WHEEL_UP);
				}

				if (!laughmoveswitch) {
					HoldKey(0x1F); // S
				}

				std::this_thread::sleep_for(std::chrono::milliseconds(25));
				ReleaseKey(0x39);

				if (!globalzoomin) {
					ReleaseKeyBinded(vk_shiftkey);
				}
				ReleaseKey(0x1C);
				std::this_thread::sleep_for(std::chrono::milliseconds(25));

				if (!laughmoveswitch) {
					ReleaseKey(0x1F); // S
				}

				islaugh = true;
			}
		} else {
			islaugh = false;
		}


		// Ledge Bounce
		if (IsHotkeyPressed(vk_bouncekey) && ForegroundRestrictionAllows(disable_outside_roblox[12], tabbedintoroblox) && macrotoggled && notbinding && section_toggles[12]) {
			if (!isbounce) {
				int turn90 = static_cast<int>((camfixtoggle ? 250 : 180) / atof(RobloxSensValue));
				int skey = 0x1F; // S key
				int dkey = 0x20; // D key
				int wkey = 0x11; // W key

				if (bouncesidetoggle) {
					turn90 = -turn90;
					dkey = 0x1E; // A Key
				}

				MoveMouse(-turn90, 0); // Turn Left
				std::this_thread::sleep_for(std::chrono::milliseconds(90));
				HoldKey(skey); // Hold S
				std::this_thread::sleep_for(std::chrono::milliseconds(40));
				ReleaseKey(skey); // Release S
				MoveMouse(turn90, 0); // Turn Right
				HoldKey(dkey);    // Hold D


				std::this_thread::sleep_for(std::chrono::milliseconds(16));

				if (bouncerealignsideways) {
					ReleaseKey(dkey); // Release D
					HoldKey(wkey); // Hold W
				}

				if (!bouncerealignsideways) {
					ReleaseKey(dkey);
				}

				// After Bounce
				if (bouncerealignsideways) {
					MoveMouse(turn90, 0); // Turn Right to face towards ledge
					std::this_thread::sleep_for(std::chrono::milliseconds(70));
					ReleaseKey(wkey);      // Release W
					MoveMouse(-turn90, 0); // Turn Left to face normally
					// Right facing end
					if (bounceautohold) {
						HoldKey(dkey); // Hold D
					}
				} else {
					// Front Facing End
					if (bounceautohold) {
						HoldKey(wkey); // Hold W
					}
					MoveMouse(turn90, 0); // Turn Right
				}

				isbounce = true;
			}
		} else {
			isbounce = false;
		}


		// Item Clip
		if (IsHotkeyPressed(vk_clipkey) && ForegroundRestrictionAllows(disable_outside_roblox[8], tabbedintoroblox) && macrotoggled && notbinding && section_toggles[8]) {
			if (!isclip) {
				isitemloop = !isitemloop;
				isclip = true;
			}
		} else {
			isclip = false;
			if (isitemclipswitch) {
				isitemloop = false;
			}
		}


		// WallWalk
		if (IsHotkeyPressed(vk_wallkey) && ForegroundRestrictionAllows(disable_outside_roblox[10], tabbedintoroblox) && macrotoggled && notbinding && section_toggles[10]) {
			if (!iswallwalk) {
				iswallwalkloop = !iswallwalkloop;
				iswallwalk = true;
			}
		} else {
			iswallwalk = false;
			if (iswallwalkswitch) {
				iswallwalkloop = false;
			}
		}

		bool can_process_bhop = ForegroundRestrictionAllows(disable_outside_roblox[13], tabbedintoroblox) && section_toggles[13] && macrotoggled && notbinding;
		if (IsHotkeyPressed(vk_chatkey)) {
			bhoplocked = true;
		}
		
		if (bhoplocked) {
			if (IsKeyPressed(VK_RETURN) || IsKeyPressed(VK_LBUTTON)) {
				bhoplocked = false;
			}
		}
		
		if (can_process_bhop) {
			bool is_bunnyhop_key_considered_held = g_isVk_BunnyhopHeldDown.load(std::memory_order_relaxed) || IsHotkeyPressed(vk_bunnyhopkey);

			if (bunnyhopsmart) {
				if (!bhoplocked && is_bunnyhop_key_considered_held) {
					if (!isbhop) {
						isbhoploop = true;
						isbhop = true;
					}
				} else {
					if (isbhop) {
						isbhoploop = false;
						isbhop = false;
					}
				}
			} else {
				if (is_bunnyhop_key_considered_held) {
					if (!isbhop) {
						isbhoploop = true;
						isbhop = true;
					}
				} else {
					if (isbhop) {
						isbhoploop = false;
						isbhop = false;
					}
				}
			}
		}

		// Floor Bounce
		if (IsHotkeyPressed(vk_floorbouncekey) && ForegroundRestrictionAllows(disable_outside_roblox[14], tabbedintoroblox) && macrotoggled && notbinding && section_toggles[14]) {
			if (!isfloorbounce) {
				isfloorbouncethread = true;
				isfloorbounce = true;
			}
		} else {
			if (isfloorbounce) {
				isfloorbounce = false;
			}
		}

		if (macrotoggled && notbinding && section_toggles[15]) {
			if (bWinDivertEnabled) {
				static bool wasLagSwitchPressed = false;
                static bool prev_blocking_state = false;
				bool isPressed = IsHotkeyPressed(vk_lagswitchkey);
                
                // Determine the "Logical" state (Does the user want it on?)
                bool logical_on_state = false;

				if (islagswitchswitch) {
					// TOGGLE MODE
					if (isPressed && !wasLagSwitchPressed) {
						SetLagSwitchBlocking(!g_windivert_blocking.load(std::memory_order_relaxed)); // Flip state
					}
                    logical_on_state = g_windivert_blocking.load(std::memory_order_relaxed);
					wasLagSwitchPressed = isPressed;
				} else {
					// HOLD MODE
					SetLagSwitchBlocking(isPressed);
                    logical_on_state = isPressed;
				}

                // Detect when Lag Switch was JUST turned on to reset the timer
                if (logical_on_state && !prev_blocking_state) {
                    lagswitch_start_time = std::chrono::steady_clock::now();
                }
                prev_blocking_state = logical_on_state;

                // Auto-Unlag Logic
                if (logical_on_state && lagswitch_autounblock) {
                    auto elapsed = std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now() - lagswitch_start_time).count();
                    
                    if (elapsed >= (lagswitch_max_duration * 1000)) {
                        // 1. Unblock momentarily (Let traffic pass)
                        SetLagSwitchBlocking(false);
                        
                        // 2. Wait
                        std::this_thread::sleep_for(std::chrono::milliseconds(lagswitch_unblock_ms));
                        
                        // 3. Determine if we should Re-Lag
                        bool should_resume = false;

                        if (islagswitchswitch) {
                            // In Toggle Mode, if we were logically on, we STAY on.
                            should_resume = true;
                        } else {
                            // In Hold Mode, we only resume if the key is STILL being held down.
                            if (IsHotkeyPressed(vk_lagswitchkey)) {
                                should_resume = true;
                            }
                        }

                        if (should_resume) {
                            SetLagSwitchBlocking(true);
                        }
                        
                        // 4. Reset timer
                        lagswitch_start_time = std::chrono::steady_clock::now();
                    }
                }

			} else {
                // If user presses key but driver isn't loaded, prompt for admin
				if (IsHotkeyPressed(vk_lagswitchkey)) {
					if (IsRunAsAdmin()) {
						std::string backendError;
						if (InitializeLagSwitchBackend(&backendError)) {
							// Allow 200ms for the driver to load before toggling on
							std::this_thread::sleep_for(std::chrono::milliseconds(200));
							SetLagSwitchBlocking(true);
						} else {
							if (!backendError.empty()) {
								LogCritical(backendError);
							}
							std::cerr << "Failed to load WinDivert files." << std::endl;
						}
					} else if (DontShowAdminWarning) {
						RestartAsAdmin();
					} else {
						bShowAdminPopup = true;
					}
				}
			}
        } else {
            // Safety: If module is disabled or macrotoggle is off, ensure we aren't blocking internet
            if (!section_toggles[15] || !macrotoggled) {
                SetLagSwitchBlocking(false);
            }
        }

		// Every second, check if roblox continues to exist.
		if (++counter % 100 == 0) {  // Check time every 100th iteration
			now = std::chrono::steady_clock::now();
			if (std::chrono::duration_cast<std::chrono::seconds>(now - processchecktime).count() >= 1) {

				targetPIDs = FindProcessIdsByName_Compat(settingsBuffer, takeallprocessids);

				if (targetPIDs.empty()) {
					processFound = false;
				} else {
					processFound = true;
				}

				hProcess = GetProcessHandles_Compat(targetPIDs, PROCESS_SUSPEND_RESUME | PROCESS_QUERY_LIMITED_INFORMATION);
				rbxhwnd = FindWindowByProcessHandle(hProcess);
				counter = 0;
				processchecktime = std::chrono::steady_clock::now();
			}
		}

		// Anti AFK (MUST STAY AT THE LOWEST PART OF THE LIST!!!)
		if ((!isafk && tabbedintoroblox) || !antiafktoggle) {
			// Not Afk, reset lastpresstime
			lastPressTime = std::chrono::steady_clock::now();
		} else {
			if (processFound && antiafktoggle && isafk && !g_isLinuxWine) {
				// Check if AntiAFKTime has passed
				auto elapsedMinutes = std::chrono::duration_cast<std::chrono::minutes>(now - lastPressTime).count();

				if (elapsedMinutes >= AntiAFKTime) {
					HWND originalHwnd = GetForegroundWindow();
					// Not tabbed into Roblox AntiAFK
					if (!IsForegroundWindowProcess(hProcess)) {
						// Save User Mouse Position to return to later
						POINT originalMousePos;
						GetCursorPos(&originalMousePos);

						HWND windowhwnd = FindNewestProcessWindow(rbxhwnd);
						SetWindowPos(windowhwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
						SetWindowPos(windowhwnd, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);

						// Get roblox rectangle coords
						RECT robloxRect;
						if (!GetWindowRect(windowhwnd, &robloxRect)) {
							std::cerr << "Failed to get window rect" << std::endl;
							continue; // or break depending on your loop
						}

						// Calculate center coordinates
						int centerX = robloxRect.left + (robloxRect.right - robloxRect.left) / 2;
						int centerY = robloxRect.top + (robloxRect.bottom - robloxRect.top) / 2;

						Sleep(300);

						// Move mouse to center of the wanted window using SetCursorPos
						SetCursorPos(centerX, centerY);

						// Click and Unclick LMB to get window focus
						INPUT mclickinput = {0};
						mclickinput.type = INPUT_MOUSE;
						mclickinput.mi.dwFlags = MOUSEEVENTF_LEFTDOWN;
						SendInput(1, &mclickinput, sizeof(INPUT));
						mclickinput.mi.dwFlags = MOUSEEVENTF_LEFTUP;
						Sleep(50);
						SendInput(1, &mclickinput, sizeof(INPUT));

						Sleep(500);

						if (doublepressafkkey) {
							HoldKeyBinded(vk_afkkey);
							Sleep(20);
							ReleaseKeyBinded(vk_afkkey);
							Sleep(20);
							HoldKeyBinded(vk_afkkey);
							Sleep(20);
							ReleaseKeyBinded(vk_afkkey);
						} else {
							HoldKeyBinded(vk_afkkey);
							Sleep(20);
							ReleaseKeyBinded(vk_afkkey);
						}
						Sleep(200);
						SetWindowPos(originalHwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
						SetWindowPos(originalHwnd, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
						Sleep(50);
						SetForegroundWindow(originalHwnd);
						Sleep(1000);

						SetCursorPos(originalMousePos.x, originalMousePos.y);

						lastPressTime = std::chrono::steady_clock::now();
					}
					// Pre Tabbed into Roblox
					if (IsForegroundWindowProcess(hProcess) && !g_isLinuxWine) {
						if (chatoverride) {
							HoldKey(0x35);
						} else {
							HoldKeyBinded(vk_chatkey);
							std::this_thread::sleep_for(std::chrono::milliseconds(17));
							ReleaseKeyBinded(vk_chatkey);
						}
						if (chatoverride) {
							Sleep(20);
							ReleaseKey(0x35);
						}
						Sleep(20);
						HoldKey(0x34);
						Sleep(20);
						ReleaseKey(0x34);
						Sleep(20);
						HoldKey(0x1C);
						Sleep(20);
						ReleaseKey(0x1C);
						Sleep(500);
						lastPressTime = std::chrono::steady_clock::now();
					}
				}
			}
		}
		// You are technically always AFK until the keyboard thread says otherwise
		isafk = true;
	}
	
	// Automatically turn off these 4 modules if you leave roblox window (so it isn't annoying)
	if (!tabbedintoroblox) {
		if (disable_outside_roblox[13]) {
			isbhoploop = false;
		}
		if (disable_outside_roblox[10]) {
			iswallwalkloop = false;
		}
		if (disable_outside_roblox[8]) {
			isitemloop = false;
		}
		if (disable_outside_roblox[1]) {
			isdesyncloop = false;
		}
		if (disable_outside_roblox[3]) {
			isspeed = false;
		}
	}

	std::this_thread::sleep_for(std::chrono::microseconds(50)); // Delay between main code loop (so your cpu doesn't die instantly)

}

	// Save Window Positions and Size before closing
	RECT windowrect;
	if (hwnd && IsWindow(hwnd) && GetWindowRect(hwnd, &windowrect)) {
		if (windowrect.left < 0) {
			WindowPosX = 0;
		} else {
			WindowPosX = windowrect.left;
		}

		if (windowrect.top < 0) {
			WindowPosY = 0;
		} else {
			WindowPosY = windowrect.top;
		}

		RECT screen_rect;

		GetWindowRect(hwnd, &screen_rect);

		screen_width = screen_rect.right - screen_rect.left;
		screen_height = screen_rect.bottom - screen_rect.top;
	}

	// Auto-promote (default) edits to a named profile if settings were changed
	PromoteDefaultProfileIfDirty(G_SETTINGS_FILEPATH);

	// Save current profile on exit
	if (!G_CURRENTLY_LOADED_PROFILE_NAME.empty() && G_CURRENTLY_LOADED_PROFILE_NAME != "(default)") {
		SaveSettings(G_SETTINGS_FILEPATH, G_CURRENTLY_LOADED_PROFILE_NAME);
	}

	if (g_keyboardHook) {
		UnhookWindowsHookEx(g_keyboardHook);
		g_keyboardHook = NULL;
	}

	CleanupOverlay(); // Destroys the window context

	running = false;
	ShutdownLagSwitchBackend();

	StopKeyboardThread();

	actionThread.join();
	actionThread2.join();
	actionThread3.join();
	for (auto& t : spamkey_threads) t.join();
	actionThread5.join();
	actionThread6.join();
	actionThread7.join();
	for (auto& t : wallhop_threads) t.join();
	for (auto& t : presskey_threads) t.join();
	actionThread10.join();

	g_log_thread_running = false;
	logScannerThread.join();

	guiThread.join();
	

	if (IsWindow(hwnd)) {
		DestroyWindow(hwnd);
	}
	
	if (IsRunAsAdmin()) {
		quiet_system("sc stop WinDivert >nul 2>&1"); // Remove windivert service upon closing the app
		quiet_system("sc delete WinDivert >nul 2>&1"); // Remove windivert service upon closing the app
		quiet_system("sc query WinDivert >nul 2>&1"); // Query WinDivert to update windows about its status
	}

	KeyboardThread.join();

	ShutdownLinuxCompatLayer();

	timeEndPeriod(1);
	exit(0);

	return 0;

}

#define NOMINMAX
#include <windows.h>
#include "resource.h"

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

#include "imgui-files/imgui.h"
#include "imgui-files/imgui_impl_opengl3.h"
#include "imgui-files/imgui_impl_win32.h"

#include "Resource Files/json.hpp"
#include "Resource Files/globals.h"
#include "Resource Files/miniz.h"
#include "Resource Files/keymapping.h"
#include "Resource Files/wine_compatibility_layer.h"
#include "Resource Files/updater_system.h"
#include "Resource Files/overlay.h"
#include "Resource Files/network_manager.h"
#include "Resource Files/macros.h"
#include "Resource Files/profile_manager.h"
#include "Resource Files/theme_manager.h"

#include <comdef.h>
#include <shlobj.h>
#include <condition_variable>
#include <fstream>
#include <filesystem>
#include <locale>
#include <codecvt>
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

// Generic libraries for I forgot

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

using json = nlohmann::json;

extern IMGUI_IMPL_API LRESULT ImGui_ImplWin32_WndProcHandler(HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam);

LRESULT WINAPI WndProc(HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam);

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

std::thread WinDivertThread;

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

// This is ran in a separate thread to avoid interfering with other functions

LRESULT CALLBACK LowLevelKeyboardProc(int nCode, WPARAM wParam, LPARAM lParam) {
    if (nCode == HC_ACTION) {
        // Reset AFK on any key activity
        isafk.store(false, std::memory_order_relaxed);
        
        const KBDLLHOOKSTRUCT* pkbhs = reinterpret_cast<const KBDLLHOOKSTRUCT*>(lParam);
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

    UnhookWindowsHookEx(g_keyboardHook);
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

    bool IsWheelUp() const { return wheelUp.load(); }
    bool IsWheelDown() const { return wheelDown.load(); }
};

MouseWheelHandler g_mouseWheel;

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


LRESULT CALLBACK WndProc(HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam)
{
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
		ReleaseDC(hWnd, hDc);
		return false;
	}
	if (SetPixelFormat(hDc, pf, &pfd) == FALSE) {
		ReleaseDC(hWnd, hDc);
		return false;
	}

	g_hRC = wglCreateContext(hDc);
	
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
    {"Helicopter High Jump", "Use COM Offset to Catapult Yourself Into The Air by Aligning your Back Angled to the Wall and Jumping and Letting Your Character Turn"},
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

static unsigned int BindKeyMode(unsigned int* keyVar, unsigned int currentkey, int selected_section)
{
    // Get or create the state for this specific key variable
    BindingState& state = g_bindingStates[keyVar];
    if (state.bindingMode) {
	    notbinding = false;
        state.rebindTime = std::chrono::steady_clock::now();
        // Initialize key states on first run in binding mode
        if (state.firstRun) {
            for (int key = 1; key < 258; key++) {
                state.keyWasPressed[key] = IsKeyPressed(key);
            }
            state.firstRun = false;
        }
		// Check side-specific shifts first to avoid ambiguity
		static const int priorityKeys[] = {0xA1, 0xA0, 0x10}; // VK_RSHIFT, VK_LSHIFT, VK_SHIFT
		for (int priorityKey : priorityKeys) {
			bool currentlyPressed = IsKeyPressed(priorityKey);
			if (currentlyPressed && !state.keyWasPressed[priorityKey]) {
				state.bindingMode = false;
				state.firstRun = true;
				state.keyWasPressed[priorityKey] = true;
				std::snprintf(state.keyBuffer, sizeof(state.keyBuffer), "0x%02x", priorityKey);
				return priorityKey;
			}
			state.keyWasPressed[priorityKey] = currentlyPressed;
		}

		// Check all other keys
		for (int key = 1; key < 258; key++) {
			// Skip the priority keys we already checked
			if (key == 0xA1 || key == 0xA0 || key == 0x10) continue;
    
			bool currentlyPressed = IsKeyPressed(key);
			// Key was previously pressed and is now released - reset it
			if (state.keyWasPressed[key] && !currentlyPressed) {
				state.keyWasPressed[key] = false;
			}
			// Key is pressed now AND it wasn't pressed before (fresh press)
			if (currentlyPressed && !state.keyWasPressed[key]) {
				state.bindingMode = false;
				state.firstRun = true;
				state.keyWasPressed[key] = true;
        
				std::snprintf(state.keyBuffer, sizeof(state.keyBuffer), "0x%02x", key);
				return key;
			}
			// Update the previous state
			state.keyWasPressed[key] = currentlyPressed;
		}
    } else {
        // Reset firstRun when not in binding mode
        state.firstRun = true;
        // Check if the selected_section has changed
        if (selected_section != state.lastSelectedSection) {
            std::snprintf(state.keyBuffer, sizeof(state.keyBuffer), "0x%02x", currentkey);
            state.lastSelectedSection = selected_section;
        }
		// Fix for settings menu being broken if no other menu is open
		if (selected_section == -1) {
			std::snprintf(state.keyBuffer, sizeof(state.keyBuffer), "0x%02x", currentkey);
		}
        state.buttonText = "Click to Bind Key";
        auto currentTime = std::chrono::steady_clock::now();
        std::chrono::duration<double> elapsedtime = currentTime - state.rebindTime;

		// Wait .3 seconds after binding before allowing macro to trigger
        if (elapsedtime.count() >= 0.3) {
            state.notBinding = true; // I honestly forgot the point of this line
			notbinding = true;
        }
    }
    
    return currentkey;
}

static void GetKeyNameFromHex(unsigned int hexKeyCode, char* buffer, size_t bufferSize)
{
    // Clear the buffer
    memset(buffer, 0, bufferSize);

    // Map the virtual key code to a scan code
    UINT scanCode = MapVirtualKey(hexKeyCode, MAPVK_VK_TO_VSC);

    // Check if THIS specific key is currently being bound
    bool thisKeyIsBinding = false;
    for (auto &[keyPtr, state] : g_bindingStates) {
	    if (state.bindingMode && *keyPtr == hexKeyCode) {
		    thisKeyIsBinding = true;
		    break;
	    }
    }

    if (thisKeyIsBinding) { // Only disable for the key being bound
	    return;
    }

    // Attempt to get the readable key name
    if (GetKeyNameTextA(scanCode << 16, buffer, bufferSize) > 0) {
        // Successfully retrieved the key name
        return;
    } else {
        // If GetKeyNameText fails, try to find the VK_ name
        auto it = vkToString.find(hexKeyCode);
        if (it != vkToString.end()) {
            strncpy(buffer, it->second.c_str(), bufferSize - 1);
            buffer[bufferSize - 1] = '\0'; // Ensure null termination
        } else {
            // If not found, return a default hex representation
            snprintf(buffer, bufferSize - 1, "0x%X", hexKeyCode);
            buffer[bufferSize - 1] = '\0';
        }
    }
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

static bool renameRMCToSMC() {
    namespace fs = std::filesystem;
    const fs::path source = "RMCSettings.json";
    const fs::path target = "SMCSettings.json";

    // Check if the source exists
    if (!fs::exists(target)) {
	    if (fs::exists(source)) {
		    try {
			    // Rename the file
			    fs::rename(source, target);
			    return true;
		    } catch (const fs::filesystem_error &e) {
			    std::cerr << "Error renaming file: " << e.what() << std::endl;
			    return false;
		    }
	    }
    }
    
    // Source didn't exist, so no action needed.
    return true;
}

// START OF GUI
static void RunGUI() {
	// Set working directory to correct path
    SetWorkingDirectoryToExecutablePath();

	renameRMCToSMC();
    G_SETTINGS_FILEPATH = getSettingsFileName();

	// Setup Linux Compatibility Layer if Needed
	InitLinuxCompatLayer();

	// Gui Thread has lower priority
	SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_LOWEST);

	// Load Default Themes into memory to be overwritten
	ThemeManager::Initialize();

	// Initialize a basic Win32 window
	WNDCLASSEX wc = { sizeof(WNDCLASSEX), CS_CLASSDC, WndProc, 0L, 0L, GetModuleHandle(NULL), NULL, NULL, NULL, NULL, _T("Spencer Macro Client"), NULL };

	// Load icons
	wc.hIcon = LoadIcon(wc.hInstance, MAKEINTRESOURCE(IDI_ICON1));
	wc.hIconSm = LoadIcon(wc.hInstance, MAKEINTRESOURCE(IDI_ICON1));

	// Check and load if settings currently exist
	TryLoadLastActiveProfile(G_SETTINGS_FILEPATH);

	// Override old default profile to new one
	SaveSettings(G_SETTINGS_FILEPATH, "SAVE_DEFAULT_90493");

	renameRMCToSMC();
    G_SETTINGS_FILEPATH = getSettingsFileName();
	
	// Reload Save file again since we updated our defaults
	TryLoadLastActiveProfile(G_SETTINGS_FILEPATH);

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
	wglMakeCurrent(g_hDC, g_hRC);

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

    // Initialize ImGui for Win32 and DirectX 11
    ImGui_ImplWin32_Init(hwnd);
	ImGui_ImplOpenGL3_Init("#version 130");

	auto lastTime = std::chrono::steady_clock::now();
	constexpr float targetFrameTime = 1.0f / 90.0f;  // 90 FPS target
	auto nextFrameTime = lastTime + std::chrono::duration<float>(targetFrameTime);

	InitializeSections();

	MSG msg;

	// Update Specific Variables on startup (Update int values from the corresponding char values)

	#define SAFE_CONVERT_INT(var, src) \
		try { var = std::stoi(src); } catch (...) {}

	#define SAFE_CONVERT_FLOAT(var, src) \
		try { var = std::stof(src); } catch (...) {}

	#define SAFE_CONVERT_DOUBLE(var, src) \
		try { var = std::stod(src); } catch (...) {}

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
    DWORD mainThreadId = GetWindowThreadProcessId(hwnd, NULL);
    DWORD guiThreadId = GetCurrentThreadId();
    AttachThreadInput(mainThreadId, guiThreadId, TRUE); // Attach the threads

    // Set window flags to disable resizing, moving, and title bar
    ImGuiWindowFlags window_flags = ImGuiWindowFlags_NoResize |
                                        ImGuiWindowFlags_NoMove |
                                        ImGuiWindowFlags_NoTitleBar |
										ImGuiWindowFlags_NoScrollbar |
										ImGuiWindowFlags_NoScrollWithMouse |
										ImGuiWindowFlags_NoBringToFrontOnFocus;

	// Check if the display scale is accurate to the macro
	CheckDisplayScale(hwnd, display_scale);

	bool amIFocused = true;
	bool processFoundOld = false;
	bool lagswitchOld = false;
	int renderfirstframe = 1;

	bool currentBlock = false;

	while (running) {
		// Process all pending messages first
		while (PeekMessage(&msg, NULL, 0U, 0U, PM_REMOVE)) {
			if (msg.message == WM_QUIT) {
				running = false;
				break;
			}
			TranslateMessage(&msg);
			DispatchMessage(&msg);
		}

		if (!running) break;

		// RENDER ONLY IF YOU'RE FOCUSED OR PROCESSFOUND CHANGES
		if (hwnd) { 
			amIFocused = (GetForegroundWindow() == hwnd);
		}

		if ((processFoundOld != processFound) || (lagswitchOld != g_windivert_blocking)) {
			amIFocused = true;
		}

		processFoundOld = processFound;

		lagswitchOld = g_windivert_blocking;

		if (renderfirstframe < 3) {
			amIFocused = true;
		} else {
			renderfirstframe += 1;
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

			// Update frame timing
			lastTime = currentTime;
			nextFrameTime = currentTime + std::chrono::duration<float>(targetFrameTime);

			// Start ImGui frame
			ImGui_ImplOpenGL3_NewFrame();
			ImGui_ImplWin32_NewFrame();
			ImGui::NewFrame();
			// Apply the current theme styles every frame
			ThemeManager::ApplyTheme();

            // ImGui window dimensions
            ImVec2 display_size = ImGui::GetIO().DisplaySize;

            // Set the size of the main ImGui window to fill the screen, fitting to the top left
            ImGui::SetNextWindowSize(display_size, ImGuiCond_Always);
            ImGui::SetNextWindowPos(ImVec2(0, 0), ImGuiCond_Always);

            // Create the main window with the specified flags
            ImGui::Begin("Main SMU Window", nullptr, window_flags); // Main ImGui window

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

			ImGui::InputText("##SettingsTextbox", settingsBuffer, sizeof(settingsBuffer), g_isLinuxWine ? 0 : ImGuiInputTextFlags_CharsNoBlank);

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
			ImGui::TextColored(ImColor(50, 102, 205, 255), "?"); ImGui::SameLine(0, 0);
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
				// Update Wallhop Degrees every change only if sensitivity is not zero/null
				float sensValue = std::atof(RobloxSensValue);
				if (sensValue != 0.0f) {
					float pixels = std::atof(WallhopDegrees) * (camfixtoggle ? 1000.0f : 720.0f) / (360.0f * sensValue);
					snprintf(WallhopPixels, sizeof(WallhopPixels), "%.0f", pixels);
					try {
						wallhop_dx = std::round(std::stoi(WallhopPixels));
						wallhop_dy = -std::round(std::stoi(WallhopPixels));
					} catch (const std::invalid_argument &e) {
					} catch (const std::out_of_range &e) {
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
				try {
					if (wallhopupdate) {
						float factor = camfixtoggle ? 1.388888889f : 1.0f / 1.388888889f;
						if (wallhopswitch) {
							wallhop_dx = std::round(std::stoi(WallhopPixels) * (camfixtoggle ? -factor : factor));
							wallhop_dy = std::round(std::stoi(WallhopPixels) * (camfixtoggle ? factor : -factor));
						} else {
							wallhop_dx = std::round(std::stoi(WallhopPixels) * factor);
							wallhop_dy = std::round(std::stoi(WallhopPixels) * -factor);
							sprintf(WallhopPixels, "%d", wallhop_dx);
						}
					}
				} catch (const std::invalid_argument &) {
				} catch (const std::out_of_range &) {
				}

				float CurrentWallWalkValue = atof(RobloxSensValue); // Wallwalk

				float baseValue = camfixtoggle ? 500.0f : 360.0f;
				wallwalk_strengthx = -static_cast<int>(std::round((baseValue / CurrentWallWalkValue) * 0.13f));
				wallwalk_strengthy = static_cast<int>(std::round((baseValue / CurrentWallWalkValue) * 0.13f));

				sprintf(RobloxWallWalkValueChar, "%d", wallwalk_strengthx);

				float CurrentSensValue = atof(RobloxSensValue); // Speedglitch

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

				ImGui::PushStyleVar(ImGuiStyleVar_WindowRounding, GetCurrentTheme().frame_rounding);

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
				ImGui::PopStyleVar();
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

				if (section_toggles[i]) {
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
					}
				} else {
					if (ImGui::Button("", ImVec2(buttonWidth - 18, buttonHeight))) {
						selected_section = i;
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
						if (potentialLineSize.x > buttonWidth - 7) {
							// Draw the current line and move to the next
							drawList->AddText(textPos, ImGui::ColorConvertFloat4ToU32(GetCurrentTheme().text_primary), currentLine.c_str());
							textPos.y += potentialLineSize.y;
							currentLine = word;
						} else {
							currentLine = potentialLine;
						}
					} else {
						if (potentialLineSize.x > buttonWidth - 18) { // Scrollbar
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
				// Get the binding state for this section's key
				unsigned int* currentKey = section_to_key.at(selected_section);
				BindingState& state = g_bindingStates[currentKey];
    
				// Display section title and keybind UI
				ImGui::TextWrapped("Settings for %s", sections[selected_section].title.c_str());
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
				if (section_to_key.count(selected_section)) {
					*currentKey = BindKeyMode(currentKey, *currentKey, selected_section);
					ImGui::SetNextItemWidth(150.0f);
        
					GetKeyNameFromHex(*currentKey, state.keyBufferHuman, sizeof(state.keyBufferHuman));
				}

				// Use the state's keyBuffer for display
				ImGui::InputText("##KeyBufferHuman", state.keyBufferHuman, sizeof(state.keyBufferHuman), ImGuiInputTextFlags_CharsNoBlank | ImGuiInputTextFlags_ReadOnly);
				ImGui::SameLine();
				ImGui::TextWrapped("Key Binding");
				ImGui::SameLine();
				ImGui::SetNextItemWidth(50.0f);

				ImGui::InputText("##KeyBuffer", state.keyBuffer, sizeof(state.keyBuffer), ImGuiInputTextFlags_CharsNoBlank | ImGuiInputTextFlags_CharsHexadecimal | ImGuiInputTextFlags_ReadOnly);

				ImGui::SameLine();
				ImGui::TextWrapped("Key Binding (Hexadecimal)");
				ImGui::PushStyleColor(ImGuiCol_Text, section_toggles[selected_section] ? GetCurrentTheme().success_color : GetCurrentTheme().error_color);
				ImGui::TextWrapped("Enable This Macro:");
				ImGui::PopStyleColor();
				ImGui::SameLine();
				if (selected_section >= 0 && selected_section < section_amounts) {
					ImGui::Checkbox(("##SectionToggle" + std::to_string(selected_section)).c_str(), 
									&section_toggles[selected_section]);
				}
				ImGui::SameLine(243);
				ImGui::TextWrapped("(Human-Readable)");

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

					ImGui::Checkbox("Allow Roblox to be frozen while not tabbed in", &freezeoutsideroblox);
					ImGui::Checkbox("Switch from Hold Key to Toggle Key", &isfreezeswitch);
					if (isfreezeswitch || takeallprocessids) {
						freezeoutsideroblox = true;
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
										"Also, for convenience sake, you cannot activate desync unless you're tabbed into roblox, You will "
										"most likely crash any other program if you activate it in there.");
				}

				if (selected_section == 2) { // HHJ
					ImGui::Checkbox("Automatically time inputs", &autotoggle);
					ImGui::SameLine();
					ImGui::TextWrapped("(EXTREMELY BUGGY/EXPERIMENTAL, WORKS BEST ON HIGH FPS AND SHALLOW ANGLE TO WALL)");
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

					ImGui::TextWrapped("(ADVANCED) Override current freeze delay (Non Zero) (ms): ");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(52);
					if (ImGui::InputText("##HHJFreezeDelayOverride", HHJFreezeDelayOverrideChar, sizeof(HHJFreezeDelayOverrideChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						try {
							HHJFreezeDelayOverride = std::stoi(HHJFreezeDelayOverrideChar);
						} catch (const std::invalid_argument &e) {
						} catch (const std::out_of_range &e) {
						}
					}

					ImGui::TextWrapped("(ADVANCED) Delay after freezing before shiftlock is held (ms): ");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(52);
					if (ImGui::InputText("##HHJDelay1", HHJDelay1Char, sizeof(HHJDelay1Char), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						try {
							HHJDelay1 = std::stoi(HHJDelay1Char);
						} catch (const std::invalid_argument &e) {
						} catch (const std::out_of_range &e) {
						}
					}

					ImGui::TextWrapped("(ADVANCED) Time shiftlock is held before spinning (ms): ");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(52);
					if (ImGui::InputText("##HHJDelay2", HHJDelay2Char, sizeof(HHJDelay2Char), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						try {
							HHJDelay2 = std::stoi(HHJDelay2Char);
						} catch (const std::invalid_argument &e) {
						} catch (const std::out_of_range &e) {
						}
					}

					ImGui::TextWrapped("(ADVANCED) Time shiftlock is held after freezing (ms): ");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(52);
					if (ImGui::InputText("##HHJDelay3", HHJDelay3Char, sizeof(HHJDelay3Char), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						try {
							HHJDelay3 = std::stoi(HHJDelay3Char);
						} catch (const std::invalid_argument &e) {
						} catch (const std::out_of_range &e) {
						}
					}

					ImVec2 tooltipcursorpos = ImGui::GetCursorScreenPos();
					ImGui::TextColored(ImColor(50, 102, 205, 255), "What is this ("); ImGui::SameLine(0, 0);
					ImGui::Text("?"); ImGui::SameLine(0, 0);
					ImGui::TextColored(ImColor(50, 102, 205, 255), ")");
					ImGui::SetCursorScreenPos(tooltipcursorpos);
					ImGui::InvisibleButton("##tooltip", ImGui::CalcTextSize("What is this (?)"));
					if (ImGui::IsItemHovered(ImGuiHoveredFlags_AllowWhenDisabled))
						ImGui::SetTooltip("These are advanced options configurable for HHJ.\nThey modify the logic of what occurs after the unfreeze.\n"
										  "If you figure out a combination of these first two values that\nconsistently leads to higher HHJ's, please "
										  "tell the Roblox Glitching Community Discord.\n\nThe Defaults of each option are 9, 17, and 16 by default for Windows,\nand "
										  "0, 0, 17 on Linux Respectively.");
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
					ImGui::TextWrapped("Assuming unequip com offset set to /e dance2 is used prior to offset com, to perform a Helicopter High Jump, "
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

					float CurrentSensValue = atof(RobloxSensValue);
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
					ImGui::Checkbox("Make Unequip Com only work while tabbed into Roblox", &unequipinroblox);

					ImGui::Separator();
					ImGui::TextWrapped("This module allows you to trick Roblox into thinking your centre of mass is elsewhere. This is used in the Helicopter "
										"High Jump, Speed Glitch and Walless LHJ modules (may change in the future).");
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
					// Get the binding state for vk_dkey
					BindingState& dkeyState = g_bindingStates[&vk_dkey];
    
					ImGui::TextWrapped("Key to Press:");
					ImGui::SameLine();
    
					if (ImGui::Button((dkeyState.buttonText + "##DKey").c_str())) {
						dkeyState.bindingMode = true;
						dkeyState.notBinding = false;
						dkeyState.buttonText = "Press a Key...";
					}
    
					ImGui::SameLine();
					vk_dkey = BindKeyMode(&vk_dkey, vk_dkey, selected_section);
					ImGui::SetNextItemWidth(150.0f);
					GetKeyNameFromHex(vk_dkey, dkeyState.keyBufferHuman, sizeof(dkeyState.keyBufferHuman));
					ImGui::InputText("Key to Press", dkeyState.keyBufferHuman, sizeof(dkeyState.keyBufferHuman), ImGuiInputTextFlags_CharsNoBlank | ImGuiInputTextFlags_ReadOnly);
					ImGui::SameLine();
					ImGui::SetNextItemWidth(50.0f);
					ImGui::PushID("Press2");
					// Use the state's keyBuffer for hex display
					ImGui::InputText("Key to Press", dkeyState.keyBuffer, sizeof(dkeyState.keyBuffer), ImGuiInputTextFlags_CharsNoBlank | ImGuiInputTextFlags_CharsHexadecimal);
					ImGui::PopID();
					ImGui::NewLine();
					ImGui::SameLine(276);
					ImGui::Text("(Human-Readable)");
					ImGui::SameLine(510);
					ImGui::Text("(Hexadecimal)");

					ImGui::Text("Length of Second Button Press (ms):");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(80);
					if (ImGui::InputText("##PressKeyDelayChar", PressKeyDelayChar, sizeof(PressKeyDelayChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						try {
							PressKeyDelay = std::stoi(PressKeyDelayChar);
						} catch (const std::invalid_argument &e) {
						} catch (const std::out_of_range &e) {
						}
					}

					ImGui::Text("Delay Before Second Press (ms):");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(80);
					if (ImGui::InputText("##PressKeyBonusDelayChar", PressKeyBonusDelayChar, sizeof(PressKeyBonusDelayChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						try {
							PressKeyBonusDelay = std::stoi(PressKeyBonusDelayChar);
						} catch (const std::invalid_argument &e) {
						} catch (const std::out_of_range &e) {
						}
					}


					ImGui::Checkbox("Make PressKey only work while tabbed into Roblox", &presskeyinroblox);

					ImGui::Separator();
					ImGui::TextWrapped("Explanation:");
					ImGui::NewLine();
					ImGui::TextWrapped("It will press the second keybind for a single frame whenever you press the first keybind. "
										"This is most commonly used for micro-adjustments while moving, especially if you do this while jumping.");
				}

				if (selected_section == 6) { // Wallhop
					ImGui::TextWrapped("Flick Degrees (Estimated):");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(70.0f);
					float sensValue = std::atof(RobloxSensValue);
					if (sensValue != 0.0f) {
						snprintf(WallhopDegrees, sizeof(WallhopDegrees), "%d", static_cast<int>(360 * (std::atof(WallhopPixels) * std::atof(RobloxSensValue)) / (camfixtoggle ? 1000 : 720)));
					}
					
					if (ImGui::InputText("##WallhopDegrees", WallhopDegrees, sizeof(WallhopDegrees), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						float pixels = std::atof(WallhopDegrees) * (camfixtoggle ? 1000.0f : 720.0f) / (360.0f * std::atof(RobloxSensValue));
						snprintf(WallhopPixels, sizeof(WallhopPixels), "%.0f", pixels);
						try {
							wallhop_dx = std::round(std::stoi(WallhopPixels));
							wallhop_dy = -std::round(std::stoi(WallhopPixels));
						} catch (const std::invalid_argument &e) {
						} catch (const std::out_of_range &e) {
						}
					}

					ImGui::TextWrapped("Flick Pixel Amount:");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(70.0f);
					if (ImGui::InputText("##WallhopPixels", WallhopPixels, sizeof(WallhopPixels), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						try {
							wallhop_dx = std::round(std::stoi(WallhopPixels));
							wallhop_dy = -std::round(std::stoi(WallhopPixels));
						} catch (const std::invalid_argument &e) {
						} catch (const std::out_of_range &e) {
						}
					}

					ImGui::TextWrapped("Wallhop Length (ms):");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(70.0f);
					if (ImGui::InputText("##WallhopDelay", WallhopDelayChar, sizeof(WallhopDelayChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						try {
							WallhopDelay = std::round(std::stoi(WallhopDelayChar));
						} catch (const std::invalid_argument &e) {
						} catch (const std::out_of_range &e) {
						}
					}

					ImGui::TextWrapped("Bonus Wallhop Delay Before Jumping (ms):");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(70.0f);
					if (ImGui::InputText("##WallhopBonusDelay", WallhopBonusDelayChar, sizeof(WallhopBonusDelayChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						try {
							WallhopBonusDelay = std::round(std::stoi(WallhopBonusDelayChar));
						} catch (const std::invalid_argument &e) {
						} catch (const std::out_of_range &e) {
						}
					}

					ImGui::Checkbox("Switch to Left-Flick Wallhop", &wallhopswitch); // Left Sided wallhop switch
					ImGui::Checkbox("Jump During Wallhop", &toggle_jump);
					ImGui::Checkbox("Flick-Back During Wallhop", &toggle_flick);

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
					ImGui::TextWrapped("Also, for convenience sake, you cannot activate item clip unless you're tabbed into roblox.");
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

					float CurrentWallWalkValue = atof(RobloxSensValue);
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
						RobloxWallWalkValueDelay = atof(RobloxWallWalkValueDelayChar);
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
										"Also, for convenience sake, you cannot activate speedglitch unless you're tabbed into roblox.");
					ImGui::Separator();
					ImGui::TextWrapped("Explanation:");
					ImGui::NewLine();
					ImGui::TextWrapped("This macro abuses the way leg raycast physics work to permanently keep wallhopping, without jumping "
										"you can walk up to a wall, maybe at a bit of an angle, and hold W and D or A to slowly walk across.");
			}

				if (selected_section == 11) { // Spamkey
					BindingState& spamState = g_bindingStates[&vk_spamkey];
    
					ImGui::TextWrapped("Key to Press:");
					ImGui::SameLine();
					if (ImGui::Button((spamState.buttonText + "##").c_str())) {
						spamState.bindingMode = true;
						spamState.notBinding = false;
						spamState.buttonText = "Press a Key...";
					}
					ImGui::SameLine();
					vk_spamkey = BindKeyMode(&vk_spamkey, vk_spamkey, selected_section);
					ImGui::SetNextItemWidth(150.0f);
					GetKeyNameFromHex(vk_spamkey, spamState.keyBufferHuman, sizeof(spamState.keyBufferHuman));
					ImGui::InputText("Key to Press", spamState.keyBufferHuman, sizeof(spamState.keyBufferHuman), ImGuiInputTextFlags_CharsNoBlank | ImGuiInputTextFlags_ReadOnly);
					ImGui::SameLine();
					ImGui::SetNextItemWidth(50.0f);
					ImGui::PushID("Press2");
					ImGui::InputText("Key to Press", spamState.keyBuffer, sizeof(spamState.keyBuffer), ImGuiInputTextFlags_CharsNoBlank | ImGuiInputTextFlags_CharsHexadecimal);
					ImGui::PopID();
					ImGui::NewLine();
					ImGui::SameLine(276);
					ImGui::Text("(Human-Readable)");
					ImGui::SameLine(510);
					ImGui::Text("(Hexadecimal)");
					ImGui::TextWrapped("Spam Delay (Milliseconds):");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(120.0f);
					if (ImGui::InputText("##SpamDelay", SpamDelay, sizeof(SpamDelay), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
						try {
						spam_delay = std::stof(SpamDelay);
						real_delay = static_cast<int>((spam_delay + 0.5f) / 2);
						} catch (const std::invalid_argument &e) {
						} catch (const std::out_of_range &e) {
						}
					}

					ImGui::TextWrapped("I do not take any responsibility if you set the delay to 0ms");
					ImGui::Checkbox("Switch from Toggle Key to Hold Key", &isspamswitch);
					ImGui::Separator();
					ImGui::TextWrapped("Explanation:");
					ImGui::NewLine();
					ImGui::TextWrapped("This macro will spam the second key with a millisecond delay. "
										"This can be used as an autoclicker for any games you want, or a key-spam.");
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
							BunnyHopDelay = atof(BunnyHopDelayChar);
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

					ImGui::TextWrapped("This will not be active unless you are currently inside of the target program.");
				}

				if (selected_section == 14) { // Floor Bounce
					ImGui::Checkbox("Attempt to HHJ for potentially slightly more height (EXPERIMENTAL, NOT RECOMMENDED)", &floorbouncehhj);
					if (ImGui::CollapsingHeader("Advanced Floor Bounce HHJ Options", showadvancedhhjbounce ? ImGuiTreeNodeFlags_DefaultOpen : 0))
					{
						showadvancedhhjbounce = true;

						// Add some vertical spacing for better visual separation
						ImGui::Spacing();
    
						// First checkbox with indentation
						ImGui::Indent();
						ImGui::SetNextItemWidth(50);
						if (ImGui::InputText("Delay (Milliseconds) after unfreezing and before shiftlocking", FloorBounceDelay1Char, sizeof(FloorBounceDelay1Char), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
							try {
								FloorBounceDelay1 = std::stoi(FloorBounceDelay1Char);
							} catch (const std::invalid_argument &e) {
							} catch (const std::out_of_range &e) {
							}
						}
						ImGui::Unindent();
    
						// Second checkbox
						ImGui::Indent();
						ImGui::SetNextItemWidth(50);
						if (ImGui::InputText("Delay (Milliseconds) before enabling helicoptering", FloorBounceDelay2Char, sizeof(FloorBounceDelay2Char), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
							try {
								FloorBounceDelay2 = std::stoi(FloorBounceDelay2Char);
							} catch (const std::invalid_argument &e) {
							} catch (const std::out_of_range &e) {
							}
						}
						ImGui::Unindent();
    
						// Third checkbox
						ImGui::Indent();
						ImGui::SetNextItemWidth(50);
						if (ImGui::InputText("Time (Milliseconds) spent helicoptering", FloorBounceDelay3Char, sizeof(FloorBounceDelay3Char), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
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
					ImGui::TextColored(ImColor(50, 102, 205, 255), "?"); ImGui::SameLine(0, 0);
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
                        ImGui::SetTooltip("Filters only Roblox traffic. Uncheck this AND Only Lag Switch Roblox to block EVERYTHING.");
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
						SafeCloseWinDivert();
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
                                if (TryLoadWinDivert()) {
                                    bWinDivertEnabled = true;
                                    g_windivert_running = true;
                                    WinDivertThread = std::thread(WindivertWorkerThread);
                                } else {
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
                            bWinDivertEnabled = false;
                            g_windivert_running = false;
                            g_windivert_blocking = false; // Reset blocking state
                            SafeCloseWinDivert(); // Kill the thread loop
                            if (WinDivertThread.joinable()) WinDivertThread.join();
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
                    SetWindowPos(hwnd, ontoptoggle ? HWND_TOPMOST : HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
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
                    BYTE alpha = static_cast<BYTE>((windowOpacityPercent / 100.0f) * 255);
                    SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA);
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

            ImGui::EndChild();
            ImGui::PopStyleVar();

            // Finish the main window
            ImGui::End(); // End main ImGui window

            // Render
            ImGui::Render();
            
            glViewport(0, 0, screen_width, screen_height);
            glClearColor(0.45f, 0.55f, 0.60f, 1.00f);
            glClear(GL_COLOR_BUFFER_BIT);
            
            ImGui_ImplOpenGL3_RenderDrawData(ImGui::GetDrawData());
            
            SwapBuffers(g_hDC);
			UpdateLagswitchOverlay();
			// Wait until next frame
			std::this_thread::sleep_until(nextFrameTime);

        }

        // No rendering needed
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
    }

	ImGui_ImplOpenGL3_Shutdown();
    ImGui_ImplWin32_Shutdown();
    ImGui::DestroyContext();
    CleanupDeviceWGL(hwnd);

    UnregisterClass(wc.lpszClassName, wc.hInstance);
    AttachThreadInput(mainThreadId, guiThreadId, FALSE);
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

	// I LOVE THREAD PRIORITY!!!!!!!!!!!!!!!
    SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);

	// Setup suspension

	std::thread actionThread(Speedglitchloop); // Start a separate thread for item desync loop, lets functions run alongside
	std::thread actionThread2(ItemDesyncLoop);
	std::thread actionThread3(SpeedglitchloopHHJ);
	std::thread actionThread4(SpamKeyLoop);
	std::thread actionThread5(ItemClipLoop);
	std::thread actionThread6(WallWalkLoop);
	std::thread actionThread7(BhopLoop);
	std::thread actionThread8(WallhopThread);
	std::thread actionThread9(PressKeyThread);
	std::thread actionThread10(FloorBounceThread);
	std::thread logScannerThread(RobloxLogScannerThread);

	std::thread updaterThread(UpdateSMCThread);
	
	std::thread guiThread(RunGUI);

	std::thread KeyboardThread;

	if (!g_isLinuxWine) {
		KeyboardThread = std::thread(KeyboardHookThread);
	}

	MSG msg;

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
		// Freeze
		if ((macrotoggled && notbinding && section_toggles[0])) {
			bool isMButtonPressed = IsKeyPressed(vk_mbutton);

			if (isfreezeswitch) {  // Toggle mode
				if (isMButtonPressed && !wasMButtonPressed && (freezeoutsideroblox || tabbedintoroblox)) {  // Detect button press edge
					isSuspended = !isSuspended;  // Toggle the freeze state
					SuspendOrResumeProcesses_Compat(targetPIDs, hProcess, isSuspended);

					if (isSuspended) {
						suspendStartTime = std::chrono::steady_clock::now();  // Start the timer
					}
				}
			} else {  // Hold mode
				if (isMButtonPressed && (freezeoutsideroblox || tabbedintoroblox)) {
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
		if (IsKeyPressed(vk_f5) && tabbedintoroblox && macrotoggled && notbinding && section_toggles[1]) {
			if (!isdesync) {
				isdesyncloop.store(true, std::memory_order_relaxed);
				isdesync = true;
			}
		} else {
			isdesync = false;
			isdesyncloop.store(false, std::memory_order_relaxed);
		}

		// PressKey
		if (IsKeyPressed(vk_zkey) && macrotoggled && notbinding && section_toggles[5] && (!presskeyinroblox || tabbedintoroblox)) {
			if (!ispresskey) {
				ispresskeythread.store(true, std::memory_order_relaxed);
				ispresskey = true;
			}
		} else {
			ispresskey = false;
			ispresskeythread.store(false, std::memory_order_relaxed);
		}

		// Wallhop (Ran in separate thread)
		if (IsKeyPressed(vk_xbutton2) && macrotoggled && notbinding && section_toggles[6]) {
			if (!iswallhop) {
				iswallhopthread.store(true, std::memory_order_relaxed);
				iswallhop = true;
				}
		} else {
			iswallhopthread.store(false, std::memory_order_relaxed);
			iswallhop = false;
		}

		// Walless LHJ (REQUIRES COM OFFSET AND .5 STUDS OF A FOOT ON A PLATFORM)
		if (IsKeyPressed(vk_f6) && macrotoggled && notbinding && section_toggles[7]) {
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
		if (IsKeyPressed(vk_xkey) && tabbedintoroblox && macrotoggled && notbinding && section_toggles[3]) {
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
		if (IsKeyPressed(vk_f8) && macrotoggled && notbinding && section_toggles[4] && (!unequipinroblox || tabbedintoroblox)) {
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
		if (IsKeyPressed(vk_xbutton1) && macrotoggled && notbinding && section_toggles[2]) {
			if (!HHJ) {
				if (autotoggle) { // Auto-Key-Timer
					HoldKey(0x39);
					std::this_thread::sleep_for(std::chrono::milliseconds(550));
					HoldKey(0x11);
					std::this_thread::sleep_for(std::chrono::milliseconds(68));
				}

				SuspendOrResumeProcesses_Compat(targetPIDs, hProcess, true);

				// If the users custom freeze duration is zero or nothing, do regular stuff, else, use their custom duration
				if (strcmp(HHJFreezeDelayOverrideChar, "0") == 0 || strcmp(HHJFreezeDelayOverrideChar, "") == 0) {
					// FastHHJ lowers freeze time by 300 ms
					if (!fasthhj) {
						std::this_thread::sleep_for(std::chrono::milliseconds(300));
					}
					std::this_thread::sleep_for(std::chrono::milliseconds(200));
				} else {
					std::this_thread::sleep_for(std::chrono::milliseconds(HHJFreezeDelayOverride));
				}

				if (autotoggle) { // Auto-Key-Timer
					ReleaseKey(0x39);
					ReleaseKey(0x11);
				}

				SuspendOrResumeProcesses_Compat(targetPIDs, hProcess, false);

				std::this_thread::sleep_for(std::chrono::milliseconds(HHJDelay1));
				if (!globalzoomin) {
					HoldKeyBinded(vk_shiftkey);
				} else {
					HoldKeyBinded(globalzoominreverse ? VK_MOUSE_WHEEL_DOWN : VK_MOUSE_WHEEL_UP);
				}
				std::this_thread::sleep_for(std::chrono::milliseconds(HHJDelay2));
				isHHJ.store(true, std::memory_order_relaxed);
				std::this_thread::sleep_for(std::chrono::milliseconds(HHJDelay3));
				if (!globalzoomin) {
					ReleaseKeyBinded(vk_shiftkey);
				}
				std::this_thread::sleep_for(std::chrono::milliseconds(HHJLength));
				isHHJ.store(false, std::memory_order_relaxed);
				HHJ = true;
			}
		} else {
			HHJ = false;
		}

		// Spamkey
		if (IsKeyPressed(vk_leftbracket) && macrotoggled && notbinding && section_toggles[11]) {
			if (!isspam) {
				isspamloop = !isspamloop;
				isspam = true;
			}
		} else {
			isspam = false;
			if (isspamswitch) {
				isspamloop = false;
			}
		}

		// Laughkey
		if (IsKeyPressed(vk_laughkey) && tabbedintoroblox && macrotoggled && notbinding && section_toggles[9]) {
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
		if (IsKeyPressed(vk_bouncekey) && tabbedintoroblox && macrotoggled && notbinding && section_toggles[12]) {
			if (!isbounce) {
				int turn90 = (camfixtoggle ? 250 : 180) / atof(RobloxSensValue);
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
		if (IsKeyPressed(vk_clipkey) && tabbedintoroblox && macrotoggled && notbinding && section_toggles[8]) {
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
		if (IsKeyPressed(vk_wallkey) && tabbedintoroblox && macrotoggled && notbinding && section_toggles[10]) {
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

		bool can_process_bhop = tabbedintoroblox && section_toggles[13] && macrotoggled && notbinding;
		if (IsKeyPressed(vk_chatkey)) {
			bhoplocked = true;
		}
		
		if (bhoplocked) {
			if (IsKeyPressed(VK_RETURN) || IsKeyPressed(VK_LBUTTON)) {
				bhoplocked = false;
			}
		}
		
		if (can_process_bhop) {
			bool is_bunnyhop_key_considered_held = g_isVk_BunnyhopHeldDown.load(std::memory_order_relaxed) || IsKeyPressed(vk_bunnyhopkey);

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
		if (IsKeyPressed(vk_floorbouncekey) && tabbedintoroblox && macrotoggled && notbinding && section_toggles[14]) {
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
				bool isPressed = IsKeyPressed(vk_lagswitchkey);
                
                // Determine the "Logical" state (Does the user want it on?)
                bool logical_on_state = false;

				if (islagswitchswitch) {
					// TOGGLE MODE
					if (isPressed && !wasLagSwitchPressed) {
						g_windivert_blocking = !g_windivert_blocking; // Flip state
					}
                    logical_on_state = g_windivert_blocking;
					wasLagSwitchPressed = isPressed;
				} else {
					// HOLD MODE
					g_windivert_blocking = isPressed;
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
                        g_windivert_blocking = false;
                        
                        // 2. Wait
                        std::this_thread::sleep_for(std::chrono::milliseconds(lagswitch_unblock_ms));
                        
                        // 3. Determine if we should Re-Lag
                        bool should_resume = false;

                        if (islagswitchswitch) {
                            // In Toggle Mode, if we were logically on, we STAY on.
                            should_resume = true;
                        } else {
                            // In Hold Mode, we only resume if the key is STILL being held down.
                            if (IsKeyPressed(vk_lagswitchkey)) {
                                should_resume = true;
                            }
                        }

                        if (should_resume) {
                            g_windivert_blocking = true;
                        }
                        
                        // 4. Reset timer
                        lagswitch_start_time = std::chrono::steady_clock::now();
                    }
                }

			} else {
                // If user presses key but driver isn't loaded, prompt for admin
				if (IsKeyPressed(vk_lagswitchkey)) {
					if (IsRunAsAdmin()) {
						if (TryLoadWinDivert()) {
							bWinDivertEnabled = true;
							g_windivert_running = true;
							WinDivertThread = std::thread(WindivertWorkerThread);
							// Allow 200ms for the driver to load before toggling on
							std::this_thread::sleep_for(std::chrono::milliseconds(200));
							g_windivert_blocking = true;
						} else {
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
            // Safety: If module is disabled or tabbed out, ensure we aren't blocking internet
            if (!section_toggles[15]) {
                g_windivert_blocking = false;
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
		isbhoploop = false;
		iswallwalkloop = false;
		isitemloop = false;
		isdesyncloop = false;
		isspeed = false;
	}

	std::this_thread::sleep_for(std::chrono::microseconds(50)); // Delay between main code loop (so your cpu doesn't die instantly)

}

	// Save Window Positions and Size before closing
	RECT windowrect;
	GetWindowRect(hwnd, &windowrect);

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

	// If save file, save normally, if not, save as Profile 1
	if (!G_CURRENTLY_LOADED_PROFILE_NAME.empty()) {
		SaveSettings(G_SETTINGS_FILEPATH, G_CURRENTLY_LOADED_PROFILE_NAME);
	} else {
		std::ifstream file(G_SETTINGS_FILEPATH);
		if (!file.is_open()) {
			G_CURRENTLY_LOADED_PROFILE_NAME = "Profile 1";
			SaveSettings(G_SETTINGS_FILEPATH, G_CURRENTLY_LOADED_PROFILE_NAME);
		}
	}

	if (g_keyboardHook) {
		UnhookWindowsHookEx(g_keyboardHook);
		g_keyboardHook = NULL;
	}

	CleanupOverlay();

	running = false;
	g_windivert_running = false;

	StopKeyboardThread();

	actionThread.join();
	actionThread2.join();
	actionThread3.join();
	actionThread4.join();
	actionThread5.join();
	actionThread6.join();
	actionThread7.join();
	actionThread8.join();
	actionThread9.join();
	actionThread10.join();

	SafeCloseWinDivert();

	g_log_thread_running = false;
	logScannerThread.join();

	guiThread.join();

	if (g_overlayFontHandle) {
		RemoveFontMemResourceEx(g_overlayFontHandle);
	}

	if (IsWindow(hwnd)) {
		DestroyWindow(hwnd);
	}
	
	if (WinDivertThread.joinable()) WinDivertThread.join();

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
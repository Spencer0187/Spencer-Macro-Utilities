#include <iostream>
#include <vector>
#include <windows.h>
#include <Psapi.h>
#include <fcntl.h>
#include <io.h>
#include <tchar.h>
#include <thread>
#include <chrono>
#include <cwctype>
#include <string>
#include <atomic>
#include <algorithm>  
#include <tlhelp32.h>
#include <d3d11.h>
#include "imgui-files/imgui.h"
#include "imgui-files/imgui_impl_dx11.h"
#include "imgui-files/imgui_impl_win32.h"
#include "imgui-files/json.hpp"
#include "imgui-files/LSANS.h"
#include <wininet.h>
#include "resource.h"
#include <condition_variable>
#include <fstream>
#include <filesystem>
#include <locale>
#include <codecvt>
#include <format>
#include <unordered_map>
#include <math.h>

#pragma comment(lib, "wininet.lib")

using json = nlohmann::json;

extern IMGUI_IMPL_API LRESULT ImGui_ImplWin32_WndProcHandler(HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam);
#define TH32CS_PROCESS 0x00000002


// DirectX11 Variables
ID3D11Device *g_pd3dDevice = NULL;
ID3D11DeviceContext *g_pd3dDeviceContext = NULL;
IDXGISwapChain *g_pSwapChain = NULL;
ID3D11RenderTargetView *g_mainRenderTargetView = NULL;

// Forward Declarations
bool CreateDeviceD3D(HWND hWnd);
void CleanupDeviceD3D();
void CreateRenderTarget();
void CleanupRenderTarget();
LRESULT WINAPI WndProc(HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam);


// TO ENABLE FREEZING, CREATE A .CMD in the same folder that just does (nameofthisexe).exe RobloxPlayerBeta.exe (or whatever your roblox process is called)


// TO PUT IN A KEYBOARD KEY, GO TO https://www.millisecond.com/support/docs/current/html/language/scancodes.htm
// Convert the scancode into hexadecimal before putting it into the HoldKey or ReleaseKey functions
// Ex: E = 18 = 0x12 = HoldKey(0x12)

// If you want to create custom HOTKEYS for stuff that isn't an alphabet/function key, go to https://learn.microsoft.com/en-us/windows/win32/inputdev/virtual-key-codes and get your virtual key code or value

std::string text = "/e dance2"; // INSERT CUSTOM TEXT HERE

std::atomic<bool> isdesyncloop(false); // Set the variable used for the alternate thread
std::atomic<bool> isspeed(false);
std::atomic<bool> isHHJ(false);
std::atomic<bool> isspamloop(false);
std::atomic<bool> isitemloop(false);
std::atomic<bool> iswallwalkloop(false);

std::mutex renderMutex;
std::condition_variable renderCondVar;
bool renderFlag = false;
bool running = true;
HWND hwnd;

const DWORD SCAN_CODE_FLAGS = KEYEVENTF_SCANCODE;
const DWORD RELEASE_FLAGS = KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP;

INPUT inputkey = {};
INPUT inputhold = {};
INPUT inputrelease = {};

// Translate all VK's into variables so it's less annoying to debug it
unsigned int scancode_shift = 0x2A;
unsigned int vk_f5 = VK_F5;
unsigned int vk_f6 = VK_F6;
unsigned int vk_f8 = VK_F8;
unsigned int vk_mbutton = VK_MBUTTON;
unsigned int vk_xbutton1 = VK_XBUTTON1;
unsigned int vk_xbutton2 = VK_XBUTTON2;
unsigned int vk_spamkey = VK_LBUTTON;
unsigned int vk_clipkey = VK_F3;
unsigned int vk_wallkey = VK_F1;
unsigned int vk_zkey = VkKeyScanEx('Z', GetKeyboardLayout(0)) & 0xFF; // Use this for alphabet keys so it works across layouts
unsigned int vk_dkey = VkKeyScanEx('D', GetKeyboardLayout(0)) & 0xFF;
unsigned int vk_xkey = VkKeyScanEx('X', GetKeyboardLayout(0)) & 0xFF;
unsigned int vk_wkey = VkKeyScanEx('W', GetKeyboardLayout(0)) & 0xFF;
unsigned int vk_leftbracket = MapVirtualKey(0x1A, MAPVK_VSC_TO_VK);
unsigned int sc_slashkey = 0x35;

std::unordered_map<int, std::string> vkToString = {
    {VK_LBUTTON, "VK_LBUTTON"},
    {VK_RBUTTON, "VK_RBUTTON"},
    {VK_CANCEL, "VK_CANCEL"},
    {VK_MBUTTON, "VK_MBUTTON"},
    {VK_XBUTTON1, "VK_XBUTTON1"},
    {VK_XBUTTON2, "VK_XBUTTON2"},
    {VK_BACK, "VK_BACK"},
    {VK_TAB, "VK_TAB"},
    {VK_CLEAR, "VK_CLEAR"},
    {VK_RETURN, "VK_RETURN"},
    {VK_SHIFT, "VK_SHIFT"},
    {VK_CONTROL, "VK_CONTROL"},
    {VK_MENU, "VK_MENU"},
    {VK_PAUSE, "VK_PAUSE"},
    {VK_CAPITAL, "VK_CAPITAL"},
    {VK_ESCAPE, "VK_ESCAPE"},
    {VK_SPACE, "VK_SPACE"},
    {VK_PRIOR, "VK_PRIOR"},
    {VK_NEXT, "VK_NEXT"},
    {VK_END, "VK_END"},
    {VK_HOME, "VK_HOME"},
    {VK_LEFT, "VK_LEFT"},
    {VK_UP, "VK_UP"},
    {VK_RIGHT, "VK_RIGHT"},
    {VK_DOWN, "VK_DOWN"},
    {VK_SELECT, "VK_SELECT"},
    {VK_PRINT, "VK_PRINT"},
    {VK_EXECUTE, "VK_EXECUTE"},
    {VK_SNAPSHOT, "VK_SNAPSHOT"},
    {VK_INSERT, "VK_INSERT"},
    {VK_DELETE, "VK_DELETE"},
    {VK_HELP, "VK_HELP"},
    {VK_LWIN, "VK_LWIN"},
    {VK_RWIN, "VK_RWIN"},
    {VK_NUMPAD0, "VK_NUMPAD0"},
    {VK_NUMPAD1, "VK_NUMPAD1"},
    {VK_NUMPAD2, "VK_NUMPAD2"},
    {VK_NUMPAD3, "VK_NUMPAD3"},
    {VK_NUMPAD4, "VK_NUMPAD4"},
    {VK_NUMPAD5, "VK_NUMPAD5"},
    {VK_NUMPAD6, "VK_NUMPAD6"},
    {VK_NUMPAD7, "VK_NUMPAD7"},
    {VK_NUMPAD8, "VK_NUMPAD8"},
    {VK_NUMPAD9, "VK_NUMPAD9"},
    {VK_MULTIPLY, "VK_MULTIPLY"},
    {VK_ADD, "VK_ADD"},
    {VK_SEPARATOR, "VK_SEPARATOR"},
    {VK_SUBTRACT, "VK_SUBTRACT"},
    {VK_DECIMAL, "VK_DECIMAL"},
    {VK_DIVIDE, "VK_DIVIDE"},
    {VK_F1, "VK_F1"},
    {VK_F2, "VK_F2"},
    {VK_F3, "VK_F3"},
    {VK_F4, "VK_F4"},
    {VK_F5, "VK_F5"},
    {VK_F6, "VK_F6"},
    {VK_F7, "VK_F7"},
    {VK_F8, "VK_F8"},
    {VK_F9, "VK_F9"},
    {VK_F10, "VK_F10"},
    {VK_F11, "VK_F11"},
    {VK_F12, "VK_F12"},
    {VK_LSHIFT, "VK_LSHIFT"},
    {VK_RSHIFT, "VK_RSHIFT"},
    {VK_LCONTROL, "VK_LCONTROL"},
    {VK_RCONTROL, "VK_RCONTROL"},
    {VK_LMENU, "VK_LMENU"},
    {VK_RMENU, "VK_RMENU"},
    {VK_OEM_PLUS, "VK_OEM_PLUS"},
    {VK_OEM_COMMA, "VK_OEM_COMMA"},
    {VK_OEM_MINUS, "VK_OEM_MINUS"},
    {VK_OEM_PERIOD, "VK_OEM_PERIOD"},
    {VK_OEM_2, "VK_OEM_2"},
    {VK_OEM_3, "VK_OEM_3"},
    {VK_OEM_4, "VK_OEM_4"},
    {VK_OEM_5, "VK_OEM_5"},
    {VK_OEM_6, "VK_OEM_6"},
    {VK_OEM_7, "VK_OEM_7"},
    {VK_OEM_8, "VK_OEM_8"},
    {VK_OEM_102, "VK_OEM_102"}
};

int wallhop_dx = 300;
int wallhop_dy = -300;
int speed_strengthx = 959;
int wallwalk_strengthx = 94;
int speedoffsetx = 0;
int speed_strengthy = -959;
int wallwalk_strengthy = -94;
int speedoffsety = 0;
int speed_slot = 3;
int desync_slot = 5;
int clip_slot = 7;
int clip_delay = 30;
int RobloxWallWalkValueDelay = 72720;
float spam_delay = 20;
float maxfreezetime = 9.00f;
int real_delay = 10000;
int selected_dropdown = 0;
char settingsBuffer[256] = "RobloxPlayerBeta.exe"; // Default value for the textbox
char KeyBufferhuman[256] = "None";
char KeyBuffer[256] = "None";
char KeyBufferhumanalt[256] = "None";
char KeyBufferalt[256] = "None";
char ItemDesyncSlot[256] = "5";
char ItemSpeedSlot[256] = "3";
char ItemClipSlot[256] = "7";
char ItemClipDelay[256] = "30";
char WallhopPixels[256] = "300";
char SpamDelay[256] = "20";
int section_amounts = 11;

static float PreviousSensValue = 0.5f;
static float PreviousWallWalkSide = 0;
static float PreviousWallWalkValue = 0.5f;
char RobloxSensValue[256] = "0.5";
char RobloxPixelValueChar[256] = "718";
char RobloxWallWalkValueChar[256] = "-94";
char RobloxWallWalkValueDelayChar[256] = "72720";
const char *optionsforoffset[] = {"/e dance2", "/e laugh", "/e cheer"};
int RobloxPixelValue = 718;
int RobloxWallWalkValue = -94;
std::string KeyButtonText = "Click to Bind Key";
std::string KeyButtonTextalt = "Click to Bind Key";
auto rebindtime = std::chrono::high_resolution_clock::now();

static int selected_section = -1;


int screen_width = GetSystemMetrics(SM_CXSCREEN)/1.5;
int screen_height = GetSystemMetrics(SM_CYSCREEN)/1.5 + 10;

auto suspendStartTime = std::chrono::steady_clock::time_point();
bool section_toggles[11] = {true, true, true, true, true, false, true, true, true, false, false};
const int unsuspendTime = 50;
bool unequiptoggle = false;
bool shiftswitch = false;
bool processFound = false; // Initialize as no process found
bool done = false;
bool macrotoggled = true;
bool bindingMode = false;
bool bindingModealt = false;
bool notbinding = true;
bool camfixtoggle = false;
bool wallhopswitch = false;
bool wallwalktoggleside = false;
bool wallhopcamfix = false;
bool toggle_jump = true;
bool toggle_flick = true;
bool autotoggle = false;
bool isspeedswitch = false;
bool isfreezeswitch = false;
bool iswallwalkswitch = false;
bool isspamswitch = false;
bool isitemclipswitch = false;
static bool wasMButtonPressed = false; 
static bool UserAcknowledgedV250 = false;


typedef LONG(NTAPI *NtSuspendProcess)(HANDLE ProcessHandle);
typedef LONG(NTAPI *NtResumeProcess)(HANDLE ProcessHandle);

// Function to suspend or resume a process
void SuspendOrResumeProcess(NtSuspendProcess pfnSuspend, NtResumeProcess pfnResume, HANDLE hProcess,
			    bool suspend)
{
	if (suspend) {
		pfnSuspend(hProcess);
	} else {
		pfnResume(hProcess);
	}
}


INPUT createInput()
{
	INPUT inputkey = {};
	inputkey.type = INPUT_KEYBOARD;
	return inputkey;
}

// Hold a key down, self explanatory
void HoldKey(WORD scanCode)
{
	INPUT input = {};
	input.type = INPUT_KEYBOARD;
	input.ki.wScan = scanCode;
	input.ki.dwFlags = SCAN_CODE_FLAGS;

	SendInput(1, &input, sizeof(INPUT));
}

void ReleaseKey(WORD scanCode)
{
	INPUT input = {};
	input.type = INPUT_KEYBOARD;
	input.ki.wScan = scanCode;
	input.ki.dwFlags = RELEASE_FLAGS;

	SendInput(1, &input, sizeof(INPUT));
}

void HoldKeyMouse(WORD scanCode)
{
    INPUT input = {};

    // Check if scanCode corresponds to M1, M2, or M3
    if (scanCode == VK_LBUTTON || scanCode == VK_RBUTTON || scanCode == VK_MBUTTON)
    {
        input.type = INPUT_MOUSE;
        switch (scanCode)
        {
        case VK_LBUTTON:
            input.mi.dwFlags = MOUSEEVENTF_LEFTDOWN;
            break;
        case VK_RBUTTON:
            input.mi.dwFlags = MOUSEEVENTF_RIGHTDOWN;
            break;
        case VK_MBUTTON:
            input.mi.dwFlags = MOUSEEVENTF_MIDDLEDOWN;
            break;
        }
    }
    else
    {
		scanCode = MapVirtualKeyEx(scanCode, MAPVK_VK_TO_VSC, GetKeyboardLayout(0));
        input.type = INPUT_KEYBOARD;
        input.ki.wScan = scanCode;
        input.ki.dwFlags = SCAN_CODE_FLAGS;
    }

    SendInput(1, &input, sizeof(INPUT));
}

void ReleaseKeyMouse(WORD scanCode)
{
    INPUT input = {};

    // Check if scanCode corresponds to M1, M2, or M3
    if (scanCode == VK_LBUTTON || scanCode == VK_RBUTTON || scanCode == VK_MBUTTON)
    {
        input.type = INPUT_MOUSE;
        switch (scanCode)
        {
        case VK_LBUTTON:
            input.mi.dwFlags = MOUSEEVENTF_LEFTUP;
            break;
        case VK_RBUTTON:
            input.mi.dwFlags = MOUSEEVENTF_RIGHTUP;
            break;
        case VK_MBUTTON:
            input.mi.dwFlags = MOUSEEVENTF_MIDDLEUP;
            break;
        }
    }
    else
    {
		scanCode = MapVirtualKeyEx(scanCode, MAPVK_VK_TO_VSC, GetKeyboardLayout(0));
        input.type = INPUT_KEYBOARD;
        input.ki.wScan = scanCode;
        input.ki.dwFlags = RELEASE_FLAGS;
    }

    SendInput(1, &input, sizeof(INPUT));
}


// Move your mouse to any coordinate
void MoveMouse(int dx, int dy)
{
	INPUT input = {0};
	input.type = INPUT_MOUSE;
	input.mi.dx = dx;
	input.mi.dy = dy;
	input.mi.dwFlags = MOUSEEVENTF_MOVE;

	SendInput(1, &input, sizeof(INPUT));
}

void PasteText(const std::string &text) // To run, just do PasteText(text), which is the name of the variable above at the beginning
{
	for (char c : text) {
        // Key down event
        INPUT input = { 0 };
        input.type = INPUT_KEYBOARD;
        input.ki.wVk = 0;
        input.ki.wScan = c;
        input.ki.dwFlags = KEYEVENTF_UNICODE;  // Unicode key down
        SendInput(1, &input, sizeof(INPUT));

        // Key up event
        input.ki.dwFlags = KEYEVENTF_UNICODE | KEYEVENTF_KEYUP;  // Unicode key up
        SendInput(1, &input, sizeof(INPUT));
    }
}

// This is ran in a separate thread to avoid interfering with other functions
void ItemDesyncLoop()
{
	while (true) { // Efficient variable checking method
		while (!isdesyncloop) {
			std::this_thread::sleep_for(std::chrono::microseconds(100));
		}
		if (macrotoggled && notbinding && section_toggles[1]) {
			HoldKey(desync_slot + 1);
			ReleaseKey(desync_slot + 1);
			HoldKey(desync_slot + 1);
			ReleaseKey(desync_slot + 1);
		}
	}
}

void Speedglitchloop()
{
	while (true) {
		while (!isspeed) {
			std::this_thread::sleep_for(std::chrono::microseconds(50));
		}
		if (macrotoggled && notbinding && section_toggles[3]) {
			MoveMouse(speed_strengthx, 0);
			std::this_thread::sleep_for(std::chrono::microseconds(6060));
			MoveMouse(speed_strengthy, 0);
			std::this_thread::sleep_for(std::chrono::microseconds(6060));
		}
	}
}

void SpeedglitchloopHHJ()
{
	while (true) {
		while (!isHHJ) {
			std::this_thread::sleep_for(std::chrono::microseconds(25));
		}
		if (macrotoggled && notbinding && section_toggles[2]) {
			MoveMouse(speed_strengthx, 0);
			std::this_thread::sleep_for(std::chrono::microseconds(3030));
			MoveMouse(speed_strengthy, 0);
			std::this_thread::sleep_for(std::chrono::microseconds(3030));
		}
	}
}

void SpamKeyLoop()
{
	while (true) { // Efficient variable checking method
		while (!isspamloop) {
			std::this_thread::sleep_for(std::chrono::microseconds(100));
		}
		if (macrotoggled && notbinding && section_toggles[10]) {
			HoldKeyMouse(vk_spamkey);
			std::this_thread::sleep_for(std::chrono::microseconds(real_delay));
			ReleaseKeyMouse(vk_spamkey);
			std::this_thread::sleep_for(std::chrono::microseconds(real_delay));
		}
	}
}

void ItemClipLoop()
{
	while (true) { // Efficient variable checking method
		while (!isitemloop) {
			std::this_thread::sleep_for(std::chrono::microseconds(100));
		}
		if (macrotoggled && notbinding && section_toggles[8]) {
			HoldKey(clip_slot + 1);
			std::this_thread::sleep_for(std::chrono::milliseconds(clip_delay / 2));
			ReleaseKey(clip_slot + 1);
			std::this_thread::sleep_for(std::chrono::milliseconds(clip_delay / 2));
		}
	}
}

void WallWalkLoop()
{
	while (true) { // Efficient variable checking method
		while (!iswallwalkloop) {
			std::this_thread::sleep_for(std::chrono::microseconds(100));
		}
		if (macrotoggled && notbinding && section_toggles[9]) {
			if (wallwalktoggleside) {
				MoveMouse(-wallwalk_strengthx, 0);
				std::this_thread::sleep_for(std::chrono::microseconds(6060));
				MoveMouse(-wallwalk_strengthy, 0);
				std::this_thread::sleep_for(std::chrono::microseconds(RobloxWallWalkValueDelay));
			} else {
				MoveMouse(wallwalk_strengthx, 0);
				std::this_thread::sleep_for(std::chrono::microseconds(6060));
				MoveMouse(wallwalk_strengthy, 0);
				std::this_thread::sleep_for(std::chrono::microseconds(RobloxWallWalkValueDelay));
			}
		}
	}
}

bool IsMainWindow(HWND handle)
{
	return GetWindow(handle, GW_OWNER) == (HWND)0 && IsWindowVisible(handle);
}

HWND FindWindowByProcessHandle(HANDLE hProcess)
{
	DWORD targetPID = GetProcessId(hProcess);
	HWND rbxhwnd = FindWindowEx(NULL, NULL, NULL, NULL);
	while (rbxhwnd != NULL) {
		DWORD windowPID = 0;
		GetWindowThreadProcessId(rbxhwnd, &windowPID);
		if (windowPID == targetPID && IsMainWindow(rbxhwnd)) {
			processFound = true;
			return rbxhwnd; // Return if it's the main window
		}
		rbxhwnd = FindWindowEx(NULL, rbxhwnd, NULL, NULL);
	}
	processFound = false;
	return NULL;
}


DWORD GetProcessIdByName() // Return hProcess from .exe name
{
	std::string RobloxName = settingsBuffer;
	std::wstring_convert<std::codecvt_utf8<wchar_t>> converter;
	std::wstring wideString = converter.from_bytes(settingsBuffer);
	const wchar_t *RobloxNameWCHAR = wideString.c_str();
	HANDLE hSnapshot = CreateToolhelp32Snapshot(TH32CS_PROCESS, 0);
	PROCESSENTRY32 pe = {sizeof(pe)};

	while (Process32Next(hSnapshot, &pe)) {
		if (_wcsicmp(pe.szExeFile, RobloxNameWCHAR) == 0) {
			processFound = true;
			CloseHandle(hSnapshot);
			return pe.th32ProcessID;
		}
	}
	CloseHandle(hSnapshot);
	processFound = false;
	return 0; // Not found
}

bool IsForegroundWindowProcess(HWND targetHwnd) // Check if the current active window is the given HWND
{
    HWND hwndtest = GetForegroundWindow();
    return (hwndtest == targetHwnd);
}

std::string Trim(const std::string& str) {
    auto start = str.begin();
    while (start != str.end() && std::isspace(*start)) {
        ++start;
    }

    auto end = str.end();
    do {
        --end;
    } while (end != start && std::isspace(*end));

    return std::string(start, end + 1);
}

size_t OutputReleaseVersion(void* contents, size_t size, size_t nmemb, std::string* output) {
    size_t totalSize = size * nmemb;
    output->append((char*)contents, totalSize);
    return totalSize;
}

std::string GetRemoteVersion() {
    HINTERNET hInternet = InternetOpen(L"User-Agent", INTERNET_OPEN_TYPE_DIRECT, NULL, NULL, 0);
    if (!hInternet) return ""; // Failed to initialize

    HINTERNET hConnect = InternetOpenUrl(hInternet, 
        L"https://raw.githubusercontent.com/Spencer0187/Roblox-Macro-Utilities/main/version", 
        NULL, 0, INTERNET_FLAG_RELOAD, 0);
    if (!hConnect) {
        InternetCloseHandle(hInternet);
        return ""; // Failed to connect
    }

    // Read the response
    char buffer[4096];
    DWORD bytesRead;
    std::string response;

    while (InternetReadFile(hConnect, buffer, sizeof(buffer), &bytesRead) && bytesRead != 0) {
        response.append(buffer, bytesRead);
    }

    // Clean up
    InternetCloseHandle(hConnect);
    InternetCloseHandle(hInternet);

    return response;
}

LRESULT CALLBACK WndProc(HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam)
{
    if (ImGui_ImplWin32_WndProcHandler(hWnd, msg, wParam, lParam))
		return true; // Forward ImGUI-related messages

    switch (msg) {
    case WM_SIZE:
		screen_width = LOWORD(lParam);;
		screen_height = HIWORD(lParam);
		if (g_pd3dDevice != NULL && wParam != SIZE_MINIMIZED) {
			CleanupRenderTarget();
			g_pSwapChain->ResizeBuffers(0, (UINT)LOWORD(lParam), (UINT)HIWORD(lParam),
						    DXGI_FORMAT_UNKNOWN, 0);
			CreateRenderTarget();
		}
		return 0;

    case WM_GETMINMAXINFO: {
        MINMAXINFO* mmi = reinterpret_cast<MINMAXINFO*>(lParam);

        // Set the minimum tracking size
        mmi->ptMinTrackSize.x = 1147;
        mmi->ptMinTrackSize.y = 780;
        return 0;
    }

    case WM_SYSCOMMAND:
		if ((wParam & 0xfff0) == SC_KEYMENU) // Disable ALT application menu
			return 0;
		break;
    case WM_DESTROY:
		PostQuitMessage(0);
		return 0;
    case WM_CLOSE:
		done = true;
		PostQuitMessage(0);
		return 0;
    }

    return DefWindowProc(hWnd, msg, wParam, lParam);
}

bool CreateDeviceD3D(HWND hWnd)
{
    RECT rect;
    GetClientRect(hWnd, &rect);
    int window_width = rect.right - rect.left;
    int window_height = rect.bottom - rect.top;
    DXGI_SWAP_CHAIN_DESC sd;
    ZeroMemory(&sd, sizeof(sd));
    sd.BufferCount = 2; // Use double buffering
    sd.BufferDesc.Width = window_width;
    sd.BufferDesc.Height = window_height;
    sd.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    sd.BufferDesc.RefreshRate.Numerator = 120; // Target 120 Hz
    sd.BufferDesc.RefreshRate.Denominator = 1;
    sd.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    sd.OutputWindow = hWnd;
    sd.SampleDesc.Count = 1;
    sd.SampleDesc.Quality = 0;
    sd.Windowed = TRUE;
    sd.SwapEffect = DXGI_SWAP_EFFECT_DISCARD;
    sd.Flags = 0;
    sd.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    sd.OutputWindow = hWnd;
    sd.SampleDesc.Count = 1;
    sd.SampleDesc.Quality = 0;
    sd.Windowed = TRUE;
    sd.SwapEffect = DXGI_SWAP_EFFECT_DISCARD;

    HRESULT res = D3D11CreateDeviceAndSwapChain(NULL, D3D_DRIVER_TYPE_HARDWARE, NULL, 0, NULL, 0, D3D11_SDK_VERSION, &sd, &g_pSwapChain, &g_pd3dDevice, NULL, &g_pd3dDeviceContext);

    if (FAILED(res))
		return false;

    CreateRenderTarget();
    return true;
}

void CleanupDeviceD3D()
{
    CleanupRenderTarget();
    if (g_pSwapChain) {
		g_pSwapChain->Release();
		g_pSwapChain = NULL;
    }
    if (g_pd3dDeviceContext) {
		g_pd3dDeviceContext->Release();
		g_pd3dDeviceContext = NULL;
    }
    if (g_pd3dDevice) {
		g_pd3dDevice->Release();
		g_pd3dDevice = NULL;
    }
}

void CreateRenderTarget()
{
    ID3D11Texture2D *pBackBuffer;
    g_pSwapChain->GetBuffer(0, IID_PPV_ARGS(&pBackBuffer));
    g_pd3dDevice->CreateRenderTargetView(pBackBuffer, NULL, &g_mainRenderTargetView);
    pBackBuffer->Release();
}

void CleanupRenderTarget()
{
    if (g_mainRenderTargetView) {
		g_mainRenderTargetView->Release();
		g_mainRenderTargetView = NULL;
    }
}

void SaveSettings(const std::string& filepath) {
    json settings;

    // Save keybinds, toggles, etc.
    settings["macrotoggled"] = macrotoggled;
    settings["shiftswitch"] = shiftswitch;
    settings["scancode_shift"] = scancode_shift;
    settings["camfixtoggle"] = camfixtoggle;
    settings["wallhopswitch"] = wallhopswitch;
    settings["wallhopcamfix"] = wallhopcamfix;
    settings["unequiptoggle"] = unequiptoggle;
    settings["toggle_jump"] = toggle_jump;
    settings["toggle_flick"] = toggle_flick;
    settings["vk_f5"] = vk_f5;
    settings["vk_f6"] = vk_f6;
    settings["vk_f8"] = vk_f8;
    settings["vk_mbutton"] = vk_mbutton;
    settings["vk_xbutton1"] = vk_xbutton1;
    settings["vk_xbutton2"] = vk_xbutton2;
    settings["vk_leftbracket"] = vk_leftbracket;
    settings["vk_spamkey"] = vk_spamkey;
    settings["vk_dkey"] = vk_spamkey;
    settings["vk_zkey"] = vk_zkey;
    settings["vk_dkey"] = vk_dkey;
    settings["vk_xkey"] = vk_xkey;
    settings["vk_clipkey"] = vk_clipkey;
    settings["vk_wallkey"] = vk_wallkey;
    settings["selected_dropdown"] = selected_dropdown;
    settings["text"] = text;

    // Save the rest of your settings
    settings["settingsBuffer"] = settingsBuffer;
    settings["KeyBuffer"] = KeyBuffer;
    settings["KeyBufferalt"] = KeyBufferalt;
    settings["ItemDesyncSlot"] = ItemDesyncSlot;
    settings["ItemSpeedSlot"] = ItemSpeedSlot;
    settings["ItemClipSlot"] = ItemClipSlot;
    settings["ItemClipDelay"] = ItemClipDelay;
    settings["RobloxSensValue"] = RobloxSensValue;
    settings["PreviousSensValue"] = PreviousSensValue;
    settings["SpamDelay"] = SpamDelay;
    settings["maxfreezetime"] = maxfreezetime;
    settings["isspeedswitch"] = isspeedswitch;
    settings["isfreezeswitch"] = isfreezeswitch;
    settings["iswallwalkswitch"] = iswallwalkswitch;
    settings["isspamswitch"] = isspamswitch;
    settings["isitemclipswitch"] = isitemclipswitch;
    settings["autotoggle"] = autotoggle;
    settings["PreviousWallWalkValue"] = PreviousWallWalkValue;
    settings["PreviousWallWalkSide"] = PreviousWallWalkSide;
    settings["RobloxWallWalkValueChar"] = RobloxWallWalkValueChar;
    settings["wallwalktoggleside"] = wallwalktoggleside;
    settings["RobloxWallWalkValueDelayChar"] = RobloxWallWalkValueDelayChar;
    settings["RobloxWallWalkValueDelay"] = RobloxWallWalkValueDelay;
    settings["RobloxPixelValue"] = RobloxPixelValue;
    settings["RobloxPixelValueChar"] = RobloxPixelValueChar;
    settings["WallhopPixels"] = WallhopPixels;
    settings["section_toggles"] = std::vector<bool>(section_toggles, section_toggles + section_amounts);
    settings["wallhop_dx"] = wallhop_dx;
    settings["wallhop_dy"] = wallhop_dy;
    settings["speed_strengthx"] = speed_strengthx;
    settings["speedoffsetx"] = speedoffsetx;
    settings["speed_strengthy"] = speed_strengthy;
    settings["speedoffsety"] = speedoffsety;
    settings["speed_slot"] = speed_slot;
    settings["desync_slot"] = desync_slot;
    settings["clip_slot"] = clip_slot;
    settings["clip_delay"] = clip_delay;
    settings["spam_delay"] = spam_delay;
    settings["real_delay"] = real_delay;
    settings["screen_width"] = screen_width;
    settings["screen_height"] = screen_height;
    settings["UserAcknowledgedV250"] = UserAcknowledgedV250;


    // Write the settings to file
    std::ofstream file(filepath);
    file << settings.dump(4); // Pretty print with indentation
}

void LoadSettings(const std::string& filepath) {
    try {
        std::ifstream file(filepath);
        if (file.is_open()) {
            json settings;
            file >> settings;

            // Use a map for boolean variables
            std::unordered_map<std::string, bool*> boolVariables = {
                {"macrotoggled", &macrotoggled},
                {"shiftswitch", &shiftswitch},
                {"wallhopswitch", &wallhopswitch},
                {"wallhopcamfix", &wallhopcamfix},
                {"unequiptoggle", &unequiptoggle},
                {"isspeedswitch", &isspeedswitch},
                {"isfreezeswitch", &isfreezeswitch},
                {"iswallwalkswitch", &iswallwalkswitch},
                {"isspamswitch", &isspamswitch},
                {"isitemclipswitch", &isitemclipswitch},
				{"autotoggle", &autotoggle},
				{"toggle_jump", &toggle_jump},
				{"toggle_flick", &toggle_flick},
				{"camfixtoggle", &camfixtoggle},
				{"wallwalktoggleside", &wallwalktoggleside},
				{"UserAcknowledgedV250", &UserAcknowledgedV250},

                // Add any additional boolean variables here
            };

            // Load boolean variables
            for (const auto& [key, ptr] : boolVariables) {
                if (settings.contains(key)) {
                    *ptr = settings[key].get<bool>();
                }
            }

            // Load other variables (non-boolean)
            scancode_shift = settings.value("scancode_shift", scancode_shift);
            vk_f5 = settings.value("vk_f5", vk_f5);
            vk_f6 = settings.value("vk_f6", vk_f6);
            vk_f8 = settings.value("vk_f8", vk_f8);
            vk_mbutton = settings.value("vk_mbutton", vk_mbutton);
            vk_xbutton1 = settings.value("vk_xbutton1", vk_xbutton1);
            vk_xbutton2 = settings.value("vk_xbutton2", vk_xbutton2);
            vk_leftbracket = settings.value("vk_leftbracket", vk_leftbracket);
            vk_spamkey = settings.value("vk_spamkey", vk_spamkey);
            vk_zkey = settings.value("vk_zkey", vk_zkey);
            vk_dkey = settings.value("vk_dkey", vk_dkey);
            vk_xkey = settings.value("vk_xkey", vk_xkey);
            vk_clipkey = settings.value("vk_clipkey", vk_clipkey);
            spam_delay = settings.value("spam_delay", spam_delay);
            real_delay = settings.value("real_delay", real_delay);
            wallhop_dx = settings.value("wallhop_dx", wallhop_dx);
            wallhop_dy = settings.value("wallhop_dy", wallhop_dy);
            selected_dropdown = settings.value("selected_dropdown", selected_dropdown);
            vk_wallkey = settings.value("vk_wallkey", vk_wallkey);
            PreviousWallWalkValue = settings.value("PreviousWallWalkValue", PreviousWallWalkValue);
            PreviousWallWalkSide = settings.value("PreviousWallWalkSide", PreviousWallWalkSide);
            maxfreezetime = settings.value("maxfreezetime", maxfreezetime);
            RobloxWallWalkValueDelay = settings.value("RobloxWallWalkValueDelay", RobloxWallWalkValueDelay);
            speed_strengthx = settings.value("speed_strengthx", speed_strengthx);
            speedoffsetx = settings.value("speedoffsetx", speedoffsetx);
            speed_strengthy = settings.value("speed_strengthy", speed_strengthy);
            speedoffsety = settings.value("speedoffsety", speedoffsety);
            speed_slot = settings.value("speed_slot", speed_slot);
            desync_slot = settings.value("desync_slot", desync_slot);
            clip_slot = settings.value("clip_slot", clip_slot);
            clip_delay = settings.value("clip_delay", clip_delay);
            screen_width = settings.value("screen_width", screen_width) + 16;
            screen_height = settings.value("screen_height", screen_height) + 39;

            // Load strings with strncpy
            strncpy(settingsBuffer, settings["settingsBuffer"].get<std::string>().c_str(), sizeof(settingsBuffer));
            strncpy(KeyBuffer, settings["KeyBuffer"].get<std::string>().c_str(), sizeof(KeyBuffer));
            strncpy(KeyBufferalt, settings["KeyBufferalt"].get<std::string>().c_str(), sizeof(KeyBufferalt));
            strncpy(ItemDesyncSlot, settings["ItemDesyncSlot"].get<std::string>().c_str(), sizeof(ItemDesyncSlot));
            strncpy(ItemSpeedSlot, settings["ItemSpeedSlot"].get<std::string>().c_str(), sizeof(ItemSpeedSlot));
            strncpy(ItemClipSlot, settings["ItemClipSlot"].get<std::string>().c_str(), sizeof(ItemClipSlot));
            strncpy(ItemClipDelay, settings["ItemClipDelay"].get<std::string>().c_str(), sizeof(ItemClipDelay));
            strncpy(RobloxSensValue, settings["RobloxSensValue"].get<std::string>().c_str(), sizeof(RobloxSensValue)); 
            strncpy(RobloxWallWalkValueChar, settings["RobloxWallWalkValueChar"].get<std::string>().c_str(), sizeof(RobloxWallWalkValueChar));
            strncpy(RobloxWallWalkValueDelayChar, settings["RobloxWallWalkValueDelayChar"].get<std::string>().c_str(), sizeof(RobloxWallWalkValueDelayChar));
            strncpy(WallhopPixels, settings["WallhopPixels"].get<std::string>().c_str(), sizeof(WallhopPixels));
            strncpy(SpamDelay, settings["SpamDelay"].get<std::string>().c_str(), sizeof(SpamDelay));
            strncpy(RobloxPixelValueChar, settings["RobloxPixelValueChar"].get<std::string>().c_str(), sizeof(RobloxPixelValueChar));
            text = settings.value("text", text);
            RobloxPixelValue = settings.value("RobloxPixelValue", RobloxPixelValue);
            PreviousSensValue = settings.value("PreviousSensValue", PreviousSensValue);
            std::vector<bool> toggles = settings.value("section_toggles", std::vector<bool>{});
            std::copy(toggles.begin(), toggles.end(), section_toggles);
        }
    } catch (const json::type_error &e) {
        // Handle the error (optional logging)
        std::cerr << "Error loading settings: " << e.what() << '\n';
        return;
    }
}

struct Section {
    std::string title;
    std::string description;
    bool optionA; 
    float settingValue; 
};

std::vector<Section> sections;

// Initialize your sections somewhere in your code
void InitializeSections() {
    for (int i = 0; i < section_amounts; ++i) {
        sections.push_back({
            "Section " + std::to_string(i),
            "This is a description for section " + std::to_string(i),
            false, // Default value for optionA
            50.0f // Default value for settingValue
        });
    }
}


void ChangeFirstSection() {
    sections[0].title = "Freeze";
    sections[0].description = "Automatically Tab Glitch With a Button";
}

void ChangeSecondSection() {
    sections[1].title = "Item Desync";
    sections[1].description = "Enable Item Collision (Hold Item Before Pressing)";
}

void ChangeThirdSection() {
    sections[2].title = "Helicopter High Jump";
    sections[2].description = "Use COM Offset to Catapult Yourself Into The Air by Aligning your Back Angled to the Wall and Jumping and Letting Your Character Turn";
}

void ChangeFourthSection() {
    sections[3].title = "Speedglitch";
    sections[3].description = "Use COM offset to Massively Increase Midair Speed";
}

void ChangeFifthSection() {
    sections[4].title = "Item Unequip COM Offset";
    sections[4].description = "Automatically Do a /e dance2 Item COM Offset Where You Unequip the Item";
}

void ChangeSixthSection() {
    sections[5].title = "Press a Button";
    sections[5].description = "Whenever You Press Your Keybind, it Presses the Other Button for One Frame";
}

void ChangeSeventhSection() {
    sections[6].title = "Wallhop";
    sections[6].description = "Automatically Flick and Jump to easily Wallhop On All FPS";
}

void ChangeEighthSection() {
    sections[7].title = "Walless LHJ";
    sections[7].description = "Lag High Jump Without a Wall by Offsetting COM Downwards or to the Right";
}

void ChangeNinthSection() {
    sections[8].title = "Item Clip";
    sections[8].description = "Clip through 2-3 Stud Walls Using Gears";
}

void ChangeTenthSection() {
    sections[9].title = "Wall-Walk";
    sections[9].description = "Walk Across Wall Seams Without Jumping";
}

void ChangeEleventhSection() {
    sections[10].title = "Spam a Key";
    sections[10].description = "Whenever You Press Your Keybind, it Spams the Other Button";
}



unsigned int BindKeyMode(unsigned int currentkey) {
    static int lastSelectedSection = -1; // Initialize with a value that won't match any valid section

    if (bindingMode) {
		rebindtime = std::chrono::high_resolution_clock::now();
        for (int key = 0; key < 255; key++) {
            if (GetAsyncKeyState(key) & 0x8000) {
                bindingMode = false;
                std::string currentkeystr = std::format("{:02x}", key); // Convert key into string
                unsigned int currentkeyint = std::stoul(currentkeystr, nullptr, 16); // Convert string into unsigned int
                std::snprintf(KeyBuffer, sizeof(KeyBuffer), "0x%02x", currentkeyint); // Update KeyBuffer text to your key
                return currentkeyint;
            }
        }
    } else { 
        // Check if the selected_section has changed
        if (selected_section != lastSelectedSection) {
            std::snprintf(KeyBuffer, sizeof(KeyBuffer), "0x%02x", currentkey); // Update KeyBuffer for the new section
            lastSelectedSection = selected_section; // Update lastSelectedSection to the current
        }
        
        KeyButtonText = "Click to Bind Key";

        // Don't crash if value is invalid
        try {
			currentkey = std::stoul(KeyBuffer, nullptr, 16);
		} catch (const std::invalid_argument& e) {
			currentkey = 0;
		} catch (const std::out_of_range& e) {
			currentkey = 0;
		}
        auto currentTime = std::chrono::high_resolution_clock::now();
        std::chrono::duration<double> elapsedtime = currentTime - rebindtime;

        if (elapsedtime.count() >= 0.2) { // .2 seconds passed
            notbinding = true;
        }
        return currentkey;
    }
}

unsigned int BindKeyModeAlt(unsigned int currentkey) {
    static int lastSelectedSection = -1; // Initialize with a value that won't match any valid section

    if (bindingModealt) {
		rebindtime = std::chrono::high_resolution_clock::now();
        for (int key = 0; key < 255; key++) {
            if (GetAsyncKeyState(key) & 0x8000) {
                bindingModealt = false;
                std::string currentkeystr = std::format("{:02x}", key); // Convert key into string
                unsigned int currentkeyint = std::stoul(currentkeystr, nullptr, 16); // Convert string into unsigned int
                std::snprintf(KeyBufferalt, sizeof(KeyBufferalt), "0x%02x", currentkeyint); // Update KeyBuffer text to your key
                return currentkeyint;
            }
        }
    } else { 
        // Check if the selected_section has changed
        if (selected_section != lastSelectedSection) {
            std::snprintf(KeyBufferalt, sizeof(KeyBufferalt), "0x%02x", currentkey); // Update KeyBuffer for the new section
            lastSelectedSection = selected_section; // Update lastSelectedSection to the current
        }
        
        KeyButtonTextalt = "Click to Bind Key";

		// Don't crash if value is invalid
		try {
			currentkey = std::stoul(KeyBufferalt, nullptr, 16);
		} catch (const std::invalid_argument& e) {
			currentkey = 0;
		} catch (const std::out_of_range& e) {
			currentkey = 0;
		}
        auto currentTime = std::chrono::high_resolution_clock::now();
        std::chrono::duration<double> elapsedtime = currentTime - rebindtime;

        if (elapsedtime.count() >= 0.2) { // .2 seconds passed
            notbinding = true;
        }
        return currentkey;
    }
}


void GetKeyNameFromHex(unsigned int hexKeyCode) {
    // Clear the buffer
    memset(KeyBufferhuman, 0, 256);

    // Map the virtual key code to a scan code
    UINT scanCode = MapVirtualKey(hexKeyCode, MAPVK_VK_TO_VSC);

	if (bindingMode) { // Turn off Key-Finding while binding
	    return;
	}

    // Attempt to get the readable key name
    if (GetKeyNameTextA(scanCode << 16, KeyBufferhuman, 256) > 0) {
        // Successfully retrieved the key name
        return; // No further action needed
    } else {
        // If GetKeyNameText fails, try to find the VK_ name
        auto it = vkToString.find(hexKeyCode);
        if (it != vkToString.end()) {
            strncpy(KeyBufferhuman, it->second.c_str(), 256);
        } else {
            // If not found, return a default hex representation
            snprintf(KeyBufferhuman, 256, "0x%X", hexKeyCode);
        }
    }
}

void GetKeyNameFromHexAlt(unsigned int hexKeyCode) {
    // Clear the buffer
    memset(KeyBufferhumanalt, 0, 256);

    // Map the virtual key code to a scan code
    UINT scanCode = MapVirtualKey(hexKeyCode, MAPVK_VK_TO_VSC);

	if (bindingModealt) { // Turn off Key-Finding while binding
	    return;
	}

    // Attempt to get the readable key name
	if (GetKeyNameTextA(scanCode << 16, KeyBufferhumanalt, 256) > 0) {
        // Successfully retrieved the key name
        return; // No further action needed
    } else {
        // If GetKeyNameText fails, try to find the VK_ name
        auto it = vkToString.find(hexKeyCode);
        if (it != vkToString.end()) {
            strncpy(KeyBufferhumanalt, it->second.c_str(), 256);
        } else {
            // If not found, return a default hex representation
            snprintf(KeyBufferhumanalt, 256, "0x%X", hexKeyCode);
        }
    }
}

void RunGUI() {
	// Initialize a basic Win32 window
    WNDCLASSEX wc = { sizeof(WNDCLASSEX), CS_CLASSDC, WndProc, 0L, 0L, GetModuleHandle(NULL), NULL, NULL, NULL, NULL, _T("Roblox Macro Client"), NULL };

	wc.hIconSm = (HICON)LoadImage(wc.hInstance, MAKEINTRESOURCE(IDI_ICON2), IMAGE_ICON, 16, 16, LR_DEFAULTCOLOR);
    wc.hIcon = NULL;

    RegisterClassEx(&wc);
    HWND hwnd = CreateWindow(wc.lpszClassName, _T("Roblox Macro Client"), WS_OVERLAPPEDWINDOW, 100, 100, 1280, 800, NULL, NULL, wc.hInstance, NULL);
    SetWindowPos(hwnd, NULL, 0, 0, screen_width, screen_height, SWP_NOZORDER | SWP_NOMOVE);

    // Initialize Direct3D
    if (!CreateDeviceD3D(hwnd)) {
		CleanupDeviceD3D();
		UnregisterClass(wc.lpszClassName, wc.hInstance);
    }

	// Show the window
    LONG_PTR style = GetWindowLongPtr(hwnd, GWL_EXSTYLE);
    SetWindowLongPtr(hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED);
    SetLayeredWindowAttributes(hwnd, 0, 255, LWA_ALPHA); // Set to full opacity
    ShowWindow(hwnd, SW_SHOWDEFAULT);
    UpdateWindow(hwnd);

    // Initialize ImGui context
    IMGUI_CHECKVERSION();
    ImGui::CreateContext();
    ImGuiIO& io = ImGui::GetIO();
	io.IniFilename = NULL;
    (void)io;
    io.ConfigFlags |= ImGuiConfigFlags_NavEnableKeyboard; // Enable Keyboard Controls

	ImFont *mainfont = io.Fonts->AddFontFromMemoryTTF(LSANS_ttf, LSANS_ttf_len, 20.0f);

    // Initialize ImGui for Win32 and DirectX 11
    ImGui_ImplWin32_Init(hwnd);
    ImGui_ImplDX11_Init(g_pd3dDevice, g_pd3dDeviceContext);


    auto lastTime = std::chrono::high_resolution_clock::now();
    float targetFrameTime = 1.0f / 120.0f;  // 120 FPS target
	InitializeSections();
    ChangeFirstSection();
    ChangeSecondSection();
    ChangeThirdSection();
    ChangeFourthSection();
    ChangeFifthSection();
    ChangeSixthSection();
    ChangeSeventhSection();
    ChangeEighthSection();
    ChangeNinthSection();
    ChangeTenthSection();
    ChangeEleventhSection();


	MSG msg;

    // Attach the GUI thread to the input of the main thread
    DWORD mainThreadId = GetWindowThreadProcessId(hwnd, NULL);
    DWORD guiThreadId = GetCurrentThreadId();
    AttachThreadInput(mainThreadId, guiThreadId, TRUE); // Attach the threads

    while (running) {
        while (PeekMessage(&msg, NULL, 0U, 0U, PM_REMOVE))
        {
            TranslateMessage(&msg);
            DispatchMessage(&msg);
            if (msg.message == WM_QUIT)
            {
                running = false;
                break;
            }
        }

        auto currentTime = std::chrono::high_resolution_clock::now();
        std::chrono::duration<float> deltaTime = currentTime - lastTime;

        if (deltaTime.count() >= targetFrameTime) {
            lastTime = currentTime;

            // Start ImGui frame
            ImGui_ImplDX11_NewFrame();
            ImGui_ImplWin32_NewFrame();
            ImGui::NewFrame();

            // ImGui window dimensions
            ImVec2 display_size = ImGui::GetIO().DisplaySize;


            // Set window flags to disable resizing, moving, and title bar
            ImGuiWindowFlags window_flags = ImGuiWindowFlags_NoResize |
                                             ImGuiWindowFlags_NoMove |
                                             ImGuiWindowFlags_NoTitleBar |
											 ImGuiWindowFlags_NoScrollbar |
											 ImGuiWindowFlags_NoScrollWithMouse;

            // Set the size of the main ImGui window to fill the screen, fitting to the top left
            ImGui::SetNextWindowSize(display_size, ImGuiCond_Always);
            ImGui::SetNextWindowPos(ImVec2(0, 0), ImGuiCond_Always);

            // Create the main window with the specified flags
            ImGui::Begin("My ImGUI Window", nullptr, window_flags); // Main ImGui window

            // Top settings child window (occupying 20% of the screen height)
            float settings_panel_height = display_size.y * 0.2f;
            ImGui::BeginChild("GlobalSettings", ImVec2(display_size.x - 15, settings_panel_height), true);

            // Example global settings (replace with your actual settings)
            ImGui::TextWrapped("Global Settings");
			ImGui::SameLine(ImGui::GetWindowWidth() - 800);
			ImGui::TextWrapped("DISCLAIMER: THIS IS NOT A CHEAT, IT NEVER INTERACTS WITH ROBLOX MEMORY.");
			// Create a checkbox for macrotoggled
            ImGui::Checkbox("Macro Toggle (Anti-AFK remains!)", &macrotoggled); // Checkbox for toggling
			ImGui::SameLine(ImGui::GetWindowWidth() - 800);
			ImGui::TextWrapped("The ONLY official source for this is https://github.com/Spencer0187/Roblox-Macro-Utilities");
			ImGui::TextWrapped("Roblox Executable Name:");
			ImGui::SameLine();
			ImGui::SetNextItemWidth(250.0f);

			ImGui::InputText("##SettingsTextbox", settingsBuffer, sizeof(settingsBuffer), ImGuiInputTextFlags_CharsNoBlank); // Textbox for input, remove blank characters

			ImGui::SameLine();

			ImDrawList* drawList = ImGui::GetWindowDrawList();
			ImVec2 pos = ImGui::GetCursorScreenPos();
			pos.y += ImGui::GetTextLineHeight() / 2 - 3;
			ImU32 color = processFound ? IM_COL32(0, 255, 0, 255) : IM_COL32(255, 0, 0, 255);

			drawList->AddCircleFilled(ImVec2(pos.x + 5, pos.y + 5), 5, color);
			ImGui::Dummy(ImVec2(5 * 2, 5 * 2));


			ImGui::Checkbox("Switch Macro From \"Left Shift\" to \"Control\" for Shiftlock", &shiftswitch); // Checkbox for toggling
			ImGui::SameLine(ImGui::GetWindowWidth() - 360);
			ImGui::TextWrapped("MANUALLY SAVE SETTINGS:");
			ImGui::SameLine(ImGui::GetWindowWidth() - 125);
			if (ImGui::Button("Save Settings")) {
				SaveSettings("RMCSettings.json");
			}

			if (shiftswitch) {
				scancode_shift = 0x1D;
			} else {
				scancode_shift = 0x2A;
			}
			ImGui::TextWrapped("Roblox Sensitivity (0-4):");
			ImGui::SameLine();
			ImGui::SetNextItemWidth(70.0f);
			ImGui::InputText("", RobloxSensValue, sizeof(RobloxSensValue), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank);
			ImGui::SameLine();
			ImGui::Text("Game Uses Cam-Fix:");
			ImGui::SameLine();
			
			if (ImGui::Checkbox("###", &camfixtoggle)) {
				PreviousSensValue = -1;
				PreviousWallWalkValue = -1;
				if (wallhopswitch) {
					if (camfixtoggle) {
						wallhop_dx = std::round(std::stoi(WallhopPixels) * -1.388888889);
						wallhop_dy = std::round(std::stoi(WallhopPixels) * 1.388888889);
					} else {
						wallhop_dx = std::round(std::stoi(WallhopPixels) / -1.388888889);
						wallhop_dy = std::round(std::stoi(WallhopPixels) / 1.388888889);
					}
				} else {
					if (camfixtoggle) {
						wallhop_dx = std::round(std::stoi(WallhopPixels) * 1.388888889);
						wallhop_dy = std::round(std::stoi(WallhopPixels) * -1.388888889);
					} else {
						wallhop_dx = std::round(std::stoi(WallhopPixels) / 1.388888889);
						wallhop_dy = std::round(std::stoi(WallhopPixels) / -1.388888889);
					}
				}
				sprintf(WallhopPixels, "%d", wallhop_dx);

				float CurrentWallWalkValue = atof(RobloxSensValue);

				if (camfixtoggle) {
					wallwalk_strengthx = -static_cast<int>(std::round(((500.0f / CurrentWallWalkValue) * 0.13f)));
					wallwalk_strengthy = static_cast<int>(std::round(((500.0f / CurrentWallWalkValue) * 0.13f)));
				} else {
					wallwalk_strengthx = -static_cast<int>(std::round(((360.0f / CurrentWallWalkValue) * 0.13f)));
					wallwalk_strengthy = static_cast<int>(std::round(((360.0f / CurrentWallWalkValue) * 0.13f)));
				}

				sprintf(RobloxWallWalkValueChar, "%d", wallwalk_strengthx);

				float CurrentSensValue = atof(RobloxSensValue);
				if (camfixtoggle) {
					try {
						RobloxPixelValue = static_cast<int>(std::round((500.0f / CurrentSensValue) * (static_cast<float>(359) / 360)));
					} catch (const std::invalid_argument &e) {
					} catch (const std::out_of_range &e) {
					}
							
				} else {
					try {
						RobloxPixelValue = static_cast<int>(std::round((360.0f / CurrentSensValue) * (static_cast<float>(359) / 360)));
					} catch (const std::invalid_argument &e) {
					} catch (const std::out_of_range &e) {
					}
				}
				PreviousSensValue = CurrentSensValue;
				sprintf(RobloxPixelValueChar, "%d", RobloxPixelValue);
			}
				

			ImGui::SameLine(ImGui::GetWindowWidth() - 350);
			ImGui::TextWrapped("AUTOSAVES ON QUIT      VERSION 2.6.0");

            ImGui::EndChild(); // End Global Settings child window

            // Left panel takes 30% of the window width
            float left_panel_width = display_size.x * 0.3f;

            // Create a left child window (scrollable section) with dynamic sizing
            ImGui::BeginChild("LeftScrollSection", ImVec2(left_panel_width - 23, display_size.y - settings_panel_height - 20), true);


            // Scrollable mini-sections (always expanded)
			for (size_t i = 0; i < sections.size(); ++i) {
				// Calculate available button width based on the left panel size (use full width with a small margin)
				float left_panel_width = ImGui::GetWindowSize().x - 15;
				float buttonWidth = left_panel_width - ImGui::GetStyle().FramePadding.x * 2;  // Deduct padding

				// Split the title and description for rendering
				std::string titleText = sections[i].title;
				std::string descriptionText = sections[i].description;

				// Manually wrap the description text and calculate height
				ImVec2 textPos = ImVec2(0, 0);  // Position of text relative to the button
				ImVec2 titleSize = ImGui::CalcTextSize(titleText.c_str(), nullptr, true);  // Title size
				ImVec2 descriptionSize = ImGui::CalcTextSize(descriptionText.c_str(), nullptr, true, buttonWidth);  // Wrapped description size
				float buttonHeight = titleSize.y + descriptionSize.y + ImGui::GetStyle().FramePadding.y * 2;  // Add padding to determine button height

				// Use PushID to create a unique ID for each button
				ImGui::PushID(i);

				// Set button colors based on toggle state
				if (section_toggles[i]) {
					ImGui::PushStyleColor(ImGuiCol_Button, ImVec4(39.0f / 255.0f, 73.0f / 255.0f, 114.0f / 255.0f, 1.0f)); // Light blue
					ImGui::PushStyleColor(ImGuiCol_ButtonHovered, ImVec4(66.0f / 255.0f, 150.0f / 255.0f, 250.0f / 255.0f, 1.0f)); // Lighter blue
					ImGui::PushStyleColor(ImGuiCol_ButtonActive, ImVec4(15.0f / 255.0f, 135.0f / 255.0f, 250.0f / 255.0f, 1.0f)); // Darker blue
				} else {
					ImGui::PushStyleColor(ImGuiCol_Button, ImVec4(114.0f / 255.0f, 73.0f / 255.0f, 39.0f / 255.0f, 1.0f)); // Light red
					ImGui::PushStyleColor(ImGuiCol_ButtonHovered, ImVec4(250.0f / 255.0f, 150.0f / 255.0f, 66.0f / 255.0f, 1.0f)); // Lighter red
					ImGui::PushStyleColor(ImGuiCol_ButtonActive, ImVec4(250.0f / 255.0f, 135.0f / 255.0f, 15.0f / 255.0f, 1.0f)); // Darker red
				}

				// Create the button
				if (ImGui::Button("", ImVec2(buttonWidth, buttonHeight))) {
					selected_section = i;  // Update selected section
				}

				// Get button's position for custom drawing
				ImVec2 buttonPos = ImGui::GetItemRectMin();
				textPos = ImVec2(buttonPos.x + ImGui::GetStyle().FramePadding.x, buttonPos.y + ImGui::GetStyle().FramePadding.y);

				ImDrawList* drawList = ImGui::GetWindowDrawList();

				// Draw the title at the top of the button
				drawList->AddText(textPos, IM_COL32(255, 255, 255, 255), titleText.c_str());
				textPos.y += titleSize.y;  // Move position down after the title

				// Manually wrap and draw the description below the title
				std::stringstream ss(descriptionText);
				std::string word;
				std::string currentLine;

				// Perform text-wrapping for the description within button width
				while (ss >> word) {
					std::string potentialLine = currentLine + (currentLine.empty() ? "" : " ") + word;
					ImVec2 potentialLineSize = ImGui::CalcTextSize(potentialLine.c_str());

					if (potentialLineSize.x > buttonWidth) {
						// Draw the current line and move to the next
						drawList->AddText(textPos, IM_COL32(255, 255, 255, 255), currentLine.c_str());
						textPos.y += potentialLineSize.y;
						currentLine = word;
					} else {
						currentLine = potentialLine;
					}
				}

				// Draw the last line
				if (!currentLine.empty()) {
					drawList->AddText(textPos, IM_COL32(255, 255, 255, 255), currentLine.c_str());
				}

				// Reset styles and pop ID
				ImGui::PopStyleColor(3);
				ImGui::PopID();

				// Optional separator between buttons
				ImGui::Separator();
			}


            // End left scroll section
            ImGui::EndChild();

            // Right section (content changes based on the selected section)
            ImGui::SameLine(); // Move to the right of the left section

            // Right child window with dynamic sizing
            ImGui::BeginChild("RightSection", ImVec2(display_size.x - left_panel_width, display_size.y - settings_panel_height - 20), true);

            // Display different content based on the selected mini-section
            if (selected_section >= 0 && selected_section < sections.size()) {

                ImGui::TextWrapped("Settings for %s", sections[selected_section].title.c_str());
				ImGui::Separator(); 
				ImGui::NewLine();
                ImGui::TextWrapped("Keybind:");
				ImGui::SameLine();
				if (ImGui::Button(KeyButtonText.c_str())) {
					notbinding = false;
					bindingMode = true;
					KeyButtonText = "Press a Key...";
					}
				ImGui::SameLine();

				if (selected_section == 0) {
					vk_mbutton = BindKeyMode(vk_mbutton);
					ImGui::SetNextItemWidth(150.0f);
					GetKeyNameFromHex(vk_mbutton);
					}

				if (selected_section == 1) {
					vk_f5 = BindKeyMode(vk_f5);
					ImGui::SetNextItemWidth(150.0f);
					GetKeyNameFromHex(vk_f5);
					}

				if (selected_section == 2) {
					vk_xbutton1 = BindKeyMode(vk_xbutton1);
					ImGui::SetNextItemWidth(150.0f);
					GetKeyNameFromHex(vk_xbutton1);
					}

				if (selected_section == 3) {
					vk_xkey = BindKeyMode(vk_xkey);
					ImGui::SetNextItemWidth(150.0f);
					GetKeyNameFromHex(vk_xkey);
					}

				if (selected_section == 4) {
					vk_f8 = BindKeyMode(vk_f8);
					ImGui::SetNextItemWidth(150.0f);
					GetKeyNameFromHex(vk_f8);
				}

				if (selected_section == 5) {
					vk_zkey = BindKeyMode(vk_zkey);
					ImGui::SetNextItemWidth(150.0f);
					GetKeyNameFromHex(vk_zkey);
				}

				if (selected_section == 6) {
					vk_xbutton2 = BindKeyMode(vk_xbutton2);
					ImGui::SetNextItemWidth(150.0f);
					GetKeyNameFromHex(vk_xbutton2);
				}

				if (selected_section == 7) {
					vk_f6 = BindKeyMode(vk_f6);
					ImGui::SetNextItemWidth(150.0f);
					GetKeyNameFromHex(vk_f6);
				}

				if (selected_section == 8) {
					vk_clipkey = BindKeyMode(vk_clipkey);
					ImGui::SetNextItemWidth(150.0f);
					GetKeyNameFromHex(vk_clipkey);
				}

				if (selected_section == 9) {
					vk_wallkey = BindKeyMode(vk_wallkey);
					ImGui::SetNextItemWidth(150.0f);
					GetKeyNameFromHex(vk_wallkey);
				}

				if (selected_section == 10) {
					vk_leftbracket = BindKeyMode(vk_leftbracket);
					ImGui::SetNextItemWidth(150.0f);
					GetKeyNameFromHex(vk_leftbracket);
				}


				ImGui::InputText("##", KeyBufferhuman, sizeof(KeyBufferhuman), ImGuiInputTextFlags_CharsNoBlank);
				ImGui::SameLine();
				ImGui::TextWrapped("Key Binding");
				ImGui::SameLine();
				ImGui::SetNextItemWidth(50.0f);
				ImGui::InputText("###", KeyBuffer, sizeof(KeyBuffer), ImGuiInputTextFlags_CharsNoBlank);
				ImGui::SameLine();
				ImGui::TextWrapped("Key Binding (Hexadecimal)");
				ImGui::TextWrapped("Toggle Macro:");
				ImGui::SameLine();
				if (selected_section >= 0 && selected_section < section_amounts) {
					ImGui::Checkbox(("##SectionToggle" + std::to_string(selected_section)).c_str(), &section_toggles[selected_section]);
				}
				ImGui::SameLine(243);
				ImGui::TextWrapped("(Human-Readable)");

				if (selected_section == 0) { // Freeze Macro
					ImGui::SetNextItemWidth(300.0f);
					ImGui::SliderFloat("", &maxfreezetime, 0.0f, 9.8f, "%.2f Seconds");
					ImGui::SameLine();
					ImGui::TextWrapped("Automatically Unfreeze for 50ms when you hit this time (Anti-Internet-Kick)");
					ImGui::SetNextItemWidth(180.0f);
					ImGui::InputFloat("Unfreeze Time Above", &maxfreezetime, 0.01f, 1.0f, "%.2f");
					ImGui::Checkbox("Switch from Hold Key to Toggle Key", &isfreezeswitch);
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
					ImGui::InputText("", ItemDesyncSlot, sizeof(ItemDesyncSlot), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank);
					try {
						desync_slot = std::stoi(ItemDesyncSlot);
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
					ImGui::TextWrapped(
										"Also, for convenience sake, you cannot activate desync unless you're tabbed into roblox, You will "
										"most likely crash any other program if you activate it in there.");
				}

				if (selected_section == 2) { // HHJ
					ImGui::Checkbox("Automatically time inputs", &autotoggle);
					ImGui::SameLine();
					ImGui::TextWrapped("(EXTREMELY BUGGY/EXPERIMENTAL, WORKS BEST ON HIGH FPS AND SHALLOW ANGLE TO WALL)");
					ImGui::Separator();
					ImGui::TextWrapped("IMPORTANT:");
					ImGui::TextWrapped("FOR MOST OPTIMAL RESULTS GO TO THE SPEEDGLITCH MENU, AND PROPERLY SET YOUR SPEEDGLITCH PIXEL VALUE!");
					ImGui::Separator();
					ImGui::TextWrapped("Explanation:");
					ImGui::NewLine();
					ImGui::TextWrapped("This macro abuses Roblox's conversion from angular velocity to regular velocity. If you put your "
										"back against a wall, rotate left 20-30 degrees, turn around 180 degrees, hold jump, and press W "
										"as you land on the floor, then, activate the macro, and let go of W, if you did it correctly, "
										"you will rotate into the wall, and get your feet stuck inside of it, the macro freezes the game "
										"during this process, and you will be catapulted up DEPENDANT ON YOUR CENTER OF MASS OFFSET "
										"Bigger COM offset = Easier to perform and higher height");
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
					ImGui::InputText("##PixelValue", RobloxPixelValueChar, sizeof(RobloxPixelValueChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank);

					try { // Error Handling
						speed_strengthx = std::stoi(RobloxPixelValueChar);
						speed_strengthy = -std::stoi(RobloxPixelValueChar);
					} catch (const std::invalid_argument &e) {
					} catch (const std::out_of_range &e) {
					}

					ImGui::Checkbox("Switch from Toggle Key to Hold Key", &isspeedswitch);

					ImGui::Separator();
					ImGui::TextWrapped("IMPORTANT: FOR MOST OPTIMAL RESULTS, INPUT YOUR ROBLOX INGAME SENSITIVITY!");
					ImGui::TextWrapped("ALSO, YOU MUST BE ON 60 FPS FOR MAXIMUM SPEED!");
					ImGui::TextWrapped("TICK OR UNTICK THE CHECKBOX DEPENDING ON WHETHER THE GAME USES CAM-FIX MODULE OR NOT. "
										"If you don't know, do BOTH and check which one provides you with a 180 degree rotation. "
										"Also, for convenience sake, you cannot activate speedglitch unless you're tabbed into roblox.");
					ImGui::Separator();
					ImGui::TextWrapped("Explanation:");
					ImGui::NewLine();
					ImGui::TextWrapped("This macro uses the way a changed center of mass affects your movement. If you offset your COM "
										"in any way, and then toggle the macro, you will rotate 180 degrees every frame, holding W and "
										"jump during this will catapult you forward.");
				}

				if (selected_section == 4) { // Gear Unequip speed
					ImGui::TextWrapped("Gear Slot:");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(30.0f);
					ImGui::InputText("", ItemSpeedSlot, sizeof(ItemSpeedSlot), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank);
					try { // Error Handling
						speed_slot = std::stoi(ItemSpeedSlot);
					} catch (const std::invalid_argument &e) {
					} catch (const std::out_of_range &e) {
					}
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
					ImGui::Checkbox("Don't Unequip Item at the End", &unequiptoggle);
					ImGui::Separator();
					ImGui::TextWrapped("Explanation:");
					ImGui::NewLine();
					ImGui::TextWrapped("R6 ONLY!");
					ImGui::TextWrapped("HAVE THE ITEM UNEQUIPPED BEFORE DOING THIS!!!");
					ImGui::TextWrapped("Performs a weaker version of the /e dance2 equip speedglitch, however, you unequip your gear. "
										"Unequipping your gear will make HHJ's feel easier to perform, will keep COM after gear deletion, "
										"AND, speedglitching while in this state will move you PERFECTLY forwards, no side movement.");
				}

				if (selected_section == 5) { // Button Press
					ImGui::TextWrapped("Key to Press:");
					ImGui::SameLine();
					if (ImGui::Button((KeyButtonTextalt + "##").c_str())) {
						notbinding = false;
						bindingModealt = true;
						KeyButtonTextalt = "Press a Key...";
						}
					ImGui::SameLine();
					vk_dkey = BindKeyModeAlt(vk_dkey);
					ImGui::SetNextItemWidth(150.0f);
					GetKeyNameFromHexAlt(vk_dkey);
					ImGui::InputText("Key to Press (Human-Readable)", KeyBufferhumanalt, sizeof(KeyBufferhumanalt), ImGuiInputTextFlags_CharsNoBlank);
					ImGui::SameLine();
					ImGui::SetNextItemWidth(50.0f);
					ImGui::InputText("Key to Press (Hexadecimal)", KeyBufferalt, sizeof(KeyBufferalt), ImGuiInputTextFlags_CharsNoBlank);
					ImGui::Separator();
					ImGui::TextWrapped("Explanation:");
					ImGui::NewLine();
					ImGui::TextWrapped("It will press the second keybind for a single frame whenever you press the first keybind. "
										"This is most commonly used for micro-adjustments while moving, especially if you do this while jumping.");
				}

				if (selected_section == 6) { // Wallhop
					ImGui::TextWrapped("Flick Pixel Amount:");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(70.0f);
					ImGui::InputText("", WallhopPixels, sizeof(WallhopPixels), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank);
					wallhop_dx = std::round(std::stoi(WallhopPixels));
					wallhop_dy = -std::round(std::stoi(WallhopPixels));

					ImGui::Checkbox("Switch to Left-Flick Wallhop", &wallhopswitch); // Left Sided wallhop switch
					ImGui::Checkbox("Jump During Wallhop", &toggle_jump);
					ImGui::Checkbox("Flick-Back During Wallhop", &toggle_flick);

					ImGui::Separator();
					ImGui::TextWrapped("IMPORTANT:");
					ImGui::TextWrapped("THE ANGLE THAT YOU TURN IS DIRECTLY RELATED TO YOUR ROBLOX SENSITIVITY. "
										"If you want to pick a SPECIFIC ANGLE, heres how. "
										"For games without the cam-fix module, 360 degrees is equal to 360 divided by your Roblox Sensitivity. "
										"For games with the cam-fix module, 360 degrees is equal to 500 divided by your Roblox Sensitivity. "
										"Ex: 0.6 sens with no cam fix = 600 pixels, which means 600 / 8 (75) is equal to a 45 degree turn.");
					ImGui::TextWrapped("INTEGERS ONLY!");
					ImGui::Separator();
					ImGui::TextWrapped("Explanation:");
					ImGui::NewLine();
					ImGui::TextWrapped("This Macro automatically flicks your screen AND jumps at the same time, performing a wallhop.");
				}

				if (selected_section == 7) { // Walless LHJ
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

					ImGui::InputText("", ItemClipSlot, sizeof(ItemClipSlot), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank);
					try {
						clip_slot = std::stoi(ItemClipSlot);
					} catch (const std::invalid_argument &e) {
					} catch (const std::out_of_range &e) {
					}

					ImGui::TextWrapped("Item Clip Delay in Milliseconds (Default 30ms):");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(120.0f);
					ImGui::InputText("##", ItemClipDelay, sizeof(ItemClipDelay), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank);
					try { // Error Handling
						clip_delay = std::stoi(ItemClipDelay);
					} catch (const std::invalid_argument &e) {
					} catch (const std::out_of_range &e) {
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

				if (selected_section == 9) { // Wall-Walk

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
										"you can walk up to a wall, maybe at a bit of an angle, and hold W and D or A to slowly walk across");
			}

				if (selected_section == 10) { // Spamkey
					ImGui::TextWrapped("Key to Press:");
					ImGui::SameLine();
					if (ImGui::Button((KeyButtonTextalt + "##").c_str())) {
						bindingModealt = true;
						notbinding = false;
						KeyButtonTextalt = "Press a Key...";
						}
					ImGui::SameLine();
					vk_spamkey = BindKeyModeAlt(vk_spamkey);
					ImGui::SetNextItemWidth(150.0f);
					GetKeyNameFromHexAlt(vk_spamkey);
					ImGui::InputText("Key to Press (Human-Readable)", KeyBufferhumanalt, sizeof(KeyBufferhumanalt), ImGuiInputTextFlags_CharsNoBlank);
					ImGui::SameLine();
					ImGui::SetNextItemWidth(50.0f);
					ImGui::InputText("Key to Press (Hexadecimal)", KeyBufferalt, sizeof(KeyBufferalt), ImGuiInputTextFlags_CharsNoBlank);
					ImGui::TextWrapped("Spam Delay (Milliseconds but accepts decimals):");
					ImGui::SameLine();
					ImGui::SetNextItemWidth(120.0f);
					ImGui::InputText("", SpamDelay, sizeof(SpamDelay), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank);
					try { // Error Handling
						spam_delay = std::stof(SpamDelay);
						real_delay = static_cast<int>((spam_delay * 1000.0f + 0.5f) / 2);

					} catch (const std::invalid_argument &e) {
					} catch (const std::out_of_range &e) {
					}
					ImGui::TextWrapped("I do not take any responsibility if you set the delay to 0ms");
					ImGui::Checkbox("Switch from Toggle Key to Hold Key", &isspamswitch);
					ImGui::Separator();
					ImGui::TextWrapped("Explanation:");
					ImGui::NewLine();
					ImGui::TextWrapped("This macro will spam the second key with a millisecond delay. "
										"This can be used as an autoclicker for any games you want, or a key-spam.");
				}


            } else {
                ImGui::TextWrapped("Select a section to see its settings.");
            }


            ImGui::EndChild(); // End right section

            // Finish the main window
            ImGui::End(); // End main ImGui window

            // Render
            ImGui::Render();
            const float clear_color[] = { 0.45f, 0.55f, 0.60f, 1.00f };
            g_pd3dDeviceContext->OMSetRenderTargets(1, &g_mainRenderTargetView, NULL);
            g_pd3dDeviceContext->ClearRenderTargetView(g_mainRenderTargetView, clear_color);
            ImGui_ImplDX11_RenderDrawData(ImGui::GetDrawData());

            g_pSwapChain->Present(1, 0);  // Vsync present for smooth rendering
        }


        // Small sleep to avoid consuming too much CPU
        std::this_thread::sleep_for(std::chrono::microseconds(500));
    }
    UnregisterClass(wc.lpszClassName, wc.hInstance);
    AttachThreadInput(mainThreadId, guiThreadId, FALSE);
}




int WINAPI WinMain(HINSTANCE hInstance, HINSTANCE hPrevInstance, LPSTR lpCmdLine, int nCmdShow)
{
    LoadSettings("RMCSettings.json");

    std::string remoteVersion = GetRemoteVersion();
    if (remoteVersion == "") {
		remoteVersion = "HTTP request for latest version failed!";
    }
    std::string localVersion = "2.6.0";

    if (Trim(remoteVersion) != localVersion && !UserAcknowledgedV250) {
		std::wstring remote_version = std::wstring(remoteVersion.begin(), remoteVersion.end());
		std::wstring local_version = std::wstring(localVersion.begin(), localVersion.end());
        std::wstring message = L"Your Version is Outdated! The latest version is: " + remote_version + L". Your version is: " + local_version + L". \nDo you understand this? If you press yes, this won't show up again.";
        
        int result = MessageBox(NULL, 
            message.c_str(), 
            L"Update Notification", 
            MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON1 | MB_APPLMODAL);

        if (result == IDYES) {
            UserAcknowledgedV250 = true;  // Change the variable
        }
    }

	const HMODULE hNtdll = GetModuleHandleA("ntdll");
	NtSuspendProcess pfnSuspend = reinterpret_cast<NtSuspendProcess>(GetProcAddress(hNtdll, "NtSuspendProcess"));
	NtResumeProcess pfnResume = reinterpret_cast<NtResumeProcess>(GetProcAddress(hNtdll, "NtResumeProcess"));
	

	std::thread actionThread(Speedglitchloop); // Start a separate thread for item desync loop, ONLY do this for SPAMMERS that need to run ALONGSIDE other functions
	std::thread actionThread2(ItemDesyncLoop);
	std::thread actionThread3(SpeedglitchloopHHJ);
	std::thread actionThread4(SpamKeyLoop);
	std::thread actionThread5(ItemClipLoop);
	std::thread actionThread6(WallWalkLoop);
	std::thread guiThread(RunGUI);
	MSG msg;


	HANDLE hProcess = OpenProcess(PROCESS_ALL_ACCESS, FALSE, GetProcessIdByName()); // Get hProcess
	HWND rbxhwnd = FindWindowByProcessHandle(hProcess); // Make sure HWND is set correctly

	bool isdesync = false; // These variables are used for "one-click" functionalies for macros, so you can press a key and it runs every time that key is pressed (without overlapping itself)
	bool isSuspended = false; 
	bool islhj = false;
	bool ispressd = false;
	bool iswallhop = false;
	bool isspeedglitch = false;
	bool isunequipspeed = false;
	bool HHJ = false;
	bool isspam = false;
	bool isclip = false;
	bool iswallwalk = false;
	auto lastPressTime = std::chrono::steady_clock::now();
	auto lastProcessCheck = std::chrono::steady_clock::now();
	static const float targetFrameTime = 1.0f / 120.0f; // Targeting 120 FPS
	auto lastTime = std::chrono::high_resolution_clock::now();

	while (!done) {
    {
		if (macrotoggled && notbinding && section_toggles[0]) {
			bool isMButtonPressed = GetAsyncKeyState(vk_mbutton) & 0x8000;

			if (isfreezeswitch) {  // Toggle mode
				if (isMButtonPressed && !wasMButtonPressed) {  // Detect button press edge
					isSuspended = !isSuspended;  // Toggle the freeze state
					SuspendOrResumeProcess(pfnSuspend, pfnResume, hProcess, isSuspended);

					if (isSuspended) {
						suspendStartTime = std::chrono::steady_clock::now();  // Start the timer
					}
				}
			} else {  // Hold mode
				if (isMButtonPressed) {
					if (!isSuspended) {
						SuspendOrResumeProcess(pfnSuspend, pfnResume, hProcess, true);  // Freeze on hold
						isSuspended = true;
						suspendStartTime = std::chrono::steady_clock::now();  // Start the timer
					}
				} else if (isSuspended) {
					SuspendOrResumeProcess(pfnSuspend, pfnResume, hProcess, false);  // Unfreeze on release
					isSuspended = false;
				}
			}

			// Common timer logic for both toggle and hold modes
			if (isSuspended) {
				auto elapsed = std::chrono::duration_cast<std::chrono::milliseconds>(
					std::chrono::steady_clock::now() - suspendStartTime).count();

				if (elapsed >= (maxfreezetime * 1000)) {
					// Unsuspend for 50 ms
					SuspendOrResumeProcess(pfnSuspend, pfnResume, hProcess, false);
					std::this_thread::sleep_for(std::chrono::milliseconds(unsuspendTime));
					SuspendOrResumeProcess(pfnSuspend, pfnResume, hProcess, true);

					// Reset the timer
					suspendStartTime = std::chrono::steady_clock::now();
				}
			}

			// Update the previous state
			wasMButtonPressed = isMButtonPressed;
		}


		if ((GetAsyncKeyState(vk_f5) & 0x8000) && IsForegroundWindowProcess(rbxhwnd) && macrotoggled && notbinding && section_toggles[1]) { // Item Desync Macro with anti-idiot design
			if (!isdesync) {
				isdesyncloop = true;
				isdesync = true;
			}
		} else {
			isdesync = false;
			isdesyncloop = false;
		}

		if ((GetAsyncKeyState(vk_zkey) & 0x8000) && macrotoggled && notbinding && section_toggles[5]) { // Press button for one frame
			if (!ispressd) {
				HoldKeyMouse(vk_dkey);
				std::this_thread::sleep_for(std::chrono::milliseconds(6));
				ReleaseKeyMouse(vk_dkey);
				ispressd = true;
			}
		} else {
			ispressd = false;
		}

		if ((GetAsyncKeyState(vk_xbutton2) & 0x8000) && macrotoggled && notbinding && section_toggles[6]) { // Wallhop
			if (!iswallhop) {

				if (toggle_jump) {
					HoldKey(0x39);
				}

				if (wallhopswitch) {
					MoveMouse(-wallhop_dx, 0);
				} else {
					MoveMouse(wallhop_dx, 0);
				}

				if (toggle_flick) {
					std::this_thread::sleep_for(std::chrono::milliseconds(75));
					if (wallhopswitch) {
						MoveMouse(-wallhop_dy, 0);
					} else {
						MoveMouse(wallhop_dy, 0);
					}
				}

				if (toggle_jump) {
					std::this_thread::sleep_for(std::chrono::milliseconds(16));
					ReleaseKey(0x39);
				}

				iswallhop = true;
		} else {
			iswallhop = false;
		}
	}

		if ((GetAsyncKeyState(vk_f6) & 0x8000) && macrotoggled && notbinding && section_toggles[7]) { // Walless LHJ (REQUIRES COM OFFSET AND .5 STUDS OF A FOOT ON A PLATFORM)
			if (!islhj) {
				HoldKey(0x20);
				std::this_thread::sleep_for(std::chrono::milliseconds(15));
				HoldKey(0x39);
				std::this_thread::sleep_for(std::chrono::milliseconds(20));
				SuspendOrResumeProcess(pfnSuspend, pfnResume, hProcess, true);
				std::this_thread::sleep_for(std::chrono::milliseconds(50));
				ReleaseKey(0x20);
				std::this_thread::sleep_for(std::chrono::milliseconds(500));
				HoldKey(scancode_shift);
				SuspendOrResumeProcess(pfnSuspend, pfnResume, hProcess, false);
				std::this_thread::sleep_for(std::chrono::milliseconds(50));
				ReleaseKey(scancode_shift);
				ReleaseKey(0x39);
				islhj = true;
			}
		} else {
			islhj = false;
		}

		if ((GetAsyncKeyState(vk_xkey) & 0x8000) && IsForegroundWindowProcess(rbxhwnd) && macrotoggled && notbinding && section_toggles[3]) { // Speedglitch
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

		if ((GetAsyncKeyState(vk_f8) & 0x8000) && macrotoggled && notbinding && section_toggles[4]) { // Unequip Speed
			if (!isunequipspeed) {

				PasteText("/");
				std::this_thread::sleep_for(std::chrono::milliseconds(50));
				PasteText(text);
				std::this_thread::sleep_for(std::chrono::milliseconds(50));
				HoldKey(0x1C);

				std::this_thread::sleep_for(std::chrono::milliseconds(35));
				ReleaseKey(0x1C);
				if (selected_dropdown == 2) {
				} else {
					std::this_thread::sleep_for(std::chrono::milliseconds(65));
				}

				if (selected_dropdown == 0) {
					std::this_thread::sleep_for(std::chrono::milliseconds(815));
				}
				if (selected_dropdown == 1) {
					std::this_thread::sleep_for(std::chrono::milliseconds(175));
				}
				HoldKey(speed_slot + 1);
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

		if ((GetAsyncKeyState(vk_xbutton1) & 0x8000) && macrotoggled && notbinding && section_toggles[2]) { // Helicopter High jump
			if (!HHJ) {

				if (autotoggle) { // Auto-Key-Timer
					HoldKey(0x39);
					std::this_thread::sleep_for(std::chrono::milliseconds(550));
					HoldKey(0x11);
					std::this_thread::sleep_for(std::chrono::milliseconds(68));
				}

				SuspendOrResumeProcess(pfnSuspend, pfnResume, hProcess, true);
				std::this_thread::sleep_for(std::chrono::milliseconds(500));

				if (autotoggle) { // Auto-Key-Timer
					ReleaseKey(0x39);
					ReleaseKey(0x11);
				}

				SuspendOrResumeProcess(pfnSuspend, pfnResume, hProcess, false);
				std::this_thread::sleep_for(std::chrono::milliseconds(8));
				HoldKey(scancode_shift);
				std::this_thread::sleep_for(std::chrono::milliseconds(17));
				isHHJ = true;
				std::this_thread::sleep_for(std::chrono::milliseconds(16));
				ReleaseKey(scancode_shift);
				std::this_thread::sleep_for(std::chrono::milliseconds(243));
				isHHJ = false;
				HHJ = true;
			}
		} else {
			HHJ = false;
		}

		if ((GetAsyncKeyState(vk_leftbracket) & 0x8000) && macrotoggled && notbinding && section_toggles[10]) { // Spamkey
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

		if ((GetAsyncKeyState(vk_clipkey) & 0x8000) && IsForegroundWindowProcess(rbxhwnd) && macrotoggled && notbinding && section_toggles[8]) { // Item Clip
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

		if ((GetAsyncKeyState(vk_wallkey) & 0x8000) && IsForegroundWindowProcess(rbxhwnd) && macrotoggled && notbinding && section_toggles[9]) { // WallWalk
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

		auto currentTime = std::chrono::steady_clock::now();
		if (std::chrono::duration_cast<std::chrono::seconds>(currentTime - lastProcessCheck).count() >= 1) {
			if (!IsWindow(rbxhwnd)) {
				hProcess = OpenProcess(PROCESS_ALL_ACCESS, FALSE, GetProcessIdByName());
				rbxhwnd = FindWindowByProcessHandle(hProcess);
			}
			lastProcessCheck = std::chrono::steady_clock::now();
		}

		if ((GetAsyncKeyState('W') & 0x8000) && IsForegroundWindowProcess(rbxhwnd)) { // Anti AFK (MUST STAY AT THE LOWEST PRIORITY!!!)
			// W is pressed, update the last press time to now
			lastPressTime = std::chrono::steady_clock::now();
		} else {
			if (IsWindow(rbxhwnd)) {
				// Check if 15 minutes have passed
				auto afktime = std::chrono::steady_clock::now();
				auto elapsedMinutes = std::chrono::duration_cast<std::chrono::minutes>(afktime - lastPressTime).count();
				if (elapsedMinutes >= 15) {
					HWND originalHwnd = GetForegroundWindow();
					INPUT input = {0};
					input.type = INPUT_MOUSE;
					input.mi.dwFlags = MOUSEEVENTF_LEFTDOWN;
					if (IsForegroundWindowProcess(rbxhwnd) ==
						0) { // Extremely long process to simulate going to roblox and typing .
						SetWindowPos(rbxhwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
						SetWindowPos(rbxhwnd, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
						Sleep(1000);
						SendInput(1, &input, sizeof(INPUT));
						input.mi.dwFlags = MOUSEEVENTF_LEFTUP;
						Sleep(50);
						SendInput(1, &input, sizeof(INPUT));
						Sleep(500);
						HoldKey(0x35);
						Sleep(20);
						ReleaseKey(0x35);
						Sleep(20);
						HoldKey(0x34);
						Sleep(20);
						ReleaseKey(0x34);
						Sleep(20);
						HoldKey(0x1C);
						Sleep(20);
						ReleaseKey(0x1C);
						Sleep(500);
						SetWindowPos(originalHwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
						SetWindowPos(originalHwnd, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
						Sleep(1000);
						SetForegroundWindow(originalHwnd);
						lastPressTime = std::chrono::steady_clock::now();
					}
					if (IsForegroundWindowProcess(rbxhwnd) == 1) {
						HoldKey(0x35);
						Sleep(20);
						ReleaseKey(0x35);
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
	}
	if (!IsForegroundWindowProcess(rbxhwnd)) { // Automatically turn off these 4 if you leave roblox window (so it isn't annoying)
		iswallwalkloop = false;
		isitemloop = false;
		isdesyncloop = false;
		isspeed = false;
	}
	std::this_thread::sleep_for(std::chrono::microseconds(50)); // Delay between checking for keys (dont touch pls)
	}
	// Cleanup
	SaveSettings("RMCSettings.json");
	guiThread.join();
	CleanupDeviceD3D();
	ImGui_ImplDX11_Shutdown();
	ImGui_ImplWin32_Shutdown();
	ImGui::DestroyContext();
	DestroyWindow(hwnd);

	return 0;
}
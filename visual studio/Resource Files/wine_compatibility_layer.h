#pragma once
#include <windows.h>
#include <string>
#include <vector>
#include <atomic>

struct KeyAction {
    WORD vk_code;
    WORD scan_code;
    bool needs_shift;
};

// Special reserved values for wheel usage
#define VK_MOUSE_WHEEL_UP 256
#define VK_MOUSE_WHEEL_DOWN 257

// --- Global state declarations (extern means "defined elsewhere") ---
extern std::string g_linuxHelperPath_Windows;

// Function Pointers
typedef LONG(WINAPI *PntSuspendProcess)(HANDLE);
typedef LONG(WINAPI *PntResumeProcess)(HANDLE);
extern PntSuspendProcess pfnSuspend;
extern PntResumeProcess pfnResume;

// --- Public API Functions ---
void InitLinuxCompatLayer();
void ShutdownLinuxCompatLayer();
bool IsKeyPressed(WORD vk_key);
void HoldKey(WORD scanCode);
void HoldKey(WORD scanCode, bool extended);
void ReleaseKey(WORD scanCode);
void ReleaseKey(WORD scanCode, bool extended);
void HoldKeyBinded(WORD Vk_key);
void ReleaseKeyBinded(WORD Vk_key);
void MoveMouse(int dx, int dy);
void SetLinuxBhopState(bool enable);
void SetDesyncState(bool enable);
void SetBhopDelay(int delay_ms);
void SetDesyncItem(int itemSlot);

// Process Logic
std::vector<DWORD> FindProcessIdsByName_Compat(const std::string &targetName, bool findAll);
std::vector<HANDLE> GetProcessHandles_Compat(const std::vector<DWORD> &pids, DWORD accessRights);
void SuspendOrResumeProcesses_Compat(const std::vector<DWORD> &pids,
				     const std::vector<HANDLE> &handles, bool suspend);

// Pointers to the wheel functions in your main file
bool IsWheelUp();
bool IsWheelDown();

// Structs for pasting logic

KeyAction CharToKeyAction_Compat(char c);
KeyAction CharToKeyAction_Global(char c);
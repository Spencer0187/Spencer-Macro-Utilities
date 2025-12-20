#pragma once
#include <windows.h>

// --- State Variables ---
extern HWND overlayHwnd;
extern HANDLE g_overlayFontHandle;

// --- Logic Functions ---
void UpdateLagswitchOverlay();
void CleanupOverlay();
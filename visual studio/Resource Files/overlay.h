#pragma once
// Remove windows.h dependency
#include <SDL3/SDL.h>

// --- State Variables ---
// We keep track of the SDL window and renderer instead of HWND
extern SDL_Window *g_overlayWindow;
extern SDL_Renderer *g_overlayRenderer;

// --- Logic Functions ---
void UpdateLagswitchOverlay();
void CleanupOverlay();
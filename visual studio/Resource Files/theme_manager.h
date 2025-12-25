#pragma once
#include "globals.h"
#include "imgui-files/imgui.h"
#include <vector>
#include <string>

namespace ThemeManager {
    // Initialize default themes (call this at startup)
    void Initialize();

    // Apply the current theme colors to ImGui Style
    void ApplyTheme();

    // Render the Theme Editor Menu
    // Pass the boolean pointer that controls the menu's visibility
    void RenderThemeMenu(bool* p_open);
}
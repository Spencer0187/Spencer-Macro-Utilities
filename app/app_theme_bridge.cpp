#include "app_theme_bridge.h"

#ifndef SMU_PORTABLE_GLOBALS
#define SMU_PORTABLE_GLOBALS
#endif
#include "Resource Files/theme_manager.h"

#include "imgui.h"

#include <array>
#include <filesystem>

namespace smu::app {
namespace {

std::filesystem::path FindLegacyFontPath()
{
    const std::array<std::filesystem::path, 5> candidates = {
        std::filesystem::path("visual studio/Resource Files/LSANS.TTF"),
        std::filesystem::path("../visual studio/Resource Files/LSANS.TTF"),
        std::filesystem::path("../../visual studio/Resource Files/LSANS.TTF"),
        std::filesystem::path("Resource Files/LSANS.TTF"),
        std::filesystem::path("LSANS.TTF"),
    };

    for (const auto& candidate : candidates) {
        if (std::filesystem::exists(candidate)) {
            return candidate;
        }
    }
    return {};
}

} // namespace

void InitializeSharedThemeSystem()
{
    ThemeManager::Initialize();
}

void ApplySharedTheme()
{
    ThemeManager::ApplyTheme();
}

void RenderSharedThemeEditor(bool* open)
{
    ThemeManager::RenderThemeMenu(open);
}

void SetupSharedFontsAndStyle(ImGuiIO& io)
{
    ImFontConfig cfg;
    cfg.OversampleH = 3;
    cfg.OversampleV = 1;
    cfg.PixelSnapH = false;

    const std::filesystem::path fontPath = FindLegacyFontPath();
    if (!fontPath.empty()) {
        io.Fonts->AddFontFromFileTTF(fontPath.string().c_str(), 20.0f, &cfg);
    } else {
        io.Fonts->AddFontDefault();
    }

    ImGui::StyleColorsDark();
}

} // namespace smu::app

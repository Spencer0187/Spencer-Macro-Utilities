#include "app_theme_bridge.h"

#include "app_assets.h"
#include "../platform/logging.h"

#ifndef SMU_PORTABLE_GLOBALS
#define SMU_PORTABLE_GLOBALS
#endif
#include "Resource Files/theme_manager.h"

#include "imgui.h"

#include <array>
#include <filesystem>

namespace smu::app {
namespace {

#if defined(__linux__)
std::filesystem::path FindLinuxFontPath()
{
    if (const char* appDir = std::getenv("APPDIR")) { // APPDIR env is set for AppImage runtimes
        std::filesystem::path internalPath = std::filesystem::path(appDir) / "usr/bin/fonts/LSANS.TTF";
        if (std::filesystem::exists(internalPath)) {
            return internalPath;
        }
    }

    if (auto runtimeAsset = FindRuntimeAsset("LSANS.TTF"); !runtimeAsset.empty()) {
        return runtimeAsset;
    }

#if defined(SMU_SOURCE_ROOT)
    const std::filesystem::path sourceTreeFont =
        std::filesystem::path(SMU_SOURCE_ROOT) / "visual studio" / "Resource Files" / "LSANS.TTF";
    if (std::filesystem::exists(sourceTreeFont)) {
        return sourceTreeFont;
    }
#endif
    return {};
}
#endif


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

#if defined(__linux__)
    const std::filesystem::path fontPath = FindLinuxFontPath();
#else
    const std::filesystem::path fontPath = FindLegacyFontPath();
#endif
    if (!fontPath.empty()) {
        if (!io.Fonts->AddFontFromFileTTF(fontPath.string().c_str(), 20.0f, &cfg)) {
            LogWarning("LSANS.TTF was found at " + fontPath.string() + " but ImGui could not load it; falling back to ImGui default font.");
            io.Fonts->AddFontDefault();
        }
    } else {
        LogWarning("LSANS.TTF font was not found; falling back to ImGui default font.");
        io.Fonts->AddFontDefault();
    }

    ImGui::StyleColorsDark();
}

} // namespace smu::app

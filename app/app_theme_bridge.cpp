#include "app_theme_bridge.h"

#include "app_assets.h"
#include "../platform/logging.h"

#include "theme_manager.h"

#include "imgui.h"

#include <array>
#include <filesystem>

#if defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#endif

namespace smu::app {
namespace {

#if defined(__linux__)
std::filesystem::path FindLinuxFontPath()
{
    if (const char* appDir = std::getenv("APPDIR")) { // APPDIR env is set for AppImage runtimes
        const std::filesystem::path appImageFont =
            std::filesystem::path(appDir) / "usr/bin/assets/LSANS.TTF";
        if (std::filesystem::exists(appImageFont)) {
            return appImageFont;
        }

        const std::filesystem::path legacyAppImageFont =
            std::filesystem::path(appDir) / "usr/bin/fonts/LSANS.TTF";
        if (std::filesystem::exists(legacyAppImageFont)) {
            return legacyAppImageFont;
        }
    }

    if (auto runtimeAsset = FindRuntimeAsset("LSANS.TTF"); !runtimeAsset.empty()) {
        return runtimeAsset;
    }

#if defined(SMU_SOURCE_ROOT)
    const std::filesystem::path sourceTreeFont =
        std::filesystem::path(SMU_SOURCE_ROOT) / "assets" / "LSANS.TTF";
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
        std::filesystem::path("assets/LSANS.TTF"),
        std::filesystem::path("../assets/LSANS.TTF"),
        std::filesystem::path("../../assets/LSANS.TTF"),
        std::filesystem::path("./assets/LSANS.TTF"),
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
#elif defined(_WIN32)
    HMODULE moduleHandle = GetModuleHandleW(nullptr);
    HRSRC fontResource = FindResourceW(moduleHandle, L"LSANS_TTF", RT_RCDATA);
    if (fontResource) {
        HGLOBAL loadedResource = LoadResource(moduleHandle, fontResource);
        if (loadedResource) {
            const DWORD resourceSize = SizeofResource(moduleHandle, fontResource);
            void* resourceData = LockResource(loadedResource);
            if (resourceData && resourceSize > 0) {
                cfg.FontDataOwnedByAtlas = false;
                if (!io.Fonts->AddFontFromMemoryTTF(resourceData, static_cast<int>(resourceSize), 20.0f, &cfg)) {
                    LogWarning("LSANS_TTF resource was found but ImGui could not load it; falling back to ImGui default font.");
                    io.Fonts->AddFontDefault();
                }
            } else {
                LogWarning("LSANS_TTF resource data was invalid; falling back to ImGui default font.");
                io.Fonts->AddFontDefault();
            }
        } else {
            LogWarning("LSANS_TTF resource could not be loaded; falling back to ImGui default font.");
            io.Fonts->AddFontDefault();
        }
    } else {
        LogWarning("LSANS_TTF resource was not found; falling back to ImGui default font.");
        io.Fonts->AddFontDefault();
    }
#else
    const std::filesystem::path fontPath = FindLegacyFontPath();
#endif

#if !defined(_WIN32)
    if (!fontPath.empty()) {
        if (!io.Fonts->AddFontFromFileTTF(fontPath.string().c_str(), 20.0f, &cfg)) {
            LogWarning("LSANS.TTF was found at " + fontPath.string() + " but ImGui could not load it; falling back to ImGui default font.");
            io.Fonts->AddFontDefault();
        }
    } else {
        LogWarning("LSANS.TTF font was not found; falling back to ImGui default font.");
        io.Fonts->AddFontDefault();
    }
#endif

    ImGui::StyleColorsDark();
}

} // namespace smu::app

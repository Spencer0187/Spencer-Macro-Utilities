#include "app_profile_bridge.h"

#include "../core/legacy_globals.h"
#include "profile_manager.h"
#include "../core/app_state.h"
#include "../platform/logging.h"

namespace smu::app {

void InitializeSharedProfiles()
{
    using namespace Globals;
    auto& state = smu::core::GetAppState();

    if (presskey_instances.empty()) {
        presskey_instances.emplace_back();
    }
    if (wallhop_instances.empty()) {
        wallhop_instances.emplace_back();
    }
    if (spamkey_instances.empty()) {
        spamkey_instances.emplace_back();
    }

    G_SETTINGS_FILEPATH = ResolveSettingsFilePath();
    LogInfo("Chosen settings path: " + G_SETTINGS_FILEPATH);
    if (!TryLoadLastActiveProfile(G_SETTINGS_FILEPATH) && G_CURRENTLY_LOADED_PROFILE_NAME.empty()) {
        G_CURRENTLY_LOADED_PROFILE_NAME = "Profile 1";
        LogWarning("Continuing with in-memory default settings because the settings file could not be loaded.");
    }

    state.screenWidth = screen_width;
    state.screenHeight = screen_height;
    state.rawWindowWidth = raw_window_width;
    state.rawWindowHeight = raw_window_height;
    state.windowPosX = WindowPosX;
    state.windowPosY = WindowPosY;
    state.windowOpacityPercent = windowOpacityPercent;
    state.alwaysOnTop = ontoptoggle;
}

void ShutdownSharedProfiles()
{
    using namespace Globals;
    auto& state = smu::core::GetAppState();

    if (G_SETTINGS_FILEPATH.empty()) {
        return;
    }

    screen_width = state.screenWidth;
    screen_height = state.screenHeight;
    raw_window_width = state.rawWindowWidth;
    raw_window_height = state.rawWindowHeight;
    WindowPosX = state.windowPosX;
    WindowPosY = state.windowPosY;

    PromoteDefaultProfileIfDirty(G_SETTINGS_FILEPATH);

    if (!G_CURRENTLY_LOADED_PROFILE_NAME.empty() && G_CURRENTLY_LOADED_PROFILE_NAME != "(default)") {
        SaveSettings(G_SETTINGS_FILEPATH, G_CURRENTLY_LOADED_PROFILE_NAME);
    }
}

void RenderSharedProfileManager()
{
    ProfileUI::DrawProfileManagerUI();
}

} // namespace smu::app

#include "app_profile_bridge.h"

#include "../core/legacy_globals.h"
#include "profile_manager.h"
#include "../core/app_state.h"
#include "../platform/logging.h"

#include <cstdlib>

namespace smu::app {

namespace {

constexpr const char kSettingsLoadFailedWarningId[] = "settings_load_failed";

bool IsUiParityProfileIsolationActive()
{
    const char* parityCase = std::getenv("SMU_UI_PARITY_CASE");
    return parityCase && parityCase[0] != '\0';
}

} // namespace

void InitializeSharedProfiles()
{
    std::lock_guard<std::recursive_mutex> persistenceLock(GetProfilePersistenceMutex());
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

    // Establish runtime values from the compile-time UI defaults before the
    // first profile snapshot is captured. This also covers a missing or
    // unreadable settings file.
    SyncRuntimeSettingsFromBuffers();

    // UI parity captures are read/write-isolated from user profile storage. They start from
    // compiled defaults and are populated by the named parity fixture in RenderAppUi.
    if (IsUiParityProfileIsolationActive()) {
        G_SETTINGS_FILEPATH.clear();
        G_CURRENTLY_LOADED_PROFILE_NAME = "(default)";
        CaptureDefaultProfileSnapshot();
    } else {
        G_SETTINGS_FILEPATH = ResolveSettingsFilePath();
        LogInfo("Chosen settings path: " + G_SETTINGS_FILEPATH);
        // Capture defaults without touching the user's file. Format detection and
        // recovery must happen before any startup write.
        CaptureDefaultProfileSnapshot();
        const bool loaded = TryLoadLastActiveProfile(G_SETTINGS_FILEPATH);
        if (loaded) {
            SaveDefaultProfile(G_SETTINGS_FILEPATH);
        } else if (G_CURRENTLY_LOADED_PROFILE_NAME.empty()) {
            G_CURRENTLY_LOADED_PROFILE_NAME = "Profile 1";
            LogWarning("Continuing with in-memory default settings because the settings file could not be loaded.",
                kSettingsLoadFailedWarningId, true);
        }
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

bool SaveSharedProfilesNow()
{
    std::lock_guard<std::recursive_mutex> persistenceLock(GetProfilePersistenceMutex());
    using namespace Globals;
    auto& state = smu::core::GetAppState();

    if (G_SETTINGS_FILEPATH.empty()) {
        return false;
    }

    screen_width = state.screenWidth;
    screen_height = state.screenHeight;
    raw_window_width = state.rawWindowWidth;
    raw_window_height = state.rawWindowHeight;
    WindowPosX = state.windowPosX;
    WindowPosY = state.windowPosY;

    if (G_CURRENTLY_LOADED_PROFILE_NAME == "(default)" &&
        PromoteDefaultProfileIfDirty(G_SETTINGS_FILEPATH).empty()) {
        return false;
    }

    if (!G_CURRENTLY_LOADED_PROFILE_NAME.empty() && G_CURRENTLY_LOADED_PROFILE_NAME != "(default)") {
        return SaveSettings(G_SETTINGS_FILEPATH, G_CURRENTLY_LOADED_PROFILE_NAME);
    }

    return false;
}

void ShutdownSharedProfiles()
{
    if (!IsUiParityProfileIsolationActive()) {
        SaveSharedProfilesNow();
    }
}

void RenderSharedProfileManager()
{
    ProfileUI::DrawProfileManagerUI();
}

} // namespace smu::app

#include "app_profile_bridge.h"

#ifndef SMU_PORTABLE_GLOBALS
#define SMU_PORTABLE_GLOBALS
#endif
#include "Resource Files/globals.h"
#include "Resource Files/profile_manager.h"
#include "../platform/logging.h"

namespace smu::app {

void InitializeSharedProfiles()
{
    using namespace Globals;

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
}

void ShutdownSharedProfiles()
{
    using namespace Globals;

    if (G_SETTINGS_FILEPATH.empty()) {
        return;
    }

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

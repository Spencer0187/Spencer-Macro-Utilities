#include "app_profile_bridge.h"

#ifndef SMU_PORTABLE_GLOBALS
#define SMU_PORTABLE_GLOBALS
#endif
#include "Resource Files/globals.h"
#include "Resource Files/profile_manager.h"

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
    TryLoadLastActiveProfile(G_SETTINGS_FILEPATH);
}

void RenderSharedProfileManager()
{
    ProfileUI::DrawProfileManagerUI();
}

} // namespace smu::app

#pragma once

#include "../platform/platform_capabilities.h"

#include <cstddef>
#include <functional>
#include <string>

namespace smu::app {

struct AppContext {
    smu::platform::PlatformCapabilities capabilities;
    bool inputBackendAvailable = false;
    bool processBackendAvailable = false;
    bool networkBackendAvailable = false;
    bool foregroundFallbackWarningShown = false;
    bool platformSetupRequired = false;
    bool platformSetupDismissed = false;
    bool platformAccessibilityPermissionGranted = false;
    bool platformScreenRecordingPermissionGranted = false;
    bool linuxInputSetupRequired = false;
    std::string inputBackendError;
    std::string processBackendError;
    std::string networkBackendError;
    std::string platformSetupTitle;
    std::string platformSetupSummary;
    std::string platformSetupDetails;
    std::string platformSetupActionMessage;
    std::string linuxInputPermissionSummary;
    std::string linuxInputPermissionDetails;
    std::string linuxInputInstallerPath;
    std::string linuxInputSudoCommand;
    std::string linuxInputSetupDocsPath;
    std::string linuxInputSetupActionMessage;
    std::size_t detectedProcessCount = 0;
    std::function<bool(bool)> setAlwaysOnTop;
    std::function<bool(float)> setWindowOpacityPercent;
    std::function<void(const char*)> openExternalUrl;
    std::function<void()> installLinuxPermissionsGraphical;
    std::function<void()> installLinuxPermissionsTerminal;
    std::function<void()> refreshLinuxInputPermissions;
    std::function<void()> requestAccessibilityPermission;
    std::function<void()> requestScreenRecordingPermission;
    std::function<void()> resetMacOSPermissionEntries;
    std::function<void()> refreshPlatformPermissions;
    std::function<void()> openPrivacySettings;
    std::function<bool()> restartApplication;
};

AppContext CreateAppContext();
bool IsForegroundDetectionFallbackActive(const AppContext& context);
bool ForegroundRestrictionAllows(const AppContext& context, bool disableOutsideRoblox, bool tabbedIntoRoblox);
void MaybeWarnForegroundDetectionFallback(AppContext& context, bool onLaunch = false);

} // namespace smu::app

#pragma once

#include "../platform/platform_capabilities.h"

#include <cstddef>
#include <string>

namespace smu::app {

struct AppContext {
    smu::platform::PlatformCapabilities capabilities;
    bool inputBackendAvailable = false;
    bool processBackendAvailable = false;
    bool networkBackendAvailable = false;
    bool foregroundFallbackWarningShown = false;
    std::string inputBackendError;
    std::string processBackendError;
    std::string networkBackendError;
    std::size_t detectedProcessCount = 0;
};

AppContext CreateAppContext();
bool IsForegroundDetectionFallbackActive(const AppContext& context);
bool ForegroundRestrictionAllows(const AppContext& context, bool disableOutsideRoblox, bool tabbedIntoRoblox);
void MaybeWarnForegroundDetectionFallback(AppContext& context);

} // namespace smu::app

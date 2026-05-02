#include "app_context.h"

#include "../platform/logging.h"

namespace smu::app {

AppContext CreateAppContext()
{
    AppContext context;
    context.capabilities = smu::platform::GetPlatformCapabilities();
    return context;
}

bool IsForegroundDetectionFallbackActive(const AppContext& context)
{
    return !context.capabilities.canDetectForegroundProcess;
}

bool ForegroundRestrictionAllows(const AppContext& context, bool disableOutsideRoblox, bool tabbedIntoRoblox)
{
    if (IsForegroundDetectionFallbackActive(context)) {
        return true;
    }
    return !disableOutsideRoblox || tabbedIntoRoblox;
}

void MaybeWarnForegroundDetectionFallback(AppContext& context)
{
    if (context.foregroundFallbackWarningShown || !IsForegroundDetectionFallbackActive(context)) {
        return;
    }

    context.foregroundFallbackWarningShown = true;
    LogWarning("Foreground-detection-dependent macros are in always-active mode because foreground Roblox window detection is unavailable on this display server.");
}

} // namespace smu::app

#include "platform_capabilities.h"

#include <cstdlib>

namespace smu::platform {

PlatformCapabilities GetPlatformCapabilities()
{
    PlatformCapabilities caps;

#if defined(_WIN32)
    caps.canInjectGlobalInput = true;
    caps.canReadGlobalInput = true;
    caps.canDetectForegroundProcess = true;
    caps.canSuspendProcesses = true;
    caps.canUseNetworkLagSwitch = true;
    caps.canShowGlobalOverlay = true;
    caps.displayServer = "windows";
#elif defined(__linux__)
    caps.displayServer = "unknown";

    const char* sessionType = std::getenv("XDG_SESSION_TYPE");
    const char* waylandDisplay = std::getenv("WAYLAND_DISPLAY");
    const char* x11Display = std::getenv("DISPLAY");

    if ((sessionType && std::string(sessionType) == "wayland") || waylandDisplay) {
        caps.displayServer = "wayland";
        caps.warnings.push_back("Generic foreground process detection is unsupported on Wayland.");
    } else if ((sessionType && std::string(sessionType) == "x11") || x11Display) {
        caps.displayServer = "x11";
        caps.canDetectForegroundProcess = true;
        caps.warnings.push_back("X11 foreground process detection requires _NET_ACTIVE_WINDOW and _NET_WM_PID support.");
    } else {
        caps.warnings.push_back("Unknown display server; foreground process detection is unsupported.");
    }

    caps.canSuspendProcesses = true;
    caps.canInjectGlobalInput = false;
    caps.canReadGlobalInput = false;
    caps.canUseNetworkLagSwitch = false;
    caps.canShowGlobalOverlay = false;
#else
    caps.warnings.push_back("Unsupported platform; no platform backends are available.");
#endif

    return caps;
}

} // namespace smu::platform

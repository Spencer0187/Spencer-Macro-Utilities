#include "platform_capabilities.h"

#include <cstdlib>

#if defined(__linux__)
#include "linux/display_server.h"
#include "linux/foreground_x11.h"
#endif

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
    const linux::DisplayServer displayServer = linux::DetectDisplayServer();
    caps.displayServer = linux::DisplayServerName(displayServer);

    if (displayServer == linux::DisplayServer::Wayland) {
        caps.warnings.push_back("Generic foreground process detection is unsupported on Wayland.");
    } else if (displayServer == linux::DisplayServer::X11) {
        std::string x11Error;
        if (linux::IsX11ForegroundDetectionAvailable(&x11Error)) {
            caps.canDetectForegroundProcess = true;
        } else {
            if (x11Error.empty()) {
                x11Error = "X11 foreground process detection is unavailable.";
            }
            caps.warnings.push_back(x11Error);
        }
    } else {
        caps.warnings.push_back("Unknown display server; foreground process detection is unsupported.");
    }

    caps.canSuspendProcesses = true;
    caps.canInjectGlobalInput = true;
    caps.canReadGlobalInput = true;
    caps.canUseNetworkLagSwitch = false;
    caps.canShowGlobalOverlay = false;
#else
    caps.warnings.push_back("Unsupported platform; no platform backends are available.");
#endif

    return caps;
}

} // namespace smu::platform

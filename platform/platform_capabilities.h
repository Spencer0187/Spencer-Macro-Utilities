#pragma once

#include <string>
#include <vector>

namespace smu::platform {

struct PlatformCapabilities {
    bool canInjectGlobalInput = false;
    bool canReadGlobalInput = false;
    bool canDetectForegroundProcess = false;
    bool canSuspendProcesses = false;
    bool canUseNetworkLagSwitch = false;
    bool canShowGlobalOverlay = false;
    std::string displayServer = "unknown";
    std::vector<std::string> warnings;
    std::vector<std::string> criticalErrors;
};

PlatformCapabilities GetPlatformCapabilities();

} // namespace smu::platform

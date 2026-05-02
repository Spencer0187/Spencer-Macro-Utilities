#pragma once

#include <cstddef>
#include <string>

namespace smu::platform::linux {

struct InputPermissionStatus {
    bool inputDirectoryAvailable = false;
    bool canReadInputEvents = false;
    bool canOpenUinput = false;
    std::size_t readableEventDeviceCount = 0;
    std::string inputEventsMessage;
    std::string uinputMessage;

    bool ready() const
    {
        return canReadInputEvents && canOpenUinput;
    }
};

bool CanReadInputEvents();
bool CanOpenUinput();
InputPermissionStatus GetInputPermissionStatus();

} // namespace smu::platform::linux

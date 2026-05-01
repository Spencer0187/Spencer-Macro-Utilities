#pragma once

#include <string>

namespace smu::platform::linux {

enum class DisplayServer {
    X11,
    Wayland,
    Unknown
};

DisplayServer DetectDisplayServer();
std::string DisplayServerName(DisplayServer server);

} // namespace smu::platform::linux

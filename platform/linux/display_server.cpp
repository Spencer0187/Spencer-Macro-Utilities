#include "display_server.h"

#include <algorithm>
#include <cctype>
#include <cstdlib>
#include <string>

namespace smu::platform::linux {
namespace {

std::string LowerEnv(const char* name)
{
    const char* value = std::getenv(name);
    if (!value) {
        return {};
    }

    std::string result(value);
    std::transform(result.begin(), result.end(), result.begin(), [](unsigned char ch) {
        return static_cast<char>(std::tolower(ch));
    });
    return result;
}

} // namespace

DisplayServer DetectDisplayServer()
{
    const std::string sessionType = LowerEnv("XDG_SESSION_TYPE");
    const bool hasWaylandDisplay = std::getenv("WAYLAND_DISPLAY") != nullptr;
    const bool hasX11Display = std::getenv("DISPLAY") != nullptr;

    if (sessionType == "wayland" || hasWaylandDisplay) {
        return DisplayServer::Wayland;
    }

    if (sessionType == "x11" || hasX11Display) {
        return DisplayServer::X11;
    }

    return DisplayServer::Unknown;
}

std::string DisplayServerName(DisplayServer server)
{
    switch (server) {
    case DisplayServer::X11:
        return "x11";
    case DisplayServer::Wayland:
        return "wayland";
    case DisplayServer::Unknown:
    default:
        return "unknown";
    }
}

} // namespace smu::platform::linux

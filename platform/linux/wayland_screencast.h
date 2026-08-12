#pragma once

#include "../platform_types.h"

#include <functional>
#include <optional>
#include <string>

namespace smu::platform::linux {

// The portal is deliberately a separate service from the input backend: a
// screen share is a user-approved privacy permission, not a prerequisite for
// using keyboard and mouse macros. The selected monitor becomes the coordinate
// space for Wayland pixel APIs while a session is active.
class WaylandScreenCast final {
public:
    static WaylandScreenCast& instance();

    bool isSupported() const;
    bool isActive() const;
    bool hasRemoteDesktopControl() const;
    std::string status() const;

    // Opens the desktop portal's monitor picker and begins a PipeWire capture.
    // This must be called from the application/UI thread, not from a Lua worker.
    bool start(std::string* errorMessage = nullptr);
    // Replaces any ScreenCast-only session with a combined RemoteDesktop +
    // ScreenCast session. The portal separately grants control of the pointer.
    bool startRemoteDesktop(std::string* errorMessage = nullptr);
    void stop();

    // Lua workers use this to request an in-app confirmation. The worker waits
    // until the user either accepts (and the first frame arrives) or declines.
    // The callbacks below must be invoked by the UI thread.
    bool requestActivationForScript(const std::function<bool()>& isCancelled,
        std::string* errorMessage = nullptr);
    bool hasPendingActivationRequest() const;
    void approveActivationRequest();
    void declineActivationRequest();

    bool requestRemoteDesktopActivationForScript(const std::function<bool()>& isCancelled,
        std::string* errorMessage = nullptr);
    bool hasPendingRemoteDesktopActivationRequest() const;
    void approveRemoteDesktopActivationRequest();
    void declineRemoteDesktopActivationRequest();

    // Coordinates are in the selected monitor's captured pixel space and are
    // mapped to the portal stream's logical coordinate space internally.
    bool movePointerAbsolute(int x, int y, std::string* errorMessage = nullptr);

    std::optional<ScreenBounds> selectedMonitorBounds() const;
    std::optional<PixelColor> sample(int x, int y, std::string* errorMessage = nullptr) const;

private:
    WaylandScreenCast();
    ~WaylandScreenCast();
    WaylandScreenCast(const WaylandScreenCast&) = delete;
    WaylandScreenCast& operator=(const WaylandScreenCast&) = delete;

    class Impl;
    Impl* impl_;
};

} // namespace smu::platform::linux

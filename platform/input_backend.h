#pragma once

#include "platform_types.h"

#include <memory>
#include <optional>
#include <string>

namespace smu::platform {

class InputBackend {
public:
    virtual ~InputBackend() = default;

    virtual bool init(std::string* errorMessage = nullptr) = 0;
    virtual void shutdown() = 0;

    virtual bool isKeyPressed(PlatformKeyCode key) const = 0;
    virtual void holdKey(PlatformKeyCode key, bool extended = false) = 0;
    virtual void releaseKey(PlatformKeyCode key, bool extended = false) = 0;
    virtual void pressKey(PlatformKeyCode key, int delayMs = 50) = 0;
    virtual void holdKeyChord(PlatformKeyCode combinedKey) = 0;
    virtual void releaseKeyChord(PlatformKeyCode combinedKey) = 0;
    virtual void moveMouse(int dx, int dy) = 0;
    virtual void moveMouseRaw(int dx, int dy)
    {
        moveMouse(dx, dy);
    }
    virtual bool moveMouseAbsolute(int x, int y, std::string* errorMessage = nullptr)
    {
        if (errorMessage) {
            *errorMessage = absolutePointerUnavailableReason();
        }
        (void)x;
        (void)y;
        return false;
    }
    virtual void mouseWheel(int delta) = 0;

    virtual std::optional<CursorPosition> getCursorPosition() const
    {
        return std::nullopt;
    }
    virtual std::optional<ScreenBounds> getScreenBounds() const
    {
        return std::nullopt;
    }

    virtual std::optional<ScreenBounds> getActiveMonitorBounds() const
    {
        return getScreenBounds();
    }

    virtual std::optional<int> getActiveMonitorRefreshRateHz() const
    {
        return std::nullopt;
    }

    virtual std::string absolutePointerUnavailableReason() const
    {
        return "cursor/screen position is not available on this platform/session";
    }

    virtual std::optional<PixelColor> getPixelColor(int x, int y, std::string* errorMessage = nullptr) const
    {
        if (errorMessage) {
            *errorMessage = "screen pixel color sampling is not available on this platform/session";
        }
        (void)x;
        (void)y;
        return std::nullopt;
    }

    virtual std::string screenReadUnavailableReason() const
    {
        return "screen pixel color sampling is not available on this platform/session";
    }

    virtual std::optional<PlatformKeyCode> getCurrentPressedKey() const = 0;
    virtual std::string formatKeyName(PlatformKeyCode key) const = 0;
};

std::shared_ptr<InputBackend> GetInputBackend();
void SetInputBackend(std::shared_ptr<InputBackend> backend);

} // namespace smu::platform

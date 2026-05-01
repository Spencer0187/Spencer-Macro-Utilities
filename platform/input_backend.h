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
    virtual void mouseWheel(int delta) = 0;

    virtual std::optional<PlatformKeyCode> getCurrentPressedKey() const = 0;
    virtual std::string formatKeyName(PlatformKeyCode key) const = 0;
};

std::shared_ptr<InputBackend> GetInputBackend();
void SetInputBackend(std::shared_ptr<InputBackend> backend);

} // namespace smu::platform

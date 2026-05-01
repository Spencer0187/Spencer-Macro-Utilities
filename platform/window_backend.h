#pragma once

#include "platform_types.h"

#include <memory>
#include <optional>
#include <string>

namespace smu::platform {

class WindowBackend {
public:
    virtual ~WindowBackend() = default;

    virtual bool init(std::string* errorMessage = nullptr) = 0;
    virtual void shutdown() = 0;
    virtual PlatformWindowHandle mainWindow() const = 0;
    virtual bool isForegroundWindow(PlatformWindowHandle window) const = 0;
    virtual std::optional<PlatformPid> foregroundProcess() const = 0;
};

std::shared_ptr<WindowBackend> GetWindowBackend();
void SetWindowBackend(std::shared_ptr<WindowBackend> backend);

} // namespace smu::platform

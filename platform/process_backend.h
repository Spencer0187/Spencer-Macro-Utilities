#pragma once

#include "platform_types.h"

#include <memory>
#include <optional>
#include <string>
#include <vector>

namespace smu::platform {

class ProcessBackend {
public:
    virtual ~ProcessBackend() = default;

    virtual bool init(std::string* errorMessage = nullptr) = 0;
    virtual void shutdown() = 0;

    virtual std::optional<PlatformPid> findProcess(const std::string& executableName) const = 0;
    virtual std::vector<PlatformPid> findAllProcesses(const std::string& executableName) const = 0;
    virtual std::optional<PlatformPid> findMainProcess(const std::string& executableName) const = 0;
    virtual bool suspend(PlatformPid pid) = 0;
    virtual bool resume(PlatformPid pid) = 0;
    virtual bool isForegroundProcess(PlatformPid pid) const = 0;
};

std::shared_ptr<ProcessBackend> GetProcessBackend();
void SetProcessBackend(std::shared_ptr<ProcessBackend> backend);

} // namespace smu::platform

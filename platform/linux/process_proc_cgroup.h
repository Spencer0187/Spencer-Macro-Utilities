#pragma once

#include "../process_backend.h"

#include <memory>

namespace smu::platform::linux {

class ProcCgroupProcessBackend final : public ProcessBackend {
public:
    bool init(std::string* errorMessage = nullptr) override;
    void shutdown() override;

    std::optional<PlatformPid> findProcess(const std::string& executableName) const override;
    std::vector<PlatformPid> findAllProcesses(const std::string& executableName) const override;
    std::optional<PlatformPid> findMainProcess(const std::string& executableName) const override;
    bool suspend(PlatformPid pid) override;
    bool resume(PlatformPid pid) override;
    bool isForegroundProcess(PlatformPid pid) const override;
};

std::shared_ptr<ProcessBackend> CreateProcCgroupProcessBackend();

} // namespace smu::platform::linux

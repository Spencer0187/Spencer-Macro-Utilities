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

// The process backend requests this only after an otherwise-valid cgroup v2
// freeze fails with EPERM/EACCES. The UI resolves the request after explaining
// and (optionally) starting the temporary privileged helper.
bool IsPrivilegedFreezeHelperAuthorizationPending();
void ResolvePrivilegedFreezeHelperAuthorization(bool approved);
void ResetPrivilegedFreezeHelperAuthorization();

std::shared_ptr<ProcessBackend> CreateProcCgroupProcessBackend();

} // namespace smu::platform::linux

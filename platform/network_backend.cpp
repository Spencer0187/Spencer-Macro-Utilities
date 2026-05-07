#include "network_backend.h"

#include <mutex>
#include <utility>

namespace smu::platform {
namespace {

class UnsupportedNetworkLagBackend final : public NetworkLagBackend {
public:
    bool init(std::string* errorMessage = nullptr) override
    {
        if (errorMessage) {
            *errorMessage = unsupportedReason();
        }
        return false;
    }

    void shutdown() override {}
    bool isAvailable() const override { return false; }
    bool isBlockingActive() const override { return effectiveConfig().currentlyBlocking; }
    bool isBaseBlockingActive() const override { return baseBlocking_; }
    void setBlockingActive(bool active) override { baseBlocking_ = active; }
    void setScriptBlockingActive(std::uintptr_t ownerToken, bool active) override
    {
        scriptBlockingOwner_ = ownerToken;
        scriptBlocking_ = active;
    }
    void setConfig(const LagSwitchConfig& config) override { baseConfig_ = config; }
    void setScriptConfigOverride(std::uintptr_t ownerToken, const LagSwitchConfig& config) override
    {
        scriptConfigOwner_ = ownerToken;
        scriptConfig_ = config;
        hasScriptConfig_ = true;
    }
    void clearScriptConfigOverride(std::uintptr_t ownerToken) override
    {
        if (hasScriptConfig_ && scriptConfigOwner_ == ownerToken) {
            hasScriptConfig_ = false;
            scriptConfig_ = {};
        }
    }
    void clearScriptState(std::uintptr_t ownerToken) override
    {
        clearScriptConfigOverride(ownerToken);
        if (scriptBlockingOwner_ == ownerToken) {
            scriptBlocking_ = false;
        }
    }
    LagSwitchConfig config() const override { return baseConfig_; }
    LagSwitchConfig effectiveConfig() const override
    {
        LagSwitchConfig effective = hasScriptConfig_ ? scriptConfig_ : baseConfig_;
        effective.currentlyBlocking = baseBlocking_ || scriptBlocking_;
        effective.enabled = effective.enabled || effective.currentlyBlocking;
        return effective;
    }
    void restartCapture() override {}
    std::string unsupportedReason() const override
    {
#if defined(__linux__)
        return "Linux lagswitch backend is not implemented yet.";
#else
        return "Network lagswitch backend is not implemented for this platform.";
#endif
    }

private:
    LagSwitchConfig baseConfig_;
    LagSwitchConfig scriptConfig_;
    std::uintptr_t scriptConfigOwner_ = 0;
    std::uintptr_t scriptBlockingOwner_ = 0;
    bool hasScriptConfig_ = false;
    bool baseBlocking_ = false;
    bool scriptBlocking_ = false;
};

std::mutex g_networkBackendMutex;
std::shared_ptr<NetworkLagBackend> g_networkBackend = std::make_shared<UnsupportedNetworkLagBackend>();
}

std::shared_ptr<NetworkLagBackend> GetNetworkLagBackend()
{
    std::lock_guard<std::mutex> lock(g_networkBackendMutex);
    return g_networkBackend;
}

void SetNetworkLagBackend(std::shared_ptr<NetworkLagBackend> backend)
{
    std::lock_guard<std::mutex> lock(g_networkBackendMutex);
    g_networkBackend = backend ? std::move(backend) : std::make_shared<UnsupportedNetworkLagBackend>();
}

} // namespace smu::platform

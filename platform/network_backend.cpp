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
    bool isBlockingActive() const override { return false; }
    void setBlockingActive(bool) override {}
    void setConfig(const LagSwitchConfig& config) override { config_ = config; }
    LagSwitchConfig config() const override { return config_; }
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
    LagSwitchConfig config_;
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

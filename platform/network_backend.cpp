#include "network_backend.h"

#include <mutex>
#include <utility>

namespace smu::platform {
namespace {

class NullNetworkBackend final : public NetworkBackend {
public:
    bool init(std::string* errorMessage = nullptr) override
    {
        if (errorMessage) {
            *errorMessage = "Network backend is not implemented for this platform yet.";
        }
        return false;
    }

    void shutdown() override {}
    bool isAvailable() const override { return false; }
    void setLagSwitchActive(bool) override {}
};

std::mutex g_networkBackendMutex;
std::shared_ptr<NetworkBackend> g_networkBackend = std::make_shared<NullNetworkBackend>();
}

std::shared_ptr<NetworkBackend> GetNetworkBackend()
{
    std::lock_guard<std::mutex> lock(g_networkBackendMutex);
    return g_networkBackend;
}

void SetNetworkBackend(std::shared_ptr<NetworkBackend> backend)
{
    std::lock_guard<std::mutex> lock(g_networkBackendMutex);
    g_networkBackend = backend ? std::move(backend) : std::make_shared<NullNetworkBackend>();
}

} // namespace smu::platform

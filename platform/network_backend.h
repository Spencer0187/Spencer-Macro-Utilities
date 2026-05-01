#pragma once

#include <memory>
#include <string>

namespace smu::platform {

class NetworkBackend {
public:
    virtual ~NetworkBackend() = default;

    virtual bool init(std::string* errorMessage = nullptr) = 0;
    virtual void shutdown() = 0;
    virtual bool isAvailable() const = 0;
    virtual void setLagSwitchActive(bool active) = 0;
};

std::shared_ptr<NetworkBackend> GetNetworkBackend();
void SetNetworkBackend(std::shared_ptr<NetworkBackend> backend);

} // namespace smu::platform

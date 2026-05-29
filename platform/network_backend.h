#pragma once

#include <cstdint>
#include <memory>
#include <string>
#include <vector>

namespace smu::platform {

enum class LagSwitchTargetMode {
    Roblox,
    All,
    Custom
};

struct LagSwitchConfig {
    bool enabled = false;
    bool currentlyBlocking = false;
    bool inboundHardBlock = true;
    bool outboundHardBlock = true;
    bool fakeLagEnabled = false;
    bool inboundFakeLag = true;
    bool outboundFakeLag = true;
    int fakeLagDelayMs = 0;
    bool targetRobloxOnly = true;
    bool useUdp = true;
    bool useTcp = false;
    bool preventDisconnect = true;
    bool autoUnblock = false;
    float maxDurationSeconds = 9.0f;
    int unblockDurationMs = 50;
    LagSwitchTargetMode targetMode = LagSwitchTargetMode::Roblox;
    std::vector<std::string> remoteIps;
    std::vector<int> remotePorts;
    bool includeRobloxDynamicIps = false;
};

class NetworkLagBackend {
public:
    virtual ~NetworkLagBackend() = default;

    virtual bool init(std::string* errorMessage = nullptr) = 0;
    virtual void shutdown() = 0;
    virtual bool isAvailable() const = 0;
    virtual bool isBlockingActive() const = 0;
    virtual bool isBaseBlockingActive() const = 0;
    virtual void setBlockingActive(bool active) = 0;
    virtual void setScriptBlockingActive(std::uintptr_t ownerToken, bool active) = 0;
    virtual void setConfig(const LagSwitchConfig& config) = 0;
    virtual void setScriptConfigOverride(std::uintptr_t ownerToken, const LagSwitchConfig& config) = 0;
    virtual void clearScriptConfigOverride(std::uintptr_t ownerToken) = 0;
    virtual void clearScriptState(std::uintptr_t ownerToken) = 0;
    virtual LagSwitchConfig config() const = 0;
    virtual LagSwitchConfig effectiveConfig() const = 0;
    virtual void restartCapture() = 0;
    virtual std::string unsupportedReason() const = 0;
};

std::shared_ptr<NetworkLagBackend> GetNetworkLagBackend();
void SetNetworkLagBackend(std::shared_ptr<NetworkLagBackend> backend);

std::shared_ptr<NetworkLagBackend> CreateGoNetworkLagBackend();

} // namespace smu::platform

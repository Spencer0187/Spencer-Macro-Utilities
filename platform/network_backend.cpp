#include "network_backend.h"

#include <mutex>
#include <utility>
#include <string>
#include <memory>

#if defined(__linux__)
#include <sys/socket.h>
#include <sys/un.h>
#include <sys/time.h>
#include <unistd.h>
#include <errno.h>
#include <cstdio>
#include <cstring>
#include <cstddef>
#endif

namespace smu::platform {

std::mutex g_networkBackendMutex;
static std::shared_ptr<NetworkLagBackend> g_networkBackend;

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

    bool isBlockingActive() const override
    {
        return effectiveConfig().currentlyBlocking;
    }

    bool isBaseBlockingActive() const override { return baseBlocking_; }

    void setBlockingActive(bool active) override { baseBlocking_ = active; }

    void setScriptBlockingActive(std::uintptr_t ownerToken, bool active) override
    {
        scriptBlockingOwner_ = ownerToken;
        scriptBlocking_ = active;
    }

    void setConfig(const LagSwitchConfig& config) override
    {
        baseConfig_ = config;
    }

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
        LagSwitchConfig cfg = hasScriptConfig_ ? scriptConfig_ : baseConfig_;
        cfg.currentlyBlocking = baseBlocking_ || scriptBlocking_;
        cfg.enabled = cfg.enabled || cfg.currentlyBlocking;
        return cfg;
    }

    void restartCapture() override {}

    std::string unsupportedReason() const override
    {
#if defined(__linux__)
        return "Linux backend unavailable";
#elif defined(__APPLE__)
        return "macOS network lagswitch backend is unavailable.";
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

#if defined(__linux__)

static int unixConnect(const char* path)
{
    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (fd < 0) return -1;

    struct timeval tv{};
    tv.tv_sec  = 2;
    tv.tv_usec = 0;
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
    setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof(tv));

    struct sockaddr_un addr{};
    addr.sun_family = AF_UNIX;
    std::snprintf(addr.sun_path, sizeof(addr.sun_path), "%s", path);

    if (connect(fd, reinterpret_cast<sockaddr*>(&addr), SUN_LEN(&addr)) != 0) {
        close(fd);
        return -1;
    }

    return fd;
}

static bool sendAll(int fd, const char* data, size_t len)
{
    while (len > 0) {
        ssize_t sent = send(fd, data, len, MSG_NOSIGNAL);
        if (sent <= 0) return false;
        data += sent;
        len  -= static_cast<size_t>(sent);
    }
    return true;
}

class GoNetworkLagBackend final : public NetworkLagBackend {
public:
    bool init(std::string* errorMessage = nullptr) override
    {
        if (available_) return true;

        socketPath_ = "/tmp/nethelper.sock";

        for (int i = 0; i < 30; ++i) {
            if (pingDaemon()) {
                available_ = true;
                fprintf(stderr, "[netbackend] connected to Go daemon at %s\n",
                        socketPath_.c_str());
                return true;
            }
            usleep(100 * 1000);
        }

        available_ = false;
        if (errorMessage) {
            *errorMessage = "Go daemon not reachable at " + socketPath_ +
                            " after 30 retries";
        }
        return false;
    }

    void shutdown() override { sendCmd("reset"); }

    bool isAvailable() const override { return available_; }

    bool isBlockingActive() const override
    {
        return effectiveConfig().currentlyBlocking;
    }

    bool isBaseBlockingActive() const override { return baseBlocking_; }

    void setBlockingActive(bool active) override
    {
        baseBlocking_ = active;
        apply();
    }

    void setScriptBlockingActive(std::uintptr_t ownerToken, bool active) override
    {
        scriptBlockingOwner_ = ownerToken;
        scriptBlocking_      = active;
        apply();
    }

    void setConfig(const LagSwitchConfig& config) override
    {
        baseConfig_ = config;
        apply();
    }

    void setScriptConfigOverride(std::uintptr_t ownerToken,
                                 const LagSwitchConfig& config) override
    {
        scriptConfigOwner_ = ownerToken;
        scriptConfig_      = config;
        hasScriptConfig_   = true;
        apply();
    }

    void clearScriptConfigOverride(std::uintptr_t ownerToken) override
    {
        if (hasScriptConfig_ && scriptConfigOwner_ == ownerToken) {
            hasScriptConfig_ = false;
            scriptConfig_    = {};
            apply();
        }
    }

    void clearScriptState(std::uintptr_t ownerToken) override
    {
        clearScriptConfigOverride(ownerToken);
        if (scriptBlockingOwner_ == ownerToken) {
            scriptBlocking_ = false;
            apply();
        }
    }

    LagSwitchConfig config() const override { return baseConfig_; }

    LagSwitchConfig effectiveConfig() const override
    {
        LagSwitchConfig cfg   = hasScriptConfig_ ? scriptConfig_ : baseConfig_;
        cfg.currentlyBlocking = baseBlocking_ || scriptBlocking_;
        cfg.enabled           = cfg.enabled || cfg.currentlyBlocking;
        return cfg;
    }

    void restartCapture() override { sendCmd("reset"); }

    std::string unsupportedReason() const override
    {
        return "Go backend active (Linux)";
    }

private:
    bool pingDaemon() const
    {
        int fd = unixConnect(socketPath_.c_str());
        if (fd < 0) return false;

        const char msg[] = "ping";
        if (!sendAll(fd, msg, strlen(msg))) {
            close(fd);
            return false;
        }

        char buf[64];
        ssize_t n = recv(fd, buf, sizeof(buf) - 1, 0);
        close(fd);

        if (n <= 0) return false;
        buf[n] = '\0';
        return std::string(buf).find("pong") != std::string::npos;
    }

    void apply()
    {
        if (!available_) return;

        LagSwitchConfig cfg = effectiveConfig();

        std::string newCmd;
        if (!cfg.currentlyBlocking) {
            newCmd = "reset";
        } else if (cfg.inboundHardBlock || cfg.outboundHardBlock) {
            newCmd = "block"
                " in="  + std::to_string(cfg.inboundHardBlock  ? 1 : 0) +
                " out=" + std::to_string(cfg.outboundHardBlock ? 1 : 0) +
                " udp=" + std::to_string(cfg.useUdp            ? 1 : 0) +
                " tcp=" + std::to_string(cfg.useTcp            ? 1 : 0);
        } else if (cfg.fakeLagEnabled) {
            newCmd = "lag"
                " delay=" + std::to_string(cfg.fakeLagDelayMs) +
                " in="    + std::to_string(cfg.inboundFakeLag  ? 1 : 0) +
                " out="   + std::to_string(cfg.outboundFakeLag ? 1 : 0) +
                " udp="   + std::to_string(cfg.useUdp          ? 1 : 0) +
                " tcp="   + std::to_string(cfg.useTcp          ? 1 : 0);
        } else {
            newCmd = "reset";
        }

        if (newCmd == lastCmd_) return;
        lastCmd_ = newCmd;
        sendCmd(newCmd);
    }

    void sendCmd(const std::string& msg) const
    {
        int fd = unixConnect(socketPath_.c_str());
        if (fd < 0) return;
        sendAll(fd, msg.c_str(), msg.size());
        close(fd);
    }

    std::string socketPath_;
    bool available_ = false;
    std::string lastCmd_;

    LagSwitchConfig baseConfig_;
    LagSwitchConfig scriptConfig_;

    std::uintptr_t scriptConfigOwner_   = 0;
    std::uintptr_t scriptBlockingOwner_ = 0;

    bool hasScriptConfig_ = false;
    bool baseBlocking_    = false;
    bool scriptBlocking_  = false;
};

std::shared_ptr<NetworkLagBackend> CreateGoNetworkLagBackend()
{
    return std::make_shared<GoNetworkLagBackend>();
}

#endif // __linux__

static struct BackendInit {
    BackendInit()
    {
#if defined(__linux__)
        auto backend = std::make_shared<GoNetworkLagBackend>();
        std::string err;
        if (backend->init(&err)) {
            g_networkBackend = backend;
            return;
        }
        fprintf(stderr, "[netbackend] GoNetworkLagBackend init failed: %s\n",
                err.c_str());
#endif
        g_networkBackend = std::make_shared<UnsupportedNetworkLagBackend>();
    }
} g_backendInit;

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

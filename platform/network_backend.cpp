#include "network_backend.h"

#include <algorithm>
#include <condition_variable>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <mutex>
#include <thread>
#include <utility>
#include <string>
#include <memory>
#include <sstream>

#if defined(__linux__)
#include <arpa/inet.h>
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
        return "macOS lagswitch requires a Developer ID-signed, Apple-entitled "
               "Network Extension and is unavailable in this build.";
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

static std::string nethelperSocketPath()
{
    return "/tmp/smu-nethelper-" + std::to_string(getuid()) + ".sock";
}

static bool isRootPeer(int fd)
{
    struct {
        pid_t pid;
        uid_t uid;
        gid_t gid;
    } credentials{};
    socklen_t credentialsLength = sizeof(credentials);
    return getsockopt(
               fd,
               SOL_SOCKET,
               SO_PEERCRED,
               &credentials,
               &credentialsLength) == 0 &&
           credentialsLength == sizeof(credentials) &&
           credentials.uid == 0;
}

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
    if (strlen(path) >= sizeof(addr.sun_path)) {
        close(fd);
        return -1;
    }
    std::snprintf(addr.sun_path, sizeof(addr.sun_path), "%s", path);

    if (connect(fd, reinterpret_cast<sockaddr*>(&addr), SUN_LEN(&addr)) != 0) {
        close(fd);
        return -1;
    }
    if (!isRootPeer(fd)) {
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

static bool receiveLine(int fd, std::string* response)
{
    response->clear();
    char buffer[256];
    while (response->size() < 1024) {
        const ssize_t received = recv(fd, buffer, sizeof(buffer), 0);
        if (received < 0 && errno == EINTR) {
            continue;
        }
        if (received <= 0) {
            break;
        }
        response->append(buffer, static_cast<std::size_t>(received));
        const std::size_t newline = response->find('\n');
        if (newline != std::string::npos) {
            response->resize(newline);
            return true;
        }
    }
    return !response->empty() && response->size() <= 1024;
}

static bool validIPv4(const std::string& value)
{
    in_addr address{};
    return inet_pton(AF_INET, value.c_str(), &address) == 1;
}

static bool extractLoggedIPv4(const std::string& line,
                              const std::string& marker,
                              std::string* address)
{
    const std::size_t begin = line.find(marker);
    if (begin == std::string::npos) {
        return false;
    }
    const std::size_t valueBegin = begin + marker.size();
    const std::size_t valueEnd = line.find_first_of(",: \t|", valueBegin);
    const std::string value = line.substr(valueBegin, valueEnd - valueBegin);
    if (!validIPv4(value)) {
        return false;
    }
    *address = value;
    return true;
}

// Sober writes the same Roblox player log format used by the Windows client.
// A UDMUX endpoint is commonly in 128.116.0.0/16, but the companion RCC
// endpoint is not.  Select the newest player log and retain its final pair so
// a prior server in the same log cannot leak into a later session's rules.
static std::vector<std::string> discoverSoberServerIPs()
{
    namespace fs = std::filesystem;

    const char* home = std::getenv("HOME");
    if (!home || home[0] == '\0') {
        return {};
    }

    std::vector<fs::path> logFolders;
    if (const char* xdgData = std::getenv("XDG_DATA_HOME"); xdgData && xdgData[0] != '\0') {
        logFolders.emplace_back(fs::path(xdgData) / "sober" / "appData" / "logs");
    }
    const fs::path homePath(home);
    logFolders.emplace_back(homePath / ".local" / "share" / "sober" / "appData" / "logs");
    logFolders.emplace_back(homePath / ".var" / "app" / "org.vinegarhq.Sober" / "data" / "sober" / "appData" / "logs");

    fs::path newest;
    fs::file_time_type newestTime{};
    std::error_code error;
    for (const fs::path& folder : logFolders) {
        if (!fs::is_directory(folder, error) || error) {
            error.clear();
            continue;
        }
        for (const fs::directory_entry& entry : fs::directory_iterator(folder, error)) {
            if (error) break;
            if (!entry.is_regular_file(error) || error || entry.path().extension() != ".log") {
                error.clear();
                continue;
            }
            const fs::file_time_type modified = entry.last_write_time(error);
            if (!error && (newest.empty() || modified > newestTime)) {
                newest = entry.path();
                newestTime = modified;
            }
            error.clear();
        }
        error.clear();
    }
    if (newest.empty()) {
        return {};
    }

    std::ifstream log(newest);
    std::string line;
    std::string udmux;
    std::string rcc;
    while (std::getline(log, line)) {
        std::string candidate;
        if (extractLoggedIPv4(line, "UDMUX Address = ", &candidate) ||
            extractLoggedIPv4(line, "UDMUX server ", &candidate)) {
            udmux = std::move(candidate);
        }
        if (extractLoggedIPv4(line, "RCC Server Address = ", &candidate) ||
            extractLoggedIPv4(line, "RCC server ", &candidate)) {
            rcc = std::move(candidate);
        }
        if (extractLoggedIPv4(line, "Connecting to ", &candidate) &&
            line.find("UDMUX") == std::string::npos) {
            rcc = std::move(candidate);
        }
    }

    std::vector<std::string> result;
    if (!udmux.empty()) result.push_back(std::move(udmux));
    if (!rcc.empty() && (result.empty() || result.front() != rcc)) result.push_back(std::move(rcc));
    return result;
}

static std::string joinStrings(const std::vector<std::string>& values)
{
    if (values.empty()) {
        return "-";
    }
    std::ostringstream result;
    for (std::size_t index = 0; index < values.size(); ++index) {
        if (index != 0) {
            result << ',';
        }
        result << values[index];
    }
    return result.str();
}

static std::string joinPorts(const std::vector<int>& values)
{
    if (values.empty()) {
        return "-";
    }
    std::ostringstream result;
    for (std::size_t index = 0; index < values.size(); ++index) {
        if (index != 0) {
            result << ',';
        }
        result << values[index];
    }
    return result.str();
}

class GoNetworkLagBackend final : public NetworkLagBackend {
public:
    ~GoNetworkLagBackend() override
    {
        stopDynamicTargetRefresh();
    }

    bool init(std::string* errorMessage = nullptr) override
    {
        std::lock_guard<std::mutex> lock(stateMutex_);
        if (errorMessage) {
            errorMessage->clear();
        }
        if (available_) return true;

        socketPath_ = nethelperSocketPath();

        if (pingDaemon()) {
            available_ = true;
            lastError_.clear();
            startDynamicTargetRefresh();
            fprintf(stderr, "[netbackend] connected to Go daemon at %s\n",
                    socketPath_.c_str());
            return true;
        }

        available_ = false;
        lastError_ =
            "Linux lagswitch helper is not running. Enable lagswitch to start it with elevated permissions.";
        return false;
    }

    void shutdown() override
    {
        stopDynamicTargetRefresh();
        std::lock_guard<std::mutex> lock(stateMutex_);
        std::string response;
        sendCmd("shutdown", &response);
        appliedBlocking_ = false;
        available_ = false;
        lastCmd_.clear();
    }

    bool isAvailable() const override
    {
        std::lock_guard<std::mutex> lock(stateMutex_);
        return available_;
    }

    bool isBlockingActive() const override
    {
        std::lock_guard<std::mutex> lock(stateMutex_);
        return appliedBlocking_;
    }

    bool isBaseBlockingActive() const override
    {
        std::lock_guard<std::mutex> lock(stateMutex_);
        return baseBlocking_;
    }

    void setBlockingActive(bool active) override
    {
        std::lock_guard<std::mutex> lock(stateMutex_);
        baseBlocking_ = active;
        applyLocked();
    }

    void setScriptBlockingActive(std::uintptr_t ownerToken, bool active) override
    {
        std::lock_guard<std::mutex> lock(stateMutex_);
        scriptBlockingOwner_ = ownerToken;
        scriptBlocking_      = active;
        applyLocked();
    }

    void setConfig(const LagSwitchConfig& config) override
    {
        std::lock_guard<std::mutex> lock(stateMutex_);
        baseConfig_ = config;
        applyLocked();
    }

    void setScriptConfigOverride(std::uintptr_t ownerToken,
                                 const LagSwitchConfig& config) override
    {
        std::lock_guard<std::mutex> lock(stateMutex_);
        scriptConfigOwner_ = ownerToken;
        scriptConfig_      = config;
        hasScriptConfig_   = true;
        applyLocked();
    }

    void clearScriptConfigOverride(std::uintptr_t ownerToken) override
    {
        std::lock_guard<std::mutex> lock(stateMutex_);
        if (hasScriptConfig_ && scriptConfigOwner_ == ownerToken) {
            hasScriptConfig_ = false;
            scriptConfig_    = {};
            applyLocked();
        }
    }

    void clearScriptState(std::uintptr_t ownerToken) override
    {
        std::lock_guard<std::mutex> lock(stateMutex_);
        bool changed = false;
        if (hasScriptConfig_ && scriptConfigOwner_ == ownerToken) {
            hasScriptConfig_ = false;
            scriptConfig_ = {};
            changed = true;
        }
        if (scriptBlockingOwner_ == ownerToken) {
            scriptBlocking_ = false;
            changed = true;
        }
        if (changed) {
            applyLocked();
        }
    }

    LagSwitchConfig config() const override
    {
        std::lock_guard<std::mutex> lock(stateMutex_);
        return baseConfig_;
    }

    LagSwitchConfig effectiveConfig() const override
    {
        std::lock_guard<std::mutex> lock(stateMutex_);
        return effectiveConfigLocked();
    }

    void restartCapture() override
    {
        std::lock_guard<std::mutex> lock(stateMutex_);
        lastCmd_.clear();
        applyLocked();
    }

    std::string unsupportedReason() const override
    {
        std::lock_guard<std::mutex> lock(stateMutex_);
        if (!lastError_.empty()) {
            return lastError_;
        }
        return available_ ? "Linux lagswitch helper is running."
                          : "Linux lagswitch helper is unavailable.";
    }

private:
    LagSwitchConfig effectiveConfigLocked() const
    {
        LagSwitchConfig cfg   = hasScriptConfig_ ? scriptConfig_ : baseConfig_;
        cfg.currentlyBlocking = baseBlocking_ || scriptBlocking_;
        cfg.enabled           = cfg.enabled || cfg.currentlyBlocking;
        return cfg;
    }

    bool pingDaemon() const
    {
        std::string response;
        return sendCmd("ping", &response) && response == "PONG";
    }

    bool targetCommand(const LagSwitchConfig& cfg, std::string* command, std::string* error) const
    {
        if (!cfg.inboundHardBlock && !cfg.outboundHardBlock) {
            *error = "Linux hard blocking requires an inbound or outbound direction.";
            return false;
        }
        if (!cfg.useUdp && !cfg.useTcp) {
            *error = "Linux hard blocking requires UDP, TCP, or both.";
            return false;
        }

        std::string mode;
        switch (cfg.targetMode) {
        case LagSwitchTargetMode::All:
            mode = "all";
            break;
        case LagSwitchTargetMode::Roblox:
            mode = "roblox";
            break;
        case LagSwitchTargetMode::Custom:
            mode = "custom";
            break;
        default:
            *error = "Linux lagswitch received an unsupported target mode.";
            return false;
        }

        std::vector<std::string> remoteIps;
        std::vector<int> remotePorts;
        if (cfg.targetMode == LagSwitchTargetMode::Roblox) {
            remoteIps = discoverSoberServerIPs();
        } else if (cfg.targetMode == LagSwitchTargetMode::Custom) {
            if (cfg.remoteIps.size() > 64 || cfg.remotePorts.size() > 64) {
                *error = "Linux custom targeting supports up to 64 IPs and 64 ports.";
                return false;
            }
            for (const std::string& ip : cfg.remoteIps) {
                if (!validIPv4(ip)) {
                    *error = "Linux custom targeting received an invalid IPv4 address.";
                    return false;
                }
                remoteIps.push_back(ip);
            }
            if (cfg.includeRobloxDynamicIps) {
                const std::vector<std::string> discovered = discoverSoberServerIPs();
                remoteIps.insert(remoteIps.end(), discovered.begin(), discovered.end());
            }
            for (int port : cfg.remotePorts) {
                if (port < 1 || port > 65535) {
                    *error = "Linux custom targeting received an invalid remote port.";
                    return false;
                }
                remotePorts.push_back(port);
            }
            if (remoteIps.empty() && remotePorts.empty()) {
                *error = "Linux custom targeting requires an IP or port.";
                return false;
            }
        }

        std::sort(remoteIps.begin(), remoteIps.end());
        remoteIps.erase(std::unique(remoteIps.begin(), remoteIps.end()), remoteIps.end());
        if (remoteIps.size() > 64) {
            *error = "Linux lagswitch supports up to 64 remote IPs.";
            return false;
        }

        *command = "block"
            " in=" + std::to_string(cfg.inboundHardBlock ? 1 : 0) +
            " out=" + std::to_string(cfg.outboundHardBlock ? 1 : 0) +
            " udp=" + std::to_string(cfg.useUdp ? 1 : 0) +
            " tcp=" + std::to_string(cfg.useTcp ? 1 : 0) +
            " mode=" + mode +
            " ips=" + joinStrings(remoteIps) +
            " ports=" + joinPorts(remotePorts);
        return true;
    }

    void disableWithError(const std::string& message)
    {
        std::string response;
        const bool reset = sendCmd("reset", &response) && response == "OK";
        appliedBlocking_ = false;
        lastCmd_ = reset ? "reset" : "";
        if (!reset) {
            available_ = false;
            lastError_ = message + " The helper could not confirm firewall cleanup.";
            return;
        }
        lastError_ = message;
    }

    void applyLocked()
    {
        if (!available_) return;

        LagSwitchConfig cfg = effectiveConfigLocked();

        std::string newCmd;
        if (!cfg.currentlyBlocking) {
            newCmd = "reset";
        } else if (cfg.fakeLagEnabled &&
                   (cfg.inboundFakeLag || cfg.outboundFakeLag)) {
            LagSwitchConfig lagConfig = cfg;
            lagConfig.inboundHardBlock = cfg.inboundFakeLag;
            lagConfig.outboundHardBlock = cfg.outboundFakeLag;
            std::string targetError;
            std::string blockCommand;
            if (!targetCommand(lagConfig, &blockCommand, &targetError)) {
                disableWithError(targetError);
                return;
            }
            newCmd = "lag delay=" + std::to_string(std::clamp(cfg.fakeLagDelayMs, 1, 5000)) +
                blockCommand.substr(std::string("block").size());
        } else if (cfg.inboundHardBlock || cfg.outboundHardBlock) {
            std::string targetError;
            if (!targetCommand(cfg, &newCmd, &targetError)) {
                disableWithError(targetError);
                return;
            }
        } else {
            newCmd = "reset";
        }

        if (newCmd == lastCmd_) {
            if (newCmd == "reset") {
                appliedBlocking_ = false;
                lastError_.clear();
            }
            return;
        }
        std::string response;
        if (!sendCmd(newCmd, &response)) {
            available_ = false;
            appliedBlocking_ = false;
            lastCmd_.clear();
            lastError_ = "Lost contact with the Linux lagswitch helper.";
            return;
        }
        if (response != "OK") {
            appliedBlocking_ = false;
            lastCmd_.clear();
            lastError_ = response.rfind("ERR ", 0) == 0
                ? response.substr(4)
                : "Linux lagswitch helper returned an invalid response.";
            return;
        }

        lastCmd_ = newCmd;
        appliedBlocking_ = newCmd.rfind("block ", 0) == 0 || newCmd.rfind("lag ", 0) == 0;
        lastError_.clear();
    }

    bool needsDynamicTargetRefreshLocked() const
    {
        const LagSwitchConfig cfg = effectiveConfigLocked();
        return available_ && cfg.currentlyBlocking &&
            (cfg.targetMode == LagSwitchTargetMode::Roblox ||
             (cfg.targetMode == LagSwitchTargetMode::Custom && cfg.includeRobloxDynamicIps));
    }

    void startDynamicTargetRefresh()
    {
        if (dynamicTargetThread_.joinable()) {
            return;
        }
        {
            std::lock_guard<std::mutex> lock(dynamicTargetMutex_);
            stopDynamicTargetThread_ = false;
        }
        dynamicTargetThread_ = std::thread([this] {
            std::unique_lock<std::mutex> waitLock(dynamicTargetMutex_);
            while (!dynamicTargetCv_.wait_for(waitLock, std::chrono::seconds(1), [this] {
                return stopDynamicTargetThread_;
            })) {
                waitLock.unlock();
                {
                    std::lock_guard<std::mutex> stateLock(stateMutex_);
                    if (needsDynamicTargetRefreshLocked()) {
                        applyLocked();
                    }
                }
                waitLock.lock();
            }
        });
    }

    void stopDynamicTargetRefresh()
    {
        {
            std::lock_guard<std::mutex> lock(dynamicTargetMutex_);
            stopDynamicTargetThread_ = true;
        }
        dynamicTargetCv_.notify_all();
        if (dynamicTargetThread_.joinable()) {
            dynamicTargetThread_.join();
        }
    }

    bool sendCmd(const std::string& msg, std::string* response) const
    {
        int fd = unixConnect(socketPath_.c_str());
        if (fd < 0) return false;

        const std::string request = msg + "\n";
        if (!sendAll(fd, request.c_str(), request.size())) {
            close(fd);
            return false;
        }
        ::shutdown(fd, SHUT_WR);
        const bool received = receiveLine(fd, response);
        close(fd);
        return received;
    }

    std::string socketPath_;
    mutable std::mutex stateMutex_;
    std::condition_variable dynamicTargetCv_;
    std::mutex dynamicTargetMutex_;
    std::thread dynamicTargetThread_;
    bool stopDynamicTargetThread_ = false;
    bool available_ = false;
    bool appliedBlocking_ = false;
    std::string lastCmd_;
    std::string lastError_;

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

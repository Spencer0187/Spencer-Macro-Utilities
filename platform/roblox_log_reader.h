#pragma once

#include <cstdint>
#include <filesystem>
#include <memory>
#include <string>
#include <vector>

struct RoLogObject;

namespace smu::platform {

struct RobloxLogServerInfo {
    std::string type;
    std::string rccAddress;
    std::uint16_t rccPort = 0;
    std::string udmuxAddress;
    std::uint16_t udmuxPort = 0;
};

struct RobloxLogSnapshot {
    bool available = false;
    bool startedNewLog = false;
    std::filesystem::path path;
    std::vector<std::string> lines;
    std::string state;
    std::uint64_t placeId = 0;
    std::uint64_t userId = 0;
    std::uint64_t universeId = 0;
    std::string jobId;
    std::string clientChannel;
    std::string clientRobloxGitHash;
    std::string serverRobloxGitHash;
    std::string serverPrefix;
    std::string serverLuauVersion;
    std::uint16_t resolutionWidth = 0;
    std::uint16_t resolutionHeight = 0;
    RobloxLogServerInfo server;
};

// Incrementally reads the newest Roblox player log.  On Windows this is the
// official client's LocalAppData log directory; on Linux it is Sober's log
// directory; on macOS it is the official client's Library/Logs directory.
class RobloxLogReader {
public:
    RobloxLogReader();
    explicit RobloxLogReader(std::vector<std::filesystem::path> logDirectories);
    ~RobloxLogReader();

    RobloxLogReader(const RobloxLogReader&) = delete;
    RobloxLogReader& operator=(const RobloxLogReader&) = delete;

    // Returns lines appended since the prior poll.  When a file is first
    // discovered, includeExisting controls whether its existing lines are
    // returned or merely used to initialize RoLogParser's parsed metadata.
    RobloxLogSnapshot poll(bool includeExisting = false);

private:
    std::filesystem::path findNewestLog() const;
    void reset(const std::filesystem::path& path);

    std::vector<std::filesystem::path> logDirectories_;
    std::filesystem::path activePath_;
    std::string activePathText_;
    std::string pendingLine_;
    std::unique_ptr<RoLogObject, void (*)(RoLogObject*)> logObject_;
};

} // namespace smu::platform

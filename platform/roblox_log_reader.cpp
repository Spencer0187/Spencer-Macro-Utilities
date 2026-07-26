#include "roblox_log_reader.h"

#include "windows/RoLogParser.h"

#include <cstdlib>
#include <system_error>
#include <utility>

namespace smu::platform {
namespace {

std::filesystem::path HomeDirectory()
{
    const char* home = std::getenv("HOME");
    return home && home[0] != '\0' ? std::filesystem::path(home) : std::filesystem::path();
}

std::vector<std::filesystem::path> DefaultLogDirectories()
{
    std::vector<std::filesystem::path> directories;

#if defined(_WIN32)
    if (const char* localAppData = std::getenv("LOCALAPPDATA"); localAppData && localAppData[0] != '\0') {
        directories.emplace_back(std::filesystem::path(localAppData) / "Roblox" / "logs");
    }
#elif defined(__linux__)
    const std::filesystem::path home = HomeDirectory();
    if (const char* xdgData = std::getenv("XDG_DATA_HOME"); xdgData && xdgData[0] != '\0') {
        directories.emplace_back(std::filesystem::path(xdgData) / "sober" / "appData" / "logs");
    }
    if (!home.empty()) {
        directories.emplace_back(home / ".local" / "share" / "sober" / "appData" / "logs");
        directories.emplace_back(home / ".var" / "app" / "org.vinegarhq.Sober" / "data" / "sober" / "appData" / "logs");
    }
#elif defined(__APPLE__)
    const std::filesystem::path home = HomeDirectory();
    if (!home.empty()) {
        directories.emplace_back(home / "Library" / "Logs" / "Roblox");
        directories.emplace_back(home / "Library" / "Application Support" / "Roblox" / "logs");
    }
#endif

    return directories;
}

std::string ToString(const char* value)
{
    return value ? std::string(value) : std::string();
}

const char* StateName(char state)
{
    switch (state) {
    case ROLOG_IN_LUA_APP:
        return "lua_app";
    case ROLOG_IN_GAME:
        return "in_game";
    case ROLOG_OFFLINE:
        return "offline";
    default:
        return "unknown";
    }
}

const char* ServerTypeName(RoLogServerType type)
{
    switch (type) {
    case ROLOGRCCSERVER:
        return "rcc";
    case ROLOGUDMUXSERVER:
        return "udmux";
    default:
        return "unknown";
    }
}

struct LineCollector {
    std::string* pendingLine = nullptr;
    std::vector<std::string> lines;
};

void CollectLine(const char* line, void* userData)
{
    auto* collector = static_cast<LineCollector*>(userData);
    if (!collector || !collector->pendingLine || !line) {
        return;
    }

    collector->pendingLine->append(line);
    if (collector->pendingLine->empty() || collector->pendingLine->back() != '\n') {
        return;
    }
    std::string value = std::move(*collector->pendingLine);
    collector->pendingLine->clear();
    while (!value.empty() && (value.back() == '\n' || value.back() == '\r')) {
        value.pop_back();
    }
    collector->lines.push_back(std::move(value));
}

void PopulateSnapshot(RobloxLogSnapshot& snapshot, const RoLogObject& log)
{
    snapshot.state = StateName(log.current_state);
    snapshot.placeId = log.placeId;
    snapshot.userId = log.userId;
    snapshot.universeId = log.universeId;
    snapshot.jobId = ToString(log.serverJobId);
    snapshot.clientChannel = ToString(log.clientChannel);
    snapshot.clientRobloxGitHash = ToString(log.clientRobloxGitHash);
    snapshot.serverRobloxGitHash = ToString(log.serverRobloxGitHash);
    snapshot.serverPrefix = ToString(log.serverPrefix);
    snapshot.serverLuauVersion = ToString(log.serverLuauVersion);
    snapshot.resolutionWidth = log.resolutionWidth;
    snapshot.resolutionHeight = log.resolutionHeight;
    if (log.serverObj) {
        snapshot.server.type = ServerTypeName(log.serverObj->serverType);
        snapshot.server.rccAddress = ToString(log.serverObj->serverIPRCC);
        snapshot.server.rccPort = log.serverObj->serverPortRCC;
        snapshot.server.udmuxAddress = ToString(log.serverObj->serverIPUDMUX);
        snapshot.server.udmuxPort = log.serverObj->serverPortUDMUX;
    }
}

} // namespace

RobloxLogReader::RobloxLogReader()
    : RobloxLogReader(DefaultLogDirectories())
{
}

RobloxLogReader::RobloxLogReader(std::vector<std::filesystem::path> logDirectories)
    : logDirectories_(std::move(logDirectories)), logObject_(nullptr, RoLogFreeObject)
{
}

RobloxLogReader::~RobloxLogReader() = default;

std::filesystem::path RobloxLogReader::findNewestLog() const
{
    std::filesystem::path newest;
    std::filesystem::file_time_type newestTime{};
    std::error_code error;

    for (const std::filesystem::path& directory : logDirectories_) {
        if (!std::filesystem::is_directory(directory, error) || error) {
            error.clear();
            continue;
        }
        for (const std::filesystem::directory_entry& entry : std::filesystem::directory_iterator(directory, error)) {
            if (error) {
                error.clear();
                break;
            }
            if (!entry.is_regular_file(error) || error || entry.path().extension() != ".log") {
                error.clear();
                continue;
            }
            const std::filesystem::file_time_type modified = entry.last_write_time(error);
            if (!error && (newest.empty() || modified > newestTime)) {
                newest = entry.path();
                newestTime = modified;
            }
            error.clear();
        }
    }
    return newest;
}

void RobloxLogReader::reset(const std::filesystem::path& path)
{
    activePath_ = path;
    activePathText_ = activePath_.string();
    pendingLine_.clear();
    logObject_.reset(RoLogCreateObject(activePathText_.c_str()));
}

RobloxLogSnapshot RobloxLogReader::poll(bool includeExisting)
{
    RobloxLogSnapshot snapshot;
    const std::filesystem::path newest = findNewestLog();
    if (newest.empty()) {
        activePath_.clear();
        activePathText_.clear();
        pendingLine_.clear();
        logObject_.reset();
        return snapshot;
    }

    std::error_code error;
    const std::uintmax_t fileSize = std::filesystem::file_size(newest, error);
    const bool fileWasTruncated = !error && logObject_ && newest == activePath_ && fileSize < logObject_->last_idx;
    const bool startedNewLog = !logObject_ || newest != activePath_ || fileWasTruncated;
    if (startedNewLog) {
        reset(newest);
    }
    if (!logObject_) {
        return snapshot;
    }

    LineCollector collector{&pendingLine_};
    const bool captureLines = !startedNewLog || includeExisting;
    const RoLogParseRes result = captureLines
        ? RoLogParseWithCallback(logObject_.get(), CollectLine, &collector)
        : RoLogParse(logObject_.get());
    if (result != ROLOGSUCCESS) {
        activePath_.clear();
        activePathText_.clear();
        pendingLine_.clear();
        logObject_.reset();
        return snapshot;
    }

    snapshot.available = true;
    snapshot.startedNewLog = startedNewLog;
    snapshot.path = activePath_;
    snapshot.lines = std::move(collector.lines);
    PopulateSnapshot(snapshot, *logObject_);
    return snapshot;
}

} // namespace smu::platform

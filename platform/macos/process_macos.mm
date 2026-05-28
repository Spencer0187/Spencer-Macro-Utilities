#include "process_macos.h"

#if defined(__APPLE__)

#include "../logging.h"

#import <AppKit/AppKit.h>
#import <Foundation/Foundation.h>

#include <algorithm>
#include <cerrno>
#include <charconv>
#include <chrono>
#include <cctype>
#include <cstring>
#include <mutex>
#include <optional>
#include <signal.h>
#include <sstream>
#include <string>
#include <unistd.h>
#include <vector>

namespace smu::platform::macos {
namespace {

std::string ToStdString(NSString* value)
{
    if (!value) {
        return {};
    }
    const char* utf8 = [value UTF8String];
    return utf8 ? std::string(utf8) : std::string{};
}

std::string Lowercase(std::string value)
{
    std::transform(value.begin(), value.end(), value.begin(), [](unsigned char ch) {
        return static_cast<char>(std::tolower(ch));
    });
    return value;
}

void RemoveSuffix(std::string& value, const std::string& suffix)
{
    if (value.size() >= suffix.size() &&
        value.compare(value.size() - suffix.size(), suffix.size(), suffix) == 0) {
        value.resize(value.size() - suffix.size());
    }
}

std::string NormalizeProcessName(std::string value)
{
    const std::size_t slash = value.find_last_of("/\\");
    if (slash != std::string::npos) {
        value.erase(0, slash + 1);
    }
    value = Lowercase(std::move(value));
    RemoveSuffix(value, ".exe");
    RemoveSuffix(value, ".app");
    return value;
}

bool IsRobloxPlayerQuery(const std::string& query)
{
    return query == "roblox" || query == "robloxplayer" || query == "robloxplayerbeta";
}

bool TextLooksLikeRobloxPlayer(const std::string& text)
{
    const std::string normalized = NormalizeProcessName(text);
    if (normalized.find("studio") != std::string::npos) {
        return false;
    }
    return normalized == "roblox" || normalized == "robloxplayer" ||
        normalized == "robloxplayerbeta" ||
        normalized.find("robloxplayer") != std::string::npos ||
        normalized.find("com.roblox.roblox") != std::string::npos;
}

std::vector<std::string> CandidateNames(NSRunningApplication* app)
{
    std::vector<std::string> names;
    if (!app) {
        return names;
    }

    names.push_back(ToStdString(app.localizedName));
    names.push_back(ToStdString(app.bundleIdentifier));
    if (app.executableURL) {
        names.push_back(ToStdString(app.executableURL.lastPathComponent));
    }
    if (app.bundleURL) {
        names.push_back(ToStdString(app.bundleURL.lastPathComponent));
    }
    return names;
}

bool AppMatchesQuery(NSRunningApplication* app, const std::string& query)
{
    const std::string normalizedQuery = NormalizeProcessName(query);
    if (normalizedQuery.empty()) {
        return false;
    }

    const std::vector<std::string> candidates = CandidateNames(app);
    if (IsRobloxPlayerQuery(normalizedQuery)) {
        return std::any_of(candidates.begin(), candidates.end(), [](const std::string& candidate) {
            return TextLooksLikeRobloxPlayer(candidate);
        });
    }

    return std::any_of(candidates.begin(), candidates.end(), [&normalizedQuery](const std::string& candidate) {
        return NormalizeProcessName(candidate) == normalizedQuery;
    });
}

std::optional<PlatformPid> ParsePid(const std::string& token)
{
    if (token.empty()) {
        return std::nullopt;
    }

    PlatformPid pid = 0;
    const auto [end, error] = std::from_chars(token.data(), token.data() + token.size(), pid);
    if (error != std::errc() || end != token.data() + token.size() || pid == 0) {
        return std::nullopt;
    }
    return pid;
}

bool IsRunningPid(PlatformPid pid)
{
    @autoreleasepool {
        return [NSRunningApplication runningApplicationWithProcessIdentifier:static_cast<pid_t>(pid)] != nil;
    }
}

bool SendSignal(PlatformPid pid, int signal, const char* action)
{
    if (pid == 0 || pid == static_cast<PlatformPid>(getpid())) {
        return false;
    }

    if (kill(static_cast<pid_t>(pid), signal) == 0) {
        return true;
    }

    LogWarning(
        "macOS process backend failed to " + std::string(action) + " PID " +
        std::to_string(pid) + ": " + std::strerror(errno));
    return false;
}

std::vector<std::string> QueryTokens(const std::string& executableName)
{
    std::istringstream input(executableName);
    std::vector<std::string> tokens;
    std::string token;
    while (input >> token) {
        tokens.push_back(token);
    }
    return tokens;
}

class MacosProcessBackend final : public ProcessBackend {
public:
    bool init(std::string* errorMessage = nullptr) override
    {
        if (errorMessage) {
            errorMessage->clear();
        }
        @autoreleasepool {
            return [NSWorkspace sharedWorkspace] != nil;
        }
    }

    void shutdown() override {}

    std::optional<PlatformPid> findProcess(const std::string& executableName) const override
    {
        return findMainProcess(executableName);
    }

    std::vector<PlatformPid> findAllProcesses(const std::string& executableName) const override
    {
        const auto now = std::chrono::steady_clock::now();
        {
            std::lock_guard<std::mutex> lock(cacheMutex_);
            if (executableName == cachedExecutableName_ &&
                now < cachedFindAllExpiresAt_) {
                return cachedFindAllPids_;
            }
        }

        std::vector<PlatformPid> pids;
        const std::vector<std::string> tokens = QueryTokens(executableName);
        if (tokens.empty()) {
            return pids;
        }

        @autoreleasepool {
            NSArray<NSRunningApplication*>* runningApps =
                [[NSWorkspace sharedWorkspace] runningApplications];
            for (const std::string& token : tokens) {
                if (const auto pid = ParsePid(token)) {
                    if (IsRunningPid(*pid)) {
                        pids.push_back(*pid);
                    }
                    continue;
                }

                for (NSRunningApplication* app in runningApps) {
                    if (AppMatchesQuery(app, token)) {
                        pids.push_back(static_cast<PlatformPid>(app.processIdentifier));
                    }
                }
            }
        }

        std::sort(pids.begin(), pids.end());
        pids.erase(std::unique(pids.begin(), pids.end()), pids.end());
        {
            std::lock_guard<std::mutex> lock(cacheMutex_);
            cachedExecutableName_ = executableName;
            cachedFindAllPids_ = pids;
            cachedFindAllExpiresAt_ = now + kFindAllCacheDuration;
        }
        return pids;
    }

    std::optional<PlatformPid> findMainProcess(const std::string& executableName) const override
    {
        const std::vector<PlatformPid> pids = findAllProcesses(executableName);
        if (pids.empty()) {
            return std::nullopt;
        }

        @autoreleasepool {
            NSRunningApplication* frontmost = [[NSWorkspace sharedWorkspace] frontmostApplication];
            if (frontmost) {
                const PlatformPid foregroundPid =
                    static_cast<PlatformPid>(frontmost.processIdentifier);
                if (std::find(pids.begin(), pids.end(), foregroundPid) != pids.end()) {
                    return foregroundPid;
                }
            }
        }

        return pids.front();
    }

    bool suspend(PlatformPid pid) override
    {
        return SendSignal(pid, SIGSTOP, "suspend");
    }

    bool resume(PlatformPid pid) override
    {
        return SendSignal(pid, SIGCONT, "resume");
    }

    bool isForegroundProcess(PlatformPid pid) const override
    {
        @autoreleasepool {
            NSRunningApplication* frontmost = [[NSWorkspace sharedWorkspace] frontmostApplication];
            return frontmost && frontmost.processIdentifier == static_cast<pid_t>(pid);
        }
    }

private:
    static constexpr auto kFindAllCacheDuration = std::chrono::milliseconds(250);

    mutable std::mutex cacheMutex_;
    mutable std::string cachedExecutableName_;
    mutable std::vector<PlatformPid> cachedFindAllPids_;
    mutable std::chrono::steady_clock::time_point cachedFindAllExpiresAt_{};
};

} // namespace

std::shared_ptr<smu::platform::ProcessBackend> CreateMacosProcessBackend()
{
    return std::make_shared<MacosProcessBackend>();
}

} // namespace smu::platform::macos

#endif

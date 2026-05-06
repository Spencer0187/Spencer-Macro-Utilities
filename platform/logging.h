#pragma once

#include <chrono>
#include <string>
#include <vector>

namespace smu::log {

enum class LogLevel {
    Info,
    Warning,
    Error,
    Critical
};

struct LogEntry {
    LogLevel level = LogLevel::Info;
    std::string message;
    std::string id;
    bool onLaunch = false;
    std::chrono::system_clock::time_point timestamp;
};

constexpr const char* kCriticalSupportText =
    "Join the Discord for help: https://discord.gg/roblox-glitching-community-998572881892094012";

void LogInfo(const std::string& message);
void LogWarning(const std::string& message);
void LogWarning(const std::string& message, const std::string& id, bool onLaunch);
void LogError(const std::string& message);
void LogCritical(const std::string& message);

std::vector<LogEntry> GetLogSnapshot();
std::vector<LogEntry> DrainCriticalNotifications();
std::vector<LogEntry> DrainWarningNotifications();
bool IsDebugLoggingEnabled();
void SetFileLoggingEnabled(bool enabled);

} // namespace smu::log

inline void LogInfo(const std::string& message) { smu::log::LogInfo(message); }
inline void LogWarning(const std::string& message) { smu::log::LogWarning(message); }
inline void LogWarning(const std::string& message, const std::string& id, bool onLaunch) { smu::log::LogWarning(message, id, onLaunch); }
inline void LogError(const std::string& message) { smu::log::LogError(message); }
inline void LogCritical(const std::string& message) { smu::log::LogCritical(message); }

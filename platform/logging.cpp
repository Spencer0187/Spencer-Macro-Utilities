#include "logging.h"

#include <cstdlib>
#include <ctime>
#include <deque>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <mutex>
#include <sstream>

#if defined(_WIN32)
#define NOMINMAX
#include <windows.h>
#endif

namespace smu::log {
namespace {

constexpr std::size_t kMaxLogEntries = 512;

std::mutex g_logMutex;
std::deque<LogEntry> g_ringBuffer;
std::deque<LogEntry> g_criticalNotifications;
std::deque<LogEntry> g_warningNotifications;
bool g_fileLoggingEnabled = false;

const char* LevelName(LogLevel level)
{
    switch (level) {
    case LogLevel::Info: return "INFO";
    case LogLevel::Warning: return "WARN";
    case LogLevel::Error: return "ERROR";
    case LogLevel::Critical: return "CRITICAL";
    default: return "UNKNOWN";
    }
}

bool ShouldLogToConsole()
{
#if defined(_DEBUG)
    return true;
#else
    const char* debug = std::getenv("DEBUG");
    const char* smuDebug = std::getenv("SMU_DEBUG");
    return (debug && std::string(debug) == "1") || (smuDebug && std::string(smuDebug) == "1");
#endif
}

std::string FormatTimestamp(const std::chrono::system_clock::time_point& timestamp)
{
    const std::time_t time = std::chrono::system_clock::to_time_t(timestamp);
    std::tm localTime = {};
#if defined(_WIN32)
    localtime_s(&localTime, &time);
#else
    localtime_r(&time, &localTime);
#endif

    std::ostringstream stream;
    stream << std::put_time(&localTime, "%Y-%m-%d %H:%M:%S");
    return stream.str();
}

std::string FormatEntry(const LogEntry& entry)
{
    std::ostringstream stream;
    stream << "[" << FormatTimestamp(entry.timestamp) << "] "
           << "[" << LevelName(entry.level) << "] "
           << entry.message;
    return stream.str();
}

std::string WithSupportText(const std::string& message)
{
    if (message.find(kCriticalSupportText) != std::string::npos) {
        return message;
    }

    return message + "\n\n" + kCriticalSupportText;
}

void WriteEntry(LogLevel level, const std::string& rawMessage)
{
    LogEntry entry;
    entry.level = level;
    entry.message = (level == LogLevel::Critical) ? WithSupportText(rawMessage) : rawMessage;
    entry.timestamp = std::chrono::system_clock::now();

    const std::string formatted = FormatEntry(entry);

    {
        std::lock_guard<std::mutex> lock(g_logMutex);
        if (g_ringBuffer.size() >= kMaxLogEntries) {
            g_ringBuffer.pop_front();
        }
        g_ringBuffer.push_back(entry);

        if (level == LogLevel::Warning) {
            if (g_warningNotifications.size() >= kMaxLogEntries) {
                g_warningNotifications.pop_front();
            }
            g_warningNotifications.push_back(entry);
        } else if (level == LogLevel::Critical) {
            if (g_criticalNotifications.size() >= kMaxLogEntries) {
                g_criticalNotifications.pop_front();
            }
            g_criticalNotifications.push_back(entry);
        }

        if (g_fileLoggingEnabled) {
            std::ofstream file("SMC.log", std::ios::app);
            if (file) {
                file << formatted << '\n';
            }
        }
    }

    if (ShouldLogToConsole()) {
        std::ostream& out = (level == LogLevel::Info || level == LogLevel::Warning) ? std::cout : std::cerr;
        out << formatted << std::endl;
    }

#if defined(_WIN32)
    OutputDebugStringA((formatted + "\n").c_str());
#endif
}

} // namespace

void LogInfo(const std::string& message)
{
    WriteEntry(LogLevel::Info, message);
}

void LogWarning(const std::string& message)
{
    WriteEntry(LogLevel::Warning, message);
}

void LogError(const std::string& message)
{
    WriteEntry(LogLevel::Error, message);
}

void LogCritical(const std::string& message)
{
    WriteEntry(LogLevel::Critical, message);
}

std::vector<LogEntry> GetLogSnapshot()
{
    std::lock_guard<std::mutex> lock(g_logMutex);
    return {g_ringBuffer.begin(), g_ringBuffer.end()};
}

std::vector<LogEntry> DrainCriticalNotifications()
{
    std::lock_guard<std::mutex> lock(g_logMutex);
    std::vector<LogEntry> notifications{g_criticalNotifications.begin(), g_criticalNotifications.end()};
    g_criticalNotifications.clear();
    return notifications;
}

std::vector<LogEntry> DrainWarningNotifications()
{
    std::lock_guard<std::mutex> lock(g_logMutex);
    std::vector<LogEntry> notifications{g_warningNotifications.begin(), g_warningNotifications.end()};
    g_warningNotifications.clear();
    return notifications;
}

void SetFileLoggingEnabled(bool enabled)
{
    std::lock_guard<std::mutex> lock(g_logMutex);
    g_fileLoggingEnabled = enabled;
}

} // namespace smu::log

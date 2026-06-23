#include "linux_lagswitch_helper.h"

#if defined(__linux__)

#include "../platform/logging.h"

#include <sys/stat.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <sys/un.h>
#include <unistd.h>

#include <cerrno>
#include <chrono>
#include <cstddef>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <string>
#include <system_error>
#include <thread>
#include <vector>

namespace smu::app {
namespace {

constexpr const char kNethelperSocketPath[] = "/tmp/nethelper.sock";

std::string GetExecutableBasePath()
{
    std::vector<char> buffer(4096);
    while (true) {
        const ssize_t length = readlink("/proc/self/exe", buffer.data(), buffer.size() - 1);
        if (length < 0) {
            return {};
        }
        if (static_cast<std::size_t>(length) < buffer.size() - 1) {
            buffer[static_cast<std::size_t>(length)] = '\0';
            return std::filesystem::path(buffer.data()).parent_path().string();
        }
        buffer.resize(buffer.size() * 2);
    }
}

bool PathExists(const std::filesystem::path& path)
{
    std::error_code ec;
    return !path.empty() && std::filesystem::exists(path, ec) && !ec;
}

bool IsNethelperReachable()
{
    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (fd < 0) {
        return false;
    }

    sockaddr_un addr{};
    addr.sun_family = AF_UNIX;
    std::snprintf(addr.sun_path, sizeof(addr.sun_path), "%s", kNethelperSocketPath);

    if (connect(fd, reinterpret_cast<sockaddr*>(&addr), SUN_LEN(&addr)) != 0) {
        close(fd);
        return false;
    }

    const char msg[] = "ping";
    if (send(fd, msg, std::strlen(msg), MSG_NOSIGNAL) <= 0) {
        close(fd);
        return false;
    }

    char buf[64];
    const ssize_t n = recv(fd, buf, sizeof(buf) - 1, 0);
    close(fd);

    if (n <= 0) {
        return false;
    }
    buf[n] = '\0';
    return std::string(buf).find("pong") != std::string::npos;
}

std::vector<std::filesystem::path> AppDirCandidates()
{
    std::vector<std::filesystem::path> candidates;
    if (const char* smuAppDir = std::getenv("SMU_APPDIR")) {
        if (smuAppDir[0] != '\0') {
            candidates.emplace_back(smuAppDir);
        }
    }
    if (const char* appDir = std::getenv("APPDIR")) {
        if (appDir[0] != '\0') {
            candidates.emplace_back(appDir);
        }
    }
    return candidates;
}

std::string ResolveNethelperPath()
{
    const std::filesystem::path exeDir = GetExecutableBasePath();
    const std::vector<std::filesystem::path> directCandidates = {
        exeDir / "nethelper",
        exeDir / "usr" / "bin" / "nethelper",
    };
    for (const auto& path : directCandidates) {
        if (PathExists(path)) {
            return path.string();
        }
    }

    for (const auto& appDir : AppDirCandidates()) {
        const std::vector<std::filesystem::path> appDirCandidates = {
            appDir / "usr" / "bin" / "nethelper",
            appDir / "nethelper",
        };
        for (const auto& path : appDirCandidates) {
            if (PathExists(path)) {
                return path.string();
            }
        }
    }

    const std::filesystem::path cwdCandidate = std::filesystem::current_path() / "nethelper";
    if (PathExists(cwdCandidate)) {
        return cwdCandidate.string();
    }

    return (exeDir / "nethelper").string();
}

std::string StagedNethelperPath()
{
    const char* user = std::getenv("USER");
    std::string suffix = (user && user[0] != '\0') ? user : std::to_string(getuid());
    return "/tmp/nethelper-" + suffix;
}

bool StageNethelper(const std::string& sourcePath, std::string* errorMessage)
{
    if (!PathExists(sourcePath)) {
        if (errorMessage) {
            *errorMessage = "Linux lagswitch helper was not found at " + sourcePath + ".";
        }
        return false;
    }

    const std::string stagedPath = StagedNethelperPath();
    std::error_code ec;
    std::filesystem::copy_file(
        sourcePath,
        stagedPath,
        std::filesystem::copy_options::overwrite_existing,
        ec);
    if (ec) {
        if (errorMessage) {
            *errorMessage = "Could not stage Linux lagswitch helper at " + stagedPath + ": " + ec.message() + ".";
        }
        return false;
    }

    if (chmod(stagedPath.c_str(), 0755) != 0) {
        if (errorMessage) {
            *errorMessage = "Could not mark Linux lagswitch helper executable: " + std::string(std::strerror(errno)) + ".";
        }
        return false;
    }

    return true;
}

bool WaitForNethelper(pid_t pid, std::string* errorMessage)
{
    using namespace std::chrono_literals;

    const auto deadline = std::chrono::steady_clock::now() + 120s;
    while (std::chrono::steady_clock::now() < deadline) {
        if (IsNethelperReachable()) {
            return true;
        }

        int status = 0;
        const pid_t waitResult = waitpid(pid, &status, WNOHANG);
        if (waitResult == pid) {
            if (errorMessage) {
                if (WIFEXITED(status) && WEXITSTATUS(status) == 127) {
                    *errorMessage = "pkexec was not found. Install polkit or start nethelper manually with elevated permissions.";
                } else if (WIFEXITED(status) && WEXITSTATUS(status) == 126) {
                    *errorMessage = "Linux lagswitch helper could not be launched.";
                } else {
                    *errorMessage = "Linux lagswitch helper permission prompt was cancelled or denied.";
                }
            }
            return false;
        }
        if (waitResult < 0 && errno != EINTR) {
            if (errorMessage) {
                *errorMessage = "Could not monitor Linux lagswitch helper startup: " + std::string(std::strerror(errno)) + ".";
            }
            return false;
        }

        std::this_thread::sleep_for(100ms);
    }

    if (errorMessage) {
        *errorMessage = "Timed out waiting for Linux lagswitch helper authentication.";
    }
    return false;
}

} // namespace

bool StartLinuxNetworkHelperWithGraphicalPkexec(std::string* errorMessage)
{
    if (IsNethelperReachable()) {
        return true;
    }

    const std::string nethelperPath = ResolveNethelperPath();
    if (!StageNethelper(nethelperPath, errorMessage)) {
        return false;
    }

    const std::string stagedPath = StagedNethelperPath();
    const pid_t pid = fork();
    if (pid < 0) {
        if (errorMessage) {
            *errorMessage = "Failed to fork Linux lagswitch helper: " + std::string(std::strerror(errno)) + ".";
        }
        return false;
    }

    if (pid == 0) {
        if (geteuid() == 0) {
            execl(stagedPath.c_str(), stagedPath.c_str(), static_cast<char*>(nullptr));
        } else {
            execlp(
                "pkexec",
                "pkexec",
                "--disable-internal-agent",
                stagedPath.c_str(),
                static_cast<char*>(nullptr));
        }
        _exit(errno == ENOENT ? 127 : 126);
    }

    if (!WaitForNethelper(pid, errorMessage)) {
        return false;
    }

    LogInfo("Linux lagswitch helper started.");
    return true;
}

} // namespace smu::app

#endif

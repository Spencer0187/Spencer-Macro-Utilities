#include "linux_lagswitch_helper.h"

#if defined(__linux__)

#include "../platform/logging.h"

#include <sys/stat.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <sys/un.h>
#include <unistd.h>

#include <cerrno>
#include <chrono>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <sstream>
#include <string>
#include <system_error>
#include <thread>
#include <vector>

namespace smu::app {
namespace {

std::string NethelperSocketPath()
{
    return "/tmp/smu-nethelper-" + std::to_string(getuid()) + ".sock";
}

bool IsRootPeer(int fd)
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

bool ReadProcessStartTime(pid_t pid, std::uint64_t* startTime)
{
    if (!startTime || pid <= 0) {
        return false;
    }

    std::ifstream statFile("/proc/" + std::to_string(pid) + "/stat");
    std::string stat;
    if (!statFile || !std::getline(statFile, stat)) {
        return false;
    }

    const std::size_t closeName = stat.rfind(')');
    if (closeName == std::string::npos || closeName + 1 >= stat.size()) {
        return false;
    }

    // The first token after the executable name is field 3 (state).
    // Linux documents process starttime as field 22.
    std::istringstream fields(stat.substr(closeName + 1));
    std::string value;
    for (int field = 3; field <= 22; ++field) {
        if (!(fields >> value)) {
            return false;
        }
    }

    try {
        std::size_t parsedLength = 0;
        const unsigned long long parsed = std::stoull(value, &parsedLength, 10);
        if (parsed == 0 || parsedLength != value.size()) {
            return false;
        }
        *startTime = static_cast<std::uint64_t>(parsed);
        return true;
    } catch (...) {
        return false;
    }
}

bool IsNethelperReachable()
{
    const std::string socketPath = NethelperSocketPath();
    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (fd < 0) {
        return false;
    }
    timeval timeout{};
    timeout.tv_sec = 2;
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout));
    setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, sizeof(timeout));

    sockaddr_un addr{};
    addr.sun_family = AF_UNIX;
    if (socketPath.size() >= sizeof(addr.sun_path)) {
        close(fd);
        return false;
    }
    std::snprintf(addr.sun_path, sizeof(addr.sun_path), "%s", socketPath.c_str());

    if (connect(fd, reinterpret_cast<sockaddr*>(&addr), SUN_LEN(&addr)) != 0) {
        close(fd);
        return false;
    }
    if (!IsRootPeer(fd)) {
        close(fd);
        return false;
    }

    const char msg[] = "ping\n";
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
    return std::string(buf).find("PONG") != std::string::npos;
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

bool WriteAll(int fd, const char* data, std::size_t length)
{
    while (length > 0) {
        const ssize_t written = write(fd, data, length);
        if (written < 0 && errno == EINTR) {
            continue;
        }
        if (written <= 0) {
            return false;
        }
        data += written;
        length -= static_cast<std::size_t>(written);
    }
    return true;
}

bool StageNethelper(
    const std::string& sourcePath,
    std::string* stagedPath,
    std::string* errorMessage)
{
    if (!PathExists(sourcePath)) {
        if (errorMessage) {
            *errorMessage = "Linux lagswitch helper was not found at " + sourcePath + ".";
        }
        return false;
    }

    char pathTemplate[] = "/tmp/smu-nethelper-stage-XXXXXX";
    const int destination = mkstemp(pathTemplate);
    if (destination < 0) {
        if (errorMessage) {
            *errorMessage = "Could not create a private Linux lagswitch helper staging file: " +
                std::string(std::strerror(errno)) + ".";
        }
        return false;
    }

    const std::string privatePath(pathTemplate);
    const auto fail = [&](const std::string& message) {
        close(destination);
        unlink(privatePath.c_str());
        if (errorMessage) {
            *errorMessage = message;
        }
        return false;
    };

    if (fchmod(destination, 0700) != 0) {
        return fail(
            "Could not protect the staged Linux lagswitch helper: " +
            std::string(std::strerror(errno)) + ".");
    }

    std::ifstream source(sourcePath, std::ios::binary);
    if (!source) {
        return fail("Could not read Linux lagswitch helper at " + sourcePath + ".");
    }

    std::vector<char> buffer(64 * 1024);
    while (source) {
        source.read(buffer.data(), static_cast<std::streamsize>(buffer.size()));
        const std::streamsize count = source.gcount();
        if (count > 0 &&
            !WriteAll(destination, buffer.data(), static_cast<std::size_t>(count))) {
            return fail(
                "Could not stage Linux lagswitch helper: " +
                std::string(std::strerror(errno)) + ".");
        }
    }
    if (!source.eof()) {
        return fail("Could not finish reading Linux lagswitch helper at " + sourcePath + ".");
    }
    if (fsync(destination) != 0) {
        return fail(
            "Could not finish staging Linux lagswitch helper: " +
            std::string(std::strerror(errno)) + ".");
    }
    if (close(destination) != 0) {
        unlink(privatePath.c_str());
        if (errorMessage) {
            *errorMessage = "Could not close the staged Linux lagswitch helper: " +
                std::string(std::strerror(errno)) + ".";
        }
        return false;
    }

    *stagedPath = privatePath;
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

void ReapNethelperProcess(pid_t pid)
{
    std::thread([pid]() {
        int status = 0;
        while (waitpid(pid, &status, 0) < 0 && errno == EINTR) {
        }
    }).detach();
}

} // namespace

bool StartLinuxNetworkHelperWithGraphicalPkexec(std::string* errorMessage)
{
    if (IsNethelperReachable()) {
        return true;
    }

    const pid_t ownerPid = getpid();
    std::uint64_t ownerStartTimeValue = 0;
    if (!ReadProcessStartTime(ownerPid, &ownerStartTimeValue)) {
        if (errorMessage) {
            *errorMessage =
                "Could not read the SMU process identity required to safely start the Linux lagswitch helper.";
        }
        return false;
    }

    const std::string nethelperPath = ResolveNethelperPath();
    std::string stagedPath;
    if (!StageNethelper(nethelperPath, &stagedPath, errorMessage)) {
        return false;
    }

    const pid_t pid = fork();
    if (pid < 0) {
        unlink(stagedPath.c_str());
        if (errorMessage) {
            *errorMessage = "Failed to fork Linux lagswitch helper: " + std::string(std::strerror(errno)) + ".";
        }
        return false;
    }

    if (pid == 0) {
        const std::string invokingUid = std::to_string(getuid());
        const std::string ownerPidText = std::to_string(ownerPid);
        const std::string ownerStartTime = std::to_string(ownerStartTimeValue);
        if (geteuid() == 0) {
            execl(
                stagedPath.c_str(),
                stagedPath.c_str(),
                "--uid",
                invokingUid.c_str(),
                "--owner-pid",
                ownerPidText.c_str(),
                "--owner-start-time",
                ownerStartTime.c_str(),
                static_cast<char*>(nullptr));
        } else {
            execlp(
                "pkexec",
                "pkexec",
                "--disable-internal-agent",
                stagedPath.c_str(),
                "--uid",
                invokingUid.c_str(),
                "--owner-pid",
                ownerPidText.c_str(),
                "--owner-start-time",
                ownerStartTime.c_str(),
                static_cast<char*>(nullptr));
        }
        _exit(errno == ENOENT ? 127 : 126);
    }

    const bool started = WaitForNethelper(pid, errorMessage);
    ReapNethelperProcess(pid);
    unlink(stagedPath.c_str());
    if (!started) {
        return false;
    }

    LogInfo("Linux lagswitch helper started.");
    return true;
}

} // namespace smu::app

#endif

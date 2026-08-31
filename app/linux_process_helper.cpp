#include "linux_process_helper.h"

#if defined(__linux__)

#include "../platform/logging.h"

#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/types.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <unistd.h>

#include <cerrno>
#include <chrono>
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

std::string HelperSocketPath()
{
    return "/tmp/smu-processhelper-" + std::to_string(getuid()) + ".sock";
}

bool IsRootPeer(int fd)
{
    struct { pid_t pid; uid_t uid; gid_t gid; } credentials{};
    socklen_t length = sizeof(credentials);
    return getsockopt(fd, SOL_SOCKET, SO_PEERCRED, &credentials, &length) == 0 &&
           length == sizeof(credentials) && credentials.uid == 0;
}

std::string ExecutableDirectory()
{
    std::vector<char> buffer(4096);
    while (true) {
        const ssize_t length = readlink("/proc/self/exe", buffer.data(), buffer.size() - 1);
        if (length < 0) return {};
        if (static_cast<std::size_t>(length) < buffer.size() - 1) {
            buffer[static_cast<std::size_t>(length)] = '\0';
            return std::filesystem::path(buffer.data()).parent_path().string();
        }
        buffer.resize(buffer.size() * 2);
    }
}

bool Exists(const std::filesystem::path& path)
{
    std::error_code ec;
    return !path.empty() && std::filesystem::exists(path, ec) && !ec;
}

std::string ResolveHelperPath()
{
    const auto exeDir = std::filesystem::path(ExecutableDirectory());
    for (const auto& candidate : {exeDir / "processhelper", exeDir / "usr" / "bin" / "processhelper"}) {
        if (Exists(candidate)) return candidate.string();
    }
    for (const char* variable : {"SMU_APPDIR", "APPDIR"}) {
        if (const char* value = std::getenv(variable); value && *value) {
            for (const auto& candidate : {
                     std::filesystem::path(value) / "usr" / "bin" / "processhelper",
                     std::filesystem::path(value) / "processhelper"}) {
                if (Exists(candidate)) return candidate.string();
            }
        }
    }
    return (exeDir / "processhelper").string();
}

bool ReadProcessStartTime(pid_t pid, std::uint64_t* startTime)
{
    std::ifstream file("/proc/" + std::to_string(pid) + "/stat");
    std::string stat;
    if (!startTime || !file || !std::getline(file, stat)) return false;
    const auto close = stat.rfind(')');
    if (close == std::string::npos) return false;
    std::istringstream fields(stat.substr(close + 1));
    std::string value;
    for (int field = 3; field <= 22; ++field) if (!(fields >> value)) return false;
    try {
        std::size_t used = 0;
        const auto parsed = std::stoull(value, &used, 10);
        if (!parsed || used != value.size()) return false;
        *startTime = parsed;
        return true;
    } catch (...) { return false; }
}

bool Reachable()
{
    const std::string path = HelperSocketPath();
    const int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (fd < 0) return false;
    timeval timeout{}; timeout.tv_sec = 2;
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout));
    setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, sizeof(timeout));
    sockaddr_un address{}; address.sun_family = AF_UNIX;
    if (path.size() >= sizeof(address.sun_path)) { close(fd); return false; }
    std::snprintf(address.sun_path, sizeof(address.sun_path), "%s", path.c_str());
    if (connect(fd, reinterpret_cast<sockaddr*>(&address), SUN_LEN(&address)) != 0 || !IsRootPeer(fd)) {
        close(fd); return false;
    }
    static constexpr char message[] = "ping\n";
    if (send(fd, message, sizeof(message) - 1, MSG_NOSIGNAL) <= 0) { close(fd); return false; }
    char response[32]{};
    const ssize_t count = recv(fd, response, sizeof(response) - 1, 0);
    close(fd);
    return count > 0 && std::string(response, static_cast<std::size_t>(count)).find("PONG") != std::string::npos;
}

bool WriteAll(int fd, const char* data, std::size_t length)
{
    while (length) {
        const ssize_t count = write(fd, data, length);
        if (count < 0 && errno == EINTR) continue;
        if (count <= 0) return false;
        data += count;
        length -= static_cast<std::size_t>(count);
    }
    return true;
}

bool StageHelper(const std::string& sourcePath, std::string* stagedPath, std::string* errorMessage)
{
    if (!Exists(sourcePath)) {
        if (errorMessage) *errorMessage = "Linux process helper was not found at " + sourcePath + ".";
        return false;
    }
    char pattern[] = "/tmp/smu-processhelper-stage-XXXXXX";
    const int out = mkstemp(pattern);
    if (out < 0) {
        if (errorMessage) *errorMessage = "Could not stage Linux process helper: " + std::string(std::strerror(errno)) + ".";
        return false;
    }
    const std::string path(pattern);
    const auto fail = [&](const std::string& message) {
        close(out); unlink(path.c_str()); if (errorMessage) *errorMessage = message; return false;
    };
    if (fchmod(out, 0700) != 0) return fail("Could not protect staged Linux process helper.");
    std::ifstream in(sourcePath, std::ios::binary);
    if (!in) return fail("Could not read Linux process helper.");
    std::vector<char> buffer(64 * 1024);
    while (in) {
        in.read(buffer.data(), static_cast<std::streamsize>(buffer.size()));
        const auto count = in.gcount();
        if (count > 0 && !WriteAll(out, buffer.data(), static_cast<std::size_t>(count))) return fail("Could not copy Linux process helper.");
    }
    if (!in.eof() || fsync(out) != 0 || close(out) != 0) {
        unlink(path.c_str()); if (errorMessage) *errorMessage = "Could not finish staging Linux process helper."; return false;
    }
    *stagedPath = path;
    return true;
}

bool WaitForHelper(pid_t pid, std::string* errorMessage)
{
    using namespace std::chrono_literals;
    const auto deadline = std::chrono::steady_clock::now() + 120s;
    while (std::chrono::steady_clock::now() < deadline) {
        if (Reachable()) return true;
        int status = 0;
        const pid_t result = waitpid(pid, &status, WNOHANG);
        if (result == pid) {
            if (errorMessage) *errorMessage = WIFEXITED(status) && WEXITSTATUS(status) == 127
                ? "pkexec was not found. Install polkit to use the privileged freeze fallback."
                : "Linux process helper permission prompt was cancelled, denied, or the helper failed to start.";
            return false;
        }
        if (result < 0 && errno != EINTR) return false;
        std::this_thread::sleep_for(100ms);
    }
    if (errorMessage) *errorMessage = "Timed out waiting for Linux process helper authentication.";
    return false;
}

void Reap(pid_t pid)
{
    std::thread([pid] { int status = 0; while (waitpid(pid, &status, 0) < 0 && errno == EINTR) {} }).detach();
}

} // namespace

bool StartLinuxProcessHelperWithGraphicalPkexec(std::string* errorMessage)
{
    if (Reachable()) return true;

    const pid_t ownerPid = getpid();
    std::uint64_t startTime = 0;
    if (!ReadProcessStartTime(ownerPid, &startTime)) {
        if (errorMessage) *errorMessage = "Could not read the SMU process identity required to safely start the Linux process helper.";
        return false;
    }

    std::string stagedPath;
    if (!StageHelper(ResolveHelperPath(), &stagedPath, errorMessage)) return false;

    const pid_t pid = fork();
    if (pid < 0) {
        unlink(stagedPath.c_str());
        if (errorMessage) *errorMessage = "Failed to fork Linux process helper: " + std::string(std::strerror(errno)) + ".";
        return false;
    }
    if (pid == 0) {
        const std::string uid = std::to_string(getuid());
        const std::string owner = std::to_string(ownerPid);
        const std::string start = std::to_string(startTime);
        execlp("pkexec", "pkexec", "--disable-internal-agent", stagedPath.c_str(),
               "--uid", uid.c_str(), "--owner-pid", owner.c_str(), "--owner-start-time", start.c_str(), static_cast<char*>(nullptr));
        _exit(errno == ENOENT ? 127 : 126);
    }

    const bool started = WaitForHelper(pid, errorMessage);
    Reap(pid);
    unlink(stagedPath.c_str());
    if (started) smu::log::LogInfo("Linux process helper started.");
    return started;
}

} // namespace smu::app

#endif

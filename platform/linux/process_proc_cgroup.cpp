#include "process_proc_cgroup.h"

#include "display_server.h"
#include "foreground_x11.h"
#include "../logging.h"

#include <algorithm>
#include <cerrno>
#include <charconv>
#include <cstring>
#include <dirent.h>
#include <fstream>
#include <optional>
#include <set>
#include <sstream>
#include <string>
#include <system_error>
#include <sys/stat.h>
#include <sys/types.h>
#include <signal.h>
#include <unistd.h>
#include <unordered_set>

namespace smu::platform::linux {
namespace {

bool IsPidToken(const std::string& token, PlatformPid* pid)
{
    if (token.empty()) {
        return false;
    }

    PlatformPid parsed = 0;
    const char* begin = token.data();
    const char* end = token.data() + token.size();
    auto [ptr, ec] = std::from_chars(begin, end, parsed);
    if (ec != std::errc() || ptr != end || parsed == 0) {
        return false;
    }

    if (pid) {
        *pid = parsed;
    }
    return true;
}

bool ProcessExists(PlatformPid pid)
{
    if (pid == 0) {
        return false;
    }

    errno = 0;
    return kill(static_cast<pid_t>(pid), 0) == 0 || errno == EPERM;
}

std::optional<std::string> ReadFirstLine(const std::string& path)
{
    std::ifstream file(path);
    if (!file) {
        return std::nullopt;
    }

    std::string line;
    std::getline(file, line);
    return line;
}

std::string Basename(const std::string& path)
{
    const std::size_t slash = path.find_last_of('/');
    if (slash == std::string::npos) {
        return path;
    }
    return path.substr(slash + 1);
}

std::optional<std::string> ReadExeBasename(PlatformPid pid)
{
    std::string linkPath = "/proc/" + std::to_string(pid) + "/exe";
    std::string buffer(4096, '\0');
    const ssize_t length = readlink(linkPath.c_str(), buffer.data(), buffer.size() - 1);
    if (length <= 0) {
        return std::nullopt;
    }
    buffer.resize(static_cast<std::size_t>(length));
    return Basename(buffer);
}

std::optional<PlatformPid> ParentPid(PlatformPid pid)
{
    std::ifstream statFile("/proc/" + std::to_string(pid) + "/stat");
    if (!statFile) {
        return std::nullopt;
    }

    std::string line;
    std::getline(statFile, line);
    const std::size_t endName = line.rfind(')');
    if (endName == std::string::npos || endName + 4 >= line.size()) {
        return std::nullopt;
    }

    std::istringstream fields(line.substr(endName + 2));
    char state = '\0';
    PlatformPid ppid = 0;
    fields >> state >> ppid;
    if (!fields) {
        return std::nullopt;
    }
    return ppid;
}

std::vector<PlatformPid> ProcessTree(PlatformPid rootPid)
{
    std::vector<PlatformPid> result;
    if (!ProcessExists(rootPid)) {
        return result;
    }

    result.push_back(rootPid);
    std::vector<PlatformPid> frontier{rootPid};

    while (!frontier.empty()) {
        const PlatformPid parent = frontier.back();
        frontier.pop_back();

        DIR* procDir = opendir("/proc");
        if (!procDir) {
            break;
        }

        while (dirent* entry = readdir(procDir)) {
            PlatformPid childPid = 0;
            if (!IsPidToken(entry->d_name, &childPid)) {
                continue;
            }

            auto ppid = ParentPid(childPid);
            if (ppid && *ppid == parent && std::find(result.begin(), result.end(), childPid) == result.end()) {
                result.push_back(childPid);
                frontier.push_back(childPid);
            }
        }

        closedir(procDir);
    }

    return result;
}

bool CgroupV2Available()
{
    struct stat st {};
    return stat("/sys/fs/cgroup/cgroup.controllers", &st) == 0;
}

std::optional<std::string> CgroupV2Path(PlatformPid pid)
{
    std::ifstream cgroupFile("/proc/" + std::to_string(pid) + "/cgroup");
    if (!cgroupFile) {
        return std::nullopt;
    }

    std::string line;
    while (std::getline(cgroupFile, line)) {
        if (line.rfind("0::/", 0) == 0) {
            return "/sys/fs/cgroup" + line.substr(3);
        }
    }

    return std::nullopt;
}

bool IsSafeAppCgroup(const std::string& path)
{
    return path.find("app-") != std::string::npos ||
           path.find("snap.") != std::string::npos ||
           path.find("/app.slice/") != std::string::npos;
}

bool WriteCgroupFreeze(const std::string& cgroupPath, bool freeze)
{
    const std::string freezePath = cgroupPath + "/cgroup.freeze";
    std::ofstream freezeFile(freezePath);
    if (!freezeFile) {
        return false;
    }

    freezeFile << (freeze ? "1" : "0");
    return !freezeFile.fail();
}

bool SetSuspended(PlatformPid pid, bool suspend)
{
    if (!ProcessExists(pid)) {
        smu::log::LogError("Linux process backend: PID " + std::to_string(pid) + " does not exist.");
        return false;
    }

    if (CgroupV2Available()) {
        auto cgroupPath = CgroupV2Path(pid);
        if (cgroupPath && IsSafeAppCgroup(*cgroupPath)) {
            if (WriteCgroupFreeze(*cgroupPath, suspend)) {
                return true;
            }

            smu::log::LogWarning(
                "Linux process backend: could not write cgroup.freeze for " + *cgroupPath +
                "; falling back to SIGSTOP/SIGCONT.");
        }
    }

    std::vector<PlatformPid> tree = ProcessTree(pid);
    if (tree.empty()) {
        tree.push_back(pid);
    }

    if (suspend) {
        std::reverse(tree.begin(), tree.end());
    }

    const int signalToSend = suspend ? SIGSTOP : SIGCONT;
    bool anySuccess = false;
    for (PlatformPid targetPid : tree) {
        if (kill(static_cast<pid_t>(targetPid), signalToSend) == 0) {
            anySuccess = true;
        } else {
            smu::log::LogWarning(
                "Linux process backend: failed to signal PID " + std::to_string(targetPid) +
                ": " + std::strerror(errno));
        }
    }

    return anySuccess;
}

} // namespace

bool ProcCgroupProcessBackend::init(std::string* errorMessage)
{
    if (errorMessage) {
        errorMessage->clear();
    }

    DIR* procDir = opendir("/proc");
    if (!procDir) {
        const std::string message = "Linux process backend failed to open /proc: " + std::string(std::strerror(errno));
        if (errorMessage) {
            *errorMessage = message;
        }
        smu::log::LogCritical(message);
        return false;
    }

    closedir(procDir);
    return true;
}

void ProcCgroupProcessBackend::shutdown() {}

std::optional<PlatformPid> ProcCgroupProcessBackend::findProcess(const std::string& executableName) const
{
    auto pids = findAllProcesses(executableName);
    if (pids.empty()) {
        return std::nullopt;
    }
    return pids.front();
}

std::vector<PlatformPid> ProcCgroupProcessBackend::findAllProcesses(const std::string& executableName) const
{
    std::vector<PlatformPid> pids;
    std::vector<std::string> executableTokens;

    std::istringstream input(executableName);
    std::string token;
    while (input >> token) {
        PlatformPid pid = 0;
        if (IsPidToken(token, &pid)) {
            if (ProcessExists(pid)) {
                pids.push_back(pid);
            }
        } else {
            executableTokens.push_back(token);
        }
    }

    if (executableTokens.empty()) {
        return pids;
    }

    DIR* procDir = opendir("/proc");
    if (!procDir) {
        smu::log::LogError("Linux process backend failed to scan /proc: " + std::string(std::strerror(errno)));
        return pids;
    }

    while (dirent* entry = readdir(procDir)) {
        PlatformPid pid = 0;
        if (!IsPidToken(entry->d_name, &pid)) {
            continue;
        }

        const auto comm = ReadFirstLine("/proc/" + std::to_string(pid) + "/comm");
        const auto exe = ReadExeBasename(pid);
        for (const std::string& executable : executableTokens) {
            if ((comm && *comm == executable) || (exe && *exe == executable)) {
                pids.push_back(pid);
                break;
            }
        }
    }

    closedir(procDir);
    std::sort(pids.begin(), pids.end());
    pids.erase(std::unique(pids.begin(), pids.end()), pids.end());
    return pids;
}

std::optional<PlatformPid> ProcCgroupProcessBackend::findMainProcess(const std::string& executableName) const
{
    std::vector<PlatformPid> pids = findAllProcesses(executableName);
    if (pids.empty()) {
        return std::nullopt;
    }

    if (pids.size() == 1) {
        return pids.front();
    }

    const std::set<PlatformPid> pidSet(pids.begin(), pids.end());
    for (PlatformPid pid : pids) {
        auto ppid = ParentPid(pid);
        if (!ppid || pidSet.find(*ppid) == pidSet.end()) {
            return pid;
        }
    }

    return *std::min_element(pids.begin(), pids.end());
}

bool ProcCgroupProcessBackend::suspend(PlatformPid pid)
{
    return SetSuspended(pid, true);
}

bool ProcCgroupProcessBackend::resume(PlatformPid pid)
{
    return SetSuspended(pid, false);
}

bool ProcCgroupProcessBackend::isForegroundProcess(PlatformPid pid) const
{
    const DisplayServer displayServer = DetectDisplayServer();
    if (displayServer == DisplayServer::Wayland) {
        return false;
    }

    if (displayServer != DisplayServer::X11) {
        return false;
    }

    std::vector<PlatformPid> candidates = ProcessTree(pid);
    if (candidates.empty()) {
        candidates.push_back(pid);
    }

    std::string error;
    const bool foreground = IsX11ForegroundProcess(candidates, &error);
    if (!foreground && !error.empty()) {
        smu::log::LogWarning("Linux foreground detection: " + error);
    }
    return foreground;
}

std::shared_ptr<ProcessBackend> CreateProcCgroupProcessBackend()
{
    return std::make_shared<ProcCgroupProcessBackend>();
}

} // namespace smu::platform::linux

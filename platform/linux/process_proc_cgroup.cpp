#include "process_proc_cgroup.h"

#include "display_server.h"
#include "foreground_x11.h"
#include "../logging.h"

#include <algorithm>
#include <cerrno>
#include <charconv>
#include <cctype>
#include <chrono>
#include <condition_variable>
#include <cstring>
#include <dirent.h>
#include <fstream>
#include <mutex>
#include <optional>
#include <set>
#include <sstream>
#include <string>
#include <system_error>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/types.h>
#include <sys/un.h>
#include <signal.h>
#include <unistd.h>
#include <unordered_set>

namespace smu::platform::linux {
namespace {

std::mutex g_freezeHelperAuthorizationMutex;
std::condition_variable g_freezeHelperAuthorizationCv;
bool g_freezeHelperAuthorizationPending = false;
bool g_freezeHelperAuthorizationResolved = false;
bool g_freezeHelperAuthorizationApproved = false;

std::string FreezeHelperSocketPath()
{
    return "/tmp/smu-processhelper-" + std::to_string(getuid()) + ".sock";
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

bool SendFreezeHelperCommand(const std::string& command, std::string* response)
{
    const std::string socketPath = FreezeHelperSocketPath();
    const int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (fd < 0) {
        return false;
    }

    timeval timeout{};
    timeout.tv_sec = 2;
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout));
    setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, sizeof(timeout));

    sockaddr_un address{};
    address.sun_family = AF_UNIX;
    if (socketPath.size() >= sizeof(address.sun_path)) {
        close(fd);
        return false;
    }
    std::snprintf(address.sun_path, sizeof(address.sun_path), "%s", socketPath.c_str());

    if (connect(fd, reinterpret_cast<sockaddr*>(&address), sizeof(address)) != 0 || !IsRootPeer(fd)) {
        close(fd);
        return false;
    }

    const std::string request = command + "\n";
    std::size_t sentTotal = 0;
    while (sentTotal < request.size()) {
        const ssize_t sent = send(
            fd,
            request.data() + sentTotal,
            request.size() - sentTotal,
            MSG_NOSIGNAL);
        if (sent < 0 && errno == EINTR) {
            continue;
        }
        if (sent <= 0) {
            close(fd);
            return false;
        }
        sentTotal += static_cast<std::size_t>(sent);
    }

    std::string line;
    char buffer[256];
    while (line.size() < 1024) {
        const ssize_t received = recv(fd, buffer, sizeof(buffer), 0);
        if (received < 0 && errno == EINTR) {
            continue;
        }
        if (received <= 0) {
            break;
        }
        line.append(buffer, static_cast<std::size_t>(received));
        const std::size_t newline = line.find('\n');
        if (newline != std::string::npos) {
            line.resize(newline);
            break;
        }
    }
    close(fd);

    if (response) {
        *response = line;
    }
    return !line.empty();
}

bool PrivilegedFreezeHelperAvailable()
{
    std::string response;
    return SendFreezeHelperCommand("ping", &response) && response == "PONG";
}

bool SetSuspendedWithPrivilegedHelper(PlatformPid pid, bool suspend)
{
    std::string response;
    const std::string command =
        "freeze " + std::to_string(pid) + (suspend ? " 1" : " 0");
    if (!SendFreezeHelperCommand(command, &response)) {
        return false;
    }
    if (response == "OK") {
        return true;
    }
    smu::log::LogWarning(
        "Linux process backend: privileged freeze helper failed for PID " +
        std::to_string(pid) + ": " + response);
    return false;
}

bool WaitForPrivilegedFreezeHelperAuthorization()
{
    std::unique_lock<std::mutex> lock(g_freezeHelperAuthorizationMutex);
    if (g_freezeHelperAuthorizationResolved) {
        return g_freezeHelperAuthorizationApproved;
    }

    g_freezeHelperAuthorizationPending = true;
    const bool resolved = g_freezeHelperAuthorizationCv.wait_for(
        lock,
        std::chrono::minutes(2),
        [] { return g_freezeHelperAuthorizationResolved; });
    if (!resolved) {
        g_freezeHelperAuthorizationPending = false;
        return false;
    }
    return g_freezeHelperAuthorizationApproved;
}

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

bool EqualsIgnoreCase(const std::string& left, const std::string& right)
{
    return left.size() == right.size() &&
        std::equal(
            left.begin(),
            left.end(),
            right.begin(),
            [](unsigned char a, unsigned char b) {
                return std::tolower(a) == std::tolower(b);
            });
}

bool IsSoberAlias(const std::string& executable)
{
    return EqualsIgnoreCase(executable, "sober");
}

bool IsMainAlias(const std::string& executable)
{
    return EqualsIgnoreCase(executable, "main");
}

std::optional<std::string> ReadExeBasename(PlatformPid pid)
{
    std::string linkPath = "/proc/" + std::to_string(pid) + "/exe";

    std::string buffer(4096, '\0');

    const ssize_t length =
        readlink(linkPath.c_str(), buffer.data(), buffer.size() - 1);

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

            if (ppid &&
                *ppid == parent &&
                std::find(result.begin(), result.end(), childPid) == result.end()) {

                result.push_back(childPid);
                frontier.push_back(childPid);
            }
        }

        closedir(procDir);
    }

    return result;
}

bool ProcessHasAncestor(PlatformPid pid, PlatformPid ancestorPid)
{
    if (pid == 0 || ancestorPid == 0) {
        return false;
    }

    std::unordered_set<PlatformPid> seen;
    PlatformPid current = pid;

    for (int depth = 0; depth < 1024; ++depth) {
        if (current == ancestorPid) {
            return true;
        }

        if (!seen.insert(current).second) {
            return false;
        }

        auto parent = ParentPid(current);
        if (!parent || *parent == 0 || *parent == current) {
            return false;
        }

        current = *parent;
    }

    return false;
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

std::optional<PlatformPid> TracerPid(PlatformPid pid)
{
    std::ifstream statusFile("/proc/" + std::to_string(pid) + "/status");

    if (!statusFile) {
        return std::nullopt;
    }

    std::string line;
    while (std::getline(statusFile, line)) {
        if (line.rfind("TracerPid:", 0) != 0) {
            continue;
        }

        std::istringstream fields(line.substr(10));
        PlatformPid tracerPid = 0;
        fields >> tracerPid;

        if (!fields) {
            return std::nullopt;
        }

        return tracerPid;
    }

    return std::nullopt;
}

bool IsPtraced(PlatformPid pid)
{
    const auto tracerPid = TracerPid(pid);
    return tracerPid && *tracerPid != 0;
}

std::vector<PlatformPid> ReadNamespacePids(PlatformPid pid)
{
    std::ifstream statusFile("/proc/" + std::to_string(pid) + "/status");
    std::vector<PlatformPid> pids;

    if (!statusFile) {
        return pids;
    }

    std::string line;
    while (std::getline(statusFile, line)) {
        if (line.rfind("NSpid:", 0) != 0) {
            continue;
        }

        std::istringstream fields(line.substr(6));
        PlatformPid namespacePid = 0;
        while (fields >> namespacePid) {
            pids.push_back(namespacePid);
        }

        break;
    }

    return pids;
}

std::optional<PlatformPid> FindHostPidForNamespacePidInTargetCgroup(
    PlatformPid namespacePid,
    const std::vector<PlatformPid>& targetPids)
{
    if (namespacePid == 0) {
        return std::nullopt;
    }

    std::set<std::string> targetCgroups;
    for (PlatformPid targetPid : targetPids) {
        auto cgroupPath = CgroupV2Path(targetPid);
        if (cgroupPath) {
            targetCgroups.insert(*cgroupPath);
        }
    }

    if (targetCgroups.empty()) {
        return std::nullopt;
    }

    DIR* procDir = opendir("/proc");
    if (!procDir) {
        return std::nullopt;
    }

    std::optional<PlatformPid> hostPid;

    while (dirent* entry = readdir(procDir)) {
        PlatformPid candidatePid = 0;
        if (!IsPidToken(entry->d_name, &candidatePid) || candidatePid == namespacePid) {
            continue;
        }

        auto cgroupPath = CgroupV2Path(candidatePid);
        if (!cgroupPath || targetCgroups.find(*cgroupPath) == targetCgroups.end()) {
            continue;
        }

        const std::vector<PlatformPid> namespacePids = ReadNamespacePids(candidatePid);
        if (std::find(namespacePids.begin(), namespacePids.end(), namespacePid) != namespacePids.end()) {
            hostPid = candidatePid;
            break;
        }
    }

    closedir(procDir);
    return hostPid;
}

bool ProcessMatchesExecutable(
    PlatformPid pid,
    const std::optional<std::string>& comm,
    const std::optional<std::string>& exe,
    const std::string& executable)
{
    if ((comm && *comm == executable) ||
        (exe && *exe == executable)) {

        return true;
    }

    if (IsSoberAlias(executable)) {
        if ((comm && EqualsIgnoreCase(*comm, "sober")) ||
            (exe && EqualsIgnoreCase(*exe, "sober"))) {

            return true;
        }

        if (comm && EqualsIgnoreCase(*comm, "main") &&
            exe && EqualsIgnoreCase(*exe, "sober")) {

            return true;
        }
    }

    // Sober's game process is exposed as "Main" while its executable remains
    // /app/bin/sober. Accept the lowercase selector as well as the displayed
    // process name, but keep the alias scoped to Sober when the executable is
    // available.
    if (IsMainAlias(executable) &&
        comm && EqualsIgnoreCase(*comm, "main") &&
        (!exe || EqualsIgnoreCase(*exe, "sober"))) {

        return true;
    }

    return false;
}

bool IsSafeAppCgroup(const std::string& path)
{
    return path.find("app-") != std::string::npos ||
           path.find("snap.") != std::string::npos ||
           path.find("/app.slice/") != std::string::npos;
}

bool IsSmuFreezeCgroup(const std::string& path)
{
    static constexpr const char* kSuffix = "/smu_freeze";
    const std::size_t suffixLength = std::strlen(kSuffix);

    return path.size() >= suffixLength &&
        path.compare(path.size() - suffixLength, suffixLength, kSuffix) == 0;
}

bool WriteTextFile(const std::string& path, const std::string& text, int* errorCode = nullptr)
{
    errno = 0;
    std::ofstream file(path);

    if (!file) {
        if (errorCode) {
            *errorCode = errno;
        }
        return false;
    }

    file << text;
    file.flush();

    if (file.fail()) {
        if (errorCode) {
            *errorCode = errno;
        }
        return false;
    }

    if (errorCode) {
        *errorCode = 0;
    }
    return true;
}

bool CreateFrozenChildCgroup(
    const std::string& currentCgroup,
    PlatformPid pid,
    bool freeze,
    int* errorCode = nullptr) // original freeze code here: https://github.com/3443e/sober-freeze
{
    if (errorCode) {
        *errorCode = 0;
    }
    static constexpr const char* kFreezeName = "smu_freeze";

    std::string parentCgroup;
    std::string childCgroup;

    const std::string suffix =
        std::string("/") + kFreezeName;

    // already inside freeze group
    if (currentCgroup.size() >= suffix.size() &&
        currentCgroup.compare(
            currentCgroup.size() - suffix.size(),
            suffix.size(),
            suffix) == 0) {

        childCgroup = currentCgroup;

        parentCgroup =
            currentCgroup.substr(
                0,
                currentCgroup.size() - suffix.size());

    } else {

        parentCgroup = currentCgroup;
        childCgroup = currentCgroup + suffix;
    }

    const std::string childProcs =
        childCgroup + "/cgroup.procs";

    const std::string childFreeze =
        childCgroup + "/cgroup.freeze";

    if (freeze) {

        if (mkdir(childCgroup.c_str(), 0755) != 0 && errno != EEXIST) {
            const int failure = errno;
            if (errorCode) {
                *errorCode = failure;
            }
            smu::log::LogWarning(
                "Failed to create frozen child cgroup: " +
                std::string(std::strerror(failure)) + ".");
            return false;
        }

        // move process into child cgroup
        int writeError = 0;
        if (!WriteTextFile(
                childProcs,
                std::to_string(pid),
                &writeError)) {

            if (errorCode) {
                *errorCode = writeError;
            }
            smu::log::LogWarning(
                "Failed to move PID into frozen cgroup: " +
                std::string(std::strerror(writeError)) + ".");

            return false;
        }

        // freeze child cgroup
        writeError = 0;
        if (!WriteTextFile(
                childFreeze,
                "1",
                &writeError)) {

            if (errorCode) {
                *errorCode = writeError;
            }
            smu::log::LogWarning(
                "Failed to freeze cgroup: " +
                std::string(std::strerror(writeError)) + ".");

            return false;
        }

        return true;
    }

    // thaw before moving process back
    int writeError = 0;
    if (!WriteTextFile(
            childFreeze,
            "0",
            &writeError)) {

        if (errorCode) {
            *errorCode = writeError;
        }
        smu::log::LogWarning(
            "Failed to unfreeze cgroup: " +
            std::string(std::strerror(writeError)) + ".");

        return false;
    }

    const std::string parentProcs =
        parentCgroup + "/cgroup.procs";

    // move process back to parent
    writeError = 0;
    if (!WriteTextFile(
            parentProcs,
            std::to_string(pid),
            &writeError)) {

        if (errorCode) {
            *errorCode = writeError;
        }
        smu::log::LogWarning(
            "Failed to move PID back to parent cgroup: " +
            std::string(std::strerror(writeError)) + ".");

        return false;
    }

    rmdir(childCgroup.c_str());

    return true;
}

bool SetSuspended(PlatformPid pid, bool suspend)
{
    if (!ProcessExists(pid)) {
        smu::log::LogError(
            "Linux process backend: PID " +
            std::to_string(pid) +
            " does not exist.");

        return false;
    }

    const bool ptraced = IsPtraced(pid);

    if (CgroupV2Available()) {
        auto cgroupPath = CgroupV2Path(pid);

        // Sober's anti-tamper can ptrace the game process and intercept SIGSTOP.
        // In that case, use the cgroup v2 freezer regardless of the distro's
        // cgroup naming convention. Also always allow our own child cgroup so a
        // process frozen this way can be resumed later.
        const bool shouldUseCgroup =
            cgroupPath &&
            (ptraced || IsSafeAppCgroup(*cgroupPath) || IsSmuFreezeCgroup(*cgroupPath));

        if (shouldUseCgroup) {
            int cgroupError = 0;
            if (CreateFrozenChildCgroup(
                    *cgroupPath,
                    pid,
                    suspend,
                    &cgroupError)) {

                return true;
            }

            const bool permissionDenied = cgroupError == EACCES || cgroupError == EPERM;
            const bool helperEligible = ptraced || IsSmuFreezeCgroup(*cgroupPath);
            if (permissionDenied && helperEligible) {
                if (SetSuspendedWithPrivilegedHelper(pid, suspend)) {
                    return true;
                }

                if (!PrivilegedFreezeHelperAvailable() &&
                    WaitForPrivilegedFreezeHelperAuthorization() &&
                    SetSuspendedWithPrivilegedHelper(pid, suspend)) {
                    return true;
                }
            }

            smu::log::LogWarning(
                "Linux process backend: could not manipulate frozen child cgroup for " +
                *cgroupPath +
                (ptraced
                    ? "; refusing SIGSTOP fallback for ptraced process."
                    : "; falling back to SIGSTOP/SIGCONT."));

            if (ptraced && suspend) {
                return false;
            }
        }
    } else if (ptraced && suspend) {
        smu::log::LogWarning(
            "Linux process backend: PID " +
            std::to_string(pid) +
            " is ptraced and cgroup v2 is unavailable; refusing unreliable SIGSTOP fallback.");

        return false;
    }

    std::vector<PlatformPid> tree = ProcessTree(pid);

    if (tree.empty()) {
        tree.push_back(pid);
    }

    if (suspend) {
        std::reverse(tree.begin(), tree.end());
    }

    const int signalToSend =
        suspend ? SIGSTOP : SIGCONT;

    bool anySuccess = false;

    for (PlatformPid targetPid : tree) {

        if (kill(
                static_cast<pid_t>(targetPid),
                signalToSend) == 0) {

            anySuccess = true;

        } else {

            smu::log::LogWarning(
                "Linux process backend: failed to signal PID " +
                std::to_string(targetPid) +
                ": " +
                std::strerror(errno));
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

        const std::string message =
            "Linux process backend failed to open /proc: " +
            std::string(std::strerror(errno));

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

std::optional<PlatformPid>
ProcCgroupProcessBackend::findProcess(
    const std::string& executableName) const
{
    auto pids = findAllProcesses(executableName);

    if (pids.empty()) {
        return std::nullopt;
    }

    return pids.front();
}

std::vector<PlatformPid>
ProcCgroupProcessBackend::findAllProcesses(
    const std::string& executableName) const
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

        smu::log::LogError(
            "Linux process backend failed to scan /proc: " +
            std::string(std::strerror(errno)));

        return pids;
    }

    while (dirent* entry = readdir(procDir)) {

        PlatformPid pid = 0;

        if (!IsPidToken(entry->d_name, &pid)) {
            continue;
        }

        const auto comm =
            ReadFirstLine(
                "/proc/" +
                std::to_string(pid) +
                "/comm");

	

        const auto exe =
            ReadExeBasename(pid);

        for (const std::string& executable : executableTokens) {

            if (ProcessMatchesExecutable(pid, comm, exe, executable)) {

                pids.push_back(pid);
                break;
            }
        }
    }

    closedir(procDir);

    std::sort(pids.begin(), pids.end());

    pids.erase(
        std::unique(pids.begin(), pids.end()),
        pids.end());

    return pids;
}

std::optional<PlatformPid>
ProcCgroupProcessBackend::findMainProcess(
    const std::string& executableName) const
{
    std::vector<PlatformPid> pids = findAllProcesses(executableName);
    if (pids.empty()) {
        return std::nullopt;
    }

    // Sober now has a launcher plus helper processes whose names are also
    // "sober". The actual game process is the one reported as "Main"; choose
    // it before applying the generic process-tree-root heuristic.
    if (IsSoberAlias(executableName) || IsMainAlias(executableName)) {
        for (PlatformPid pid : pids) {
            const auto comm = ReadFirstLine(
                "/proc/" + std::to_string(pid) + "/comm");
            const auto exe = ReadExeBasename(pid);

            if (comm && EqualsIgnoreCase(*comm, "main") &&
                exe && EqualsIgnoreCase(*exe, "sober")) {

                return pid;
            }
        }
    }

    if (pids.size() == 1) {
        return pids.front();
    }

    const std::set<PlatformPid> pidSet(
        pids.begin(),
        pids.end());

    for (PlatformPid pid : pids) {

        auto ppid = ParentPid(pid);

        if (!ppid ||
            pidSet.find(*ppid) == pidSet.end()) {

            return pid;
        }
    }

    return *std::min_element(
        pids.begin(),
        pids.end());
}

bool ProcCgroupProcessBackend::suspend(PlatformPid pid)
{
    return SetSuspended(pid, true);
}

bool ProcCgroupProcessBackend::resume(PlatformPid pid)
{
    return SetSuspended(pid, false);
}

bool ProcCgroupProcessBackend::isForegroundProcess(
    PlatformPid pid) const
{
    const DisplayServer displayServer =
        DetectDisplayServer();

    if (displayServer == DisplayServer::Wayland) {
        return false;
    }

    if (displayServer != DisplayServer::X11) {
        return false;
    }

    std::vector<PlatformPid> candidates =
        ProcessTree(pid);

    if (candidates.empty()) {
        candidates.push_back(pid);
    }

    auto foregroundPid = GetX11ForegroundProcess();
    if (!foregroundPid) {
        return false;
    }

    std::vector<PlatformPid> foregroundCandidates{*foregroundPid};
    if (auto hostForegroundPid = FindHostPidForNamespacePidInTargetCgroup(*foregroundPid, candidates)) {
        foregroundCandidates.push_back(*hostForegroundPid);
    }

    std::sort(foregroundCandidates.begin(), foregroundCandidates.end());
    foregroundCandidates.erase(
        std::unique(foregroundCandidates.begin(), foregroundCandidates.end()),
        foregroundCandidates.end());

    for (PlatformPid candidateForegroundPid : foregroundCandidates) {
        if (std::find(candidates.begin(), candidates.end(), candidateForegroundPid) != candidates.end()) {
            return true;
        }

        if (ProcessHasAncestor(pid, candidateForegroundPid)) {
            return true;
        }
    }

    return false;
}

bool IsPrivilegedFreezeHelperAuthorizationPending()
{
    std::lock_guard<std::mutex> lock(g_freezeHelperAuthorizationMutex);
    return g_freezeHelperAuthorizationPending && !g_freezeHelperAuthorizationResolved;
}

void ResolvePrivilegedFreezeHelperAuthorization(bool approved)
{
    {
        std::lock_guard<std::mutex> lock(g_freezeHelperAuthorizationMutex);
        g_freezeHelperAuthorizationApproved = approved;
        g_freezeHelperAuthorizationResolved = true;
        g_freezeHelperAuthorizationPending = false;
    }
    g_freezeHelperAuthorizationCv.notify_all();
}

void ResetPrivilegedFreezeHelperAuthorization()
{
    {
        std::lock_guard<std::mutex> lock(g_freezeHelperAuthorizationMutex);
        g_freezeHelperAuthorizationPending = false;
        g_freezeHelperAuthorizationResolved = false;
        g_freezeHelperAuthorizationApproved = false;
    }
    g_freezeHelperAuthorizationCv.notify_all();
}

std::shared_ptr<ProcessBackend>
CreateProcCgroupProcessBackend()
{
    return std::make_shared<ProcCgroupProcessBackend>();
}

} // namespace smu::platform::linux

#if defined(__linux__)

#include "updater.h"

#include <algorithm>
#include <cerrno>
#include <cctype>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <string>
#include <sys/wait.h>
#include <unistd.h>
#include <vector>

namespace smu::updater::detail {
namespace {

std::string Lower(std::string value)
{
    std::transform(value.begin(), value.end(), value.begin(), [](unsigned char ch) {
        return static_cast<char>(std::tolower(ch));
    });
    return value;
}

bool Contains(const std::string& value, const char* needle)
{
    return value.find(needle) != std::string::npos;
}

bool EndsWith(const std::string& value, const char* suffix)
{
    const std::string suffixText(suffix);
    return value.size() >= suffixText.size() &&
        value.compare(value.size() - suffixText.size(), suffixText.size(), suffixText) == 0;
}

std::string WaitStatusMessage(int status)
{
    if (WIFEXITED(status)) {
        return "curl exited with code " + std::to_string(WEXITSTATUS(status)) + ".";
    }
    if (WIFSIGNALED(status)) {
        return "curl was terminated by signal " + std::to_string(WTERMSIG(status)) + ".";
    }
    return "curl did not complete successfully.";
}

bool ReadAllFromFd(int fd, std::string& output)
{
    char buffer[8192];
    for (;;) {
        const ssize_t bytesRead = read(fd, buffer, sizeof(buffer));
        if (bytesRead > 0) {
            output.append(buffer, static_cast<std::size_t>(bytesRead));
            continue;
        }
        if (bytesRead == 0) {
            return true;
        }
        if (errno == EINTR) {
            continue;
        }
        return false;
    }
}

bool RunCurlToStdout(const std::string& url, std::string& output, std::string* errorMessage)
{
    output.clear();
    int pipeFds[2] {};
    if (pipe(pipeFds) != 0) {
        if (errorMessage) {
            *errorMessage = std::string("Failed to create curl pipe: ") + std::strerror(errno);
        }
        return false;
    }

    const pid_t pid = fork();
    if (pid < 0) {
        close(pipeFds[0]);
        close(pipeFds[1]);
        if (errorMessage) {
            *errorMessage = std::string("Failed to fork curl: ") + std::strerror(errno);
        }
        return false;
    }

    if (pid == 0) {
        close(pipeFds[0]);
        dup2(pipeFds[1], STDOUT_FILENO);
        close(pipeFds[1]);
        execlp("curl",
            "curl",
            "-fsSL",
            "--max-time",
            "15",
            "-H",
            "User-Agent: Spencer-Macro-Utilities-Updater",
            "-H",
            "Accept: application/vnd.github+json",
            url.c_str(),
            static_cast<char*>(nullptr));
        _exit(errno == ENOENT ? 127 : 126);
    }

    close(pipeFds[1]);
    const bool readOk = ReadAllFromFd(pipeFds[0], output);
    close(pipeFds[0]);

    int status = 0;
    if (waitpid(pid, &status, 0) < 0) {
        if (errorMessage) {
            *errorMessage = std::string("Failed waiting for curl: ") + std::strerror(errno);
        }
        return false;
    }

    if (!readOk) {
        if (errorMessage) {
            *errorMessage = std::string("Failed reading curl output: ") + std::strerror(errno);
        }
        return false;
    }

    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        if (errorMessage) {
            *errorMessage = "Update HTTP request failed; " + WaitStatusMessage(status);
        }
        return false;
    }

    return !output.empty();
}

bool RunCurlToFile(const std::string& url, const std::filesystem::path& destination, std::string* errorMessage)
{
    const pid_t pid = fork();
    if (pid < 0) {
        if (errorMessage) {
            *errorMessage = std::string("Failed to fork curl: ") + std::strerror(errno);
        }
        return false;
    }

    if (pid == 0) {
        execlp("curl",
            "curl",
            "-fsSL",
            "--max-time",
            "60",
            "-H",
            "User-Agent: Spencer-Macro-Utilities-Updater",
            "-L",
            "-o",
            destination.c_str(),
            url.c_str(),
            static_cast<char*>(nullptr));
        _exit(errno == ENOENT ? 127 : 126);
    }

    int status = 0;
    if (waitpid(pid, &status, 0) < 0) {
        if (errorMessage) {
            *errorMessage = std::string("Failed waiting for curl: ") + std::strerror(errno);
        }
        return false;
    }

    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        if (errorMessage) {
            *errorMessage = "Update download failed; " + WaitStatusMessage(status);
        }
        return false;
    }

    return true;
}

} // namespace

bool HttpGetString(const std::string& url, std::string& output, std::string* errorMessage)
{
    return RunCurlToStdout(url, output, errorMessage);
}

bool DownloadUrlToFile(const std::string& url, const std::filesystem::path& destination, std::string* errorMessage)
{
    return RunCurlToFile(url, destination, errorMessage);
}

bool DownloadUrlToMemory(const std::string& url, std::vector<char>& data, std::string* errorMessage)
{
    data.clear();
    char pathTemplate[] = "/tmp/smu-update-XXXXXX";
    const int fd = mkstemp(pathTemplate);
    if (fd < 0) {
        if (errorMessage) {
            *errorMessage = std::string("Failed to create temporary update file: ") + std::strerror(errno);
        }
        return false;
    }
    close(fd);

    const std::filesystem::path tempPath(pathTemplate);
    const bool downloaded = DownloadUrlToFile(url, tempPath, errorMessage);
    if (!downloaded) {
        std::filesystem::remove(tempPath);
        return false;
    }

    std::ifstream file(tempPath, std::ios::binary);
    if (!file) {
        std::filesystem::remove(tempPath);
        if (errorMessage) {
            *errorMessage = "Failed to open downloaded update file.";
        }
        return false;
    }

    data.assign(std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>());
    std::filesystem::remove(tempPath);
    return !data.empty();
}

int ScoreAssetForCurrentPlatform(const ReleaseAsset& asset)
{
    const std::string name = Lower(asset.name);
    if (Contains(name, "windows") || Contains(name, "win64") || Contains(name, "win32") || EndsWith(name, ".exe")) {
        return 0;
    }

    const bool hasLinuxMarker =
        Contains(name, "linux") ||
        Contains(name, "appimage") ||
        Contains(name, "x86_64") ||
        Contains(name, "amd64");
    const bool packageLooksLinux =
        EndsWith(name, ".appimage") ||
        EndsWith(name, ".tar.gz") ||
        EndsWith(name, ".tgz") ||
        (EndsWith(name, ".zip") && hasLinuxMarker);
    if (!packageLooksLinux) {
        return 0;
    }

    int score = 0;
    if (Contains(name, "linux")) score += 50;
    if (Contains(name, "x86_64") || Contains(name, "amd64")) score += 10;
    if (EndsWith(name, ".appimage")) score += 40;
    if (EndsWith(name, ".tar.gz") || EndsWith(name, ".tgz")) score += 25;
    if (EndsWith(name, ".zip")) score += 10;
    if (Contains(name, "spencer") || Contains(name, "macro") || Contains(name, "suspend")) score += 8;
    return score;
}

bool PlatformAutoApplySupported()
{
    return false;
}

bool ApplyUpdateFromAsset(const ReleaseAsset&, const std::string&, const std::string&, std::string* errorMessage)
{
    if (errorMessage) {
        *errorMessage = "Update check is available on Linux, but auto-apply is not implemented until Linux release packaging is finalized.";
    }
    return false;
}

} // namespace smu::updater::detail

#endif

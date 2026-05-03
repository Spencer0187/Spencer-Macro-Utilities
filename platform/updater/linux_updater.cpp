#if defined(__linux__)

#include "updater.h"

#include <algorithm>
#include <cerrno>
#include <cctype>
#include <cstring>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <string>
#include <system_error>
#include <sys/stat.h>
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

    // Linux auto-apply is AppImage-only. Do not select tarballs or ZIPs here.
    if (!EndsWith(name, ".appimage")) {
        return 0;
    }

#if defined(__x86_64__) || defined(__amd64__)
    if (Contains(name, "aarch64") || Contains(name, "arm64")) {
        return 0;
    }
#elif defined(__aarch64__)
    if (Contains(name, "x86_64") || Contains(name, "amd64")) {
        return 0;
    }
#endif

    // Return a flat score so SelectUpdateAsset() keeps the first matching AppImage.
    return 100;
}


bool PlatformAutoApplySupported()
{
    const char* appImagePath = std::getenv("APPIMAGE");
    if (!appImagePath || !*appImagePath) {
        return false;
    }

    std::error_code ec;
    return std::filesystem::exists(std::filesystem::path(appImagePath), ec) && !ec;
}


bool ApplyUpdateFromAsset(const ReleaseAsset& asset, const std::string&, const std::string&, std::string* errorMessage)
{
    const std::string assetName = Lower(asset.name);
    if (!EndsWith(assetName, ".appimage")) {
        if (errorMessage) {
            *errorMessage = "Linux auto-update only supports AppImage release assets.";
        }
        return false;
    }

    const char* appImageEnv = std::getenv("APPIMAGE");
    if (!appImageEnv || !*appImageEnv) {
        if (errorMessage) {
            *errorMessage = "Linux auto-update requires running SMU from an AppImage. APPIMAGE is not set.";
        }
        return false;
    }

    std::error_code ec;
    const std::filesystem::path currentAppImage = std::filesystem::absolute(std::filesystem::path(appImageEnv), ec);
    if (ec || currentAppImage.empty()) {
        if (errorMessage) {
            *errorMessage = "Could not resolve the current AppImage path.";
        }
        return false;
    }

    if (!std::filesystem::exists(currentAppImage, ec) || ec) {
        if (errorMessage) {
            *errorMessage = "Current AppImage path does not exist: " + currentAppImage.string();
        }
        return false;
    }

    const std::filesystem::path appImageDir = currentAppImage.parent_path();
    if (appImageDir.empty() || access(appImageDir.c_str(), W_OK) != 0) {
        if (errorMessage) {
            *errorMessage = "Cannot update AppImage because its folder is not writable: " + appImageDir.string();
        }
        return false;
    }

    const std::filesystem::path tempAppImage =
        appImageDir / ("." + currentAppImage.filename().string() + ".update-" + std::to_string(getpid()) + ".tmp");

    std::filesystem::remove(tempAppImage, ec);

    if (!DownloadUrlToFile(asset.downloadUrl, tempAppImage, errorMessage)) {
        std::filesystem::remove(tempAppImage, ec);
        return false;
    }

    if (!std::filesystem::exists(tempAppImage, ec) || ec || std::filesystem::file_size(tempAppImage, ec) == 0 || ec) {
        std::filesystem::remove(tempAppImage, ec);
        if (errorMessage) {
            *errorMessage = "Downloaded AppImage was missing or empty.";
        }
        return false;
    }

    if (chmod(tempAppImage.c_str(), 0755) != 0) {
        std::filesystem::remove(tempAppImage, ec);
        if (errorMessage) {
            *errorMessage = std::string("Failed to make downloaded AppImage executable: ") + std::strerror(errno);
        }
        return false;
    }

    char scriptTemplate[] = "/tmp/smu-appimage-updater-XXXXXX";
    const int scriptFd = mkstemp(scriptTemplate);
    if (scriptFd < 0) {
        std::filesystem::remove(tempAppImage, ec);
        if (errorMessage) {
            *errorMessage = std::string("Failed to create updater script: ") + std::strerror(errno);
        }
        return false;
    }
    close(scriptFd);

    const std::filesystem::path scriptPath(scriptTemplate);
    {
        std::ofstream script(scriptPath, std::ios::binary | std::ios::trunc);
        if (!script) {
            std::filesystem::remove(tempAppImage, ec);
            std::filesystem::remove(scriptPath, ec);
            if (errorMessage) {
                *errorMessage = "Failed to open updater script for writing.";
            }
            return false;
        }

        script <<
            "#!/bin/sh\n"
            "set -u\n"
            "\n"
            "OLD_APPIMAGE=\"$1\"\n"
            "NEW_APPIMAGE=\"$2\"\n"
            "OLD_PID=\"$3\"\n"
            "LOG_FILE=\"${TMPDIR:-/tmp}/smu-appimage-updater.log\"\n"
            "\n"
            "echo \"SMU AppImage updater started\" > \"$LOG_FILE\"\n"
            "echo \"Old AppImage: $OLD_APPIMAGE\" >> \"$LOG_FILE\"\n"
            "echo \"New AppImage: $NEW_APPIMAGE\" >> \"$LOG_FILE\"\n"
            "echo \"Old PID: $OLD_PID\" >> \"$LOG_FILE\"\n"
            "\n"
            "i=0\n"
            "while kill -0 \"$OLD_PID\" 2>/dev/null; do\n"
            "    i=$((i + 1))\n"
            "    if [ \"$i\" -gt 100 ]; then\n"
            "        echo \"Timed out waiting for old SMU process to exit\" >> \"$LOG_FILE\"\n"
            "        exit 1\n"
            "    fi\n"
            "    sleep 0.1\n"
            "done\n"
            "\n"
            "sleep 0.5\n"
            "\n"
            "chmod 755 \"$NEW_APPIMAGE\" 2>> \"$LOG_FILE\" || exit 1\n"
            "\n"
            "i=0\n"
            "while true; do\n"
            "    if mv -f -- \"$NEW_APPIMAGE\" \"$OLD_APPIMAGE\" 2>> \"$LOG_FILE\"; then\n"
            "        break\n"
            "    fi\n"
            "\n"
            "    i=$((i + 1))\n"
            "    if [ \"$i\" -gt 40 ]; then\n"
            "        echo \"Failed to replace old AppImage\" >> \"$LOG_FILE\"\n"
            "        exit 1\n"
            "    fi\n"
            "\n"
            "    sleep 0.25\n"
            "done\n"
            "\n"
            "nohup \"$OLD_APPIMAGE\" >/dev/null 2>&1 &\n"
            "rm -f -- \"$0\"\n"
            "exit 0\n";
    }

    if (chmod(scriptPath.c_str(), 0700) != 0) {
        std::filesystem::remove(tempAppImage, ec);
        std::filesystem::remove(scriptPath, ec);
        if (errorMessage) {
            *errorMessage = std::string("Failed to make updater script executable: ") + std::strerror(errno);
        }
        return false;
    }

    const pid_t child = fork();
    if (child < 0) {
        std::filesystem::remove(tempAppImage, ec);
        std::filesystem::remove(scriptPath, ec);
        if (errorMessage) {
            *errorMessage = std::string("Failed to launch updater script: ") + std::strerror(errno);
        }
        return false;
    }

    if (child == 0) {
        setsid();

        const std::string oldPid = std::to_string(getppid());
        execl(
            "/bin/sh",
            "sh",
            scriptPath.c_str(),
            currentAppImage.c_str(),
            tempAppImage.c_str(),
            oldPid.c_str(),
            static_cast<char*>(nullptr));

        _exit(errno == ENOENT ? 127 : 126);
    }

    std::exit(0);
    return true;
}

} // namespace smu::updater::detail

#endif

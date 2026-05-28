#if defined(__APPLE__)

#include "updater.h"

#include <algorithm>
#include <cerrno>
#include <cctype>
#include <cstdint>
#include <cstring>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <mach-o/dyld.h>
#include <optional>
#include <string>
#include <sys/stat.h>
#include <sys/wait.h>
#include <system_error>
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
        execl("/usr/bin/curl",
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
        execl("/usr/bin/curl",
            "curl",
            "-fsSL",
            "--max-time",
            "120",
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

std::optional<std::filesystem::path> CurrentExecutablePath()
{
    std::vector<char> buffer(1024);
    while (true) {
        std::uint32_t size = static_cast<std::uint32_t>(buffer.size());
        if (_NSGetExecutablePath(buffer.data(), &size) == 0) {
            std::error_code ec;
            const std::filesystem::path resolved = std::filesystem::weakly_canonical(buffer.data(), ec);
            return ec ? std::filesystem::path(buffer.data()) : resolved;
        }
        if (size <= buffer.size()) {
            return std::nullopt;
        }
        buffer.resize(size);
    }
}

std::optional<std::filesystem::path> CurrentAppBundlePath()
{
    const auto executablePath = CurrentExecutablePath();
    if (!executablePath) {
        return std::nullopt;
    }

    const std::filesystem::path contentsPath = executablePath->parent_path().parent_path();
    const std::filesystem::path bundlePath = contentsPath.parent_path();
    if (bundlePath.extension() != ".app") {
        return std::nullopt;
    }
    return bundlePath;
}

bool IsMountedDmgPath(const std::filesystem::path& path)
{
    const std::string value = path.string();
    return value == "/Volumes" || value.rfind("/Volumes/", 0) == 0;
}

bool CurrentBundleCanBeReplaced(std::string* errorMessage)
{
    const auto bundlePath = CurrentAppBundlePath();
    if (!bundlePath) {
        if (errorMessage) {
            *errorMessage = "macOS auto-update requires running from a .app bundle.";
        }
        return false;
    }

    if (IsMountedDmgPath(*bundlePath)) {
        if (errorMessage) {
            *errorMessage =
                "Automatic installation is unavailable while SMU is running from a mounted DMG. Drag the app to Applications and launch it there first.";
        }
        return false;
    }

    std::error_code ec;
    if (!std::filesystem::exists(*bundlePath, ec) || ec) {
        if (errorMessage) {
            *errorMessage = "Could not resolve the current SMU app bundle on disk.";
        }
        return false;
    }

    const std::filesystem::path parent = bundlePath->parent_path();
    if (parent.empty() || access(parent.c_str(), W_OK) != 0) {
        if (errorMessage) {
            *errorMessage = "Cannot update SMU because its containing folder is not writable: " + parent.string();
        }
        return false;
    }

    return true;
}

std::filesystem::path CreateTemporaryDirectory(std::string* errorMessage)
{
    char dirTemplate[] = "/tmp/smu-macos-updater-XXXXXX";
    char* created = mkdtemp(dirTemplate);
    if (!created) {
        if (errorMessage) {
            *errorMessage = std::string("Failed to create temporary updater directory: ") + std::strerror(errno);
        }
        return {};
    }
    return std::filesystem::path(created);
}

bool WriteUpdaterScript(const std::filesystem::path& scriptPath, std::string* errorMessage)
{
    std::ofstream script(scriptPath, std::ios::binary | std::ios::trunc);
    if (!script) {
        if (errorMessage) {
            *errorMessage = "Failed to open macOS updater script for writing.";
        }
        return false;
    }

    script <<
        "#!/bin/sh\n"
        "set -u\n"
        "\n"
        "CURRENT_APP=\"$1\"\n"
        "WORK_DIR=\"$2\"\n"
        "ZIP_PATH=\"$3\"\n"
        "APP_NAME=\"$4\"\n"
        "OLD_PID=\"$5\"\n"
        "LOG_FILE=\"${TMPDIR:-/tmp}/smu-macos-updater.log\"\n"
        "\n"
        "echo \"SMU macOS updater started\" > \"$LOG_FILE\"\n"
        "echo \"Current app: $CURRENT_APP\" >> \"$LOG_FILE\"\n"
        "echo \"Update zip: $ZIP_PATH\" >> \"$LOG_FILE\"\n"
        "\n"
        "i=0\n"
        "while kill -0 \"$OLD_PID\" 2>/dev/null; do\n"
        "    i=$((i + 1))\n"
        "    if [ \"$i\" -gt 150 ]; then\n"
        "        echo \"Timed out waiting for old SMU process to exit\" >> \"$LOG_FILE\"\n"
        "        exit 1\n"
        "    fi\n"
        "    sleep 0.1\n"
        "done\n"
        "\n"
        "EXTRACT_DIR=\"$WORK_DIR/extracted\"\n"
        "mkdir -p \"$EXTRACT_DIR\" || exit 1\n"
        "/usr/bin/ditto -x -k -- \"$ZIP_PATH\" \"$EXTRACT_DIR\" >> \"$LOG_FILE\" 2>&1 || exit 1\n"
        "\n"
        "NEW_APP=$(/usr/bin/find \"$EXTRACT_DIR\" -maxdepth 3 -type d -name \"$APP_NAME\" -print -quit)\n"
        "if [ -z \"$NEW_APP\" ]; then\n"
        "    NEW_APP=$(/usr/bin/find \"$EXTRACT_DIR\" -maxdepth 3 -type d -name \"*.app\" -print -quit)\n"
        "fi\n"
        "if [ -z \"$NEW_APP\" ]; then\n"
        "    echo \"Downloaded update did not contain a .app bundle\" >> \"$LOG_FILE\"\n"
        "    exit 1\n"
        "fi\n"
        "\n"
        "BACKUP_APP=\"$CURRENT_APP.old.$$\"\n"
        "rm -rf -- \"$BACKUP_APP\"\n"
        "if ! mv -- \"$CURRENT_APP\" \"$BACKUP_APP\" >> \"$LOG_FILE\" 2>&1; then\n"
        "    echo \"Failed to move current app aside\" >> \"$LOG_FILE\"\n"
        "    exit 1\n"
        "fi\n"
        "if ! mv -- \"$NEW_APP\" \"$CURRENT_APP\" >> \"$LOG_FILE\" 2>&1; then\n"
        "    echo \"Failed to move new app into place; restoring previous app\" >> \"$LOG_FILE\"\n"
        "    mv -- \"$BACKUP_APP\" \"$CURRENT_APP\" >> \"$LOG_FILE\" 2>&1\n"
        "    exit 1\n"
        "fi\n"
        "\n"
        "SUPPORT_DIR=\"$HOME/Library/Application Support/Spencer Macro Utilities\"\n"
        "mkdir -p \"$SUPPORT_DIR\" >/dev/null 2>&1 || true\n"
        "touch \"$SUPPORT_DIR/macos-permissions-may-need-repair\" >/dev/null 2>&1 || true\n"
        "xattr -dr com.apple.quarantine \"$CURRENT_APP\" >/dev/null 2>&1 || true\n"
        "/usr/bin/open -n \"$CURRENT_APP\" >> \"$LOG_FILE\" 2>&1 &\n"
        "rm -rf -- \"$BACKUP_APP\" \"$WORK_DIR\"\n"
        "rm -f -- \"$0\"\n"
        "exit 0\n";

    script.close();
    if (!script) {
        if (errorMessage) {
            *errorMessage = "Failed to finish writing macOS updater script.";
        }
        return false;
    }

    if (chmod(scriptPath.c_str(), 0700) != 0) {
        if (errorMessage) {
            *errorMessage = std::string("Failed to make macOS updater script executable: ") + std::strerror(errno);
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
    const std::filesystem::path tempDir = CreateTemporaryDirectory(errorMessage);
    if (tempDir.empty()) {
        return false;
    }

    const std::filesystem::path tempPath = tempDir / "download.bin";
    const bool downloaded = DownloadUrlToFile(url, tempPath, errorMessage);
    if (!downloaded) {
        std::filesystem::remove_all(tempDir);
        return false;
    }

    std::ifstream file(tempPath, std::ios::binary);
    if (!file) {
        std::filesystem::remove_all(tempDir);
        if (errorMessage) {
            *errorMessage = "Failed to open downloaded update file.";
        }
        return false;
    }

    data.assign(std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>());
    std::filesystem::remove_all(tempDir);
    return !data.empty();
}

int ScoreAssetForCurrentPlatform(const ReleaseAsset& asset)
{
    const std::string name = Lower(asset.name);
    if (!EndsWith(name, ".zip")) {
        return 0;
    }
    if (Contains(name, "windows") || Contains(name, "linux") || Contains(name, "appimage")) {
        return 0;
    }

    int score = 20;
    if (Contains(name, "macos") || Contains(name, "mac-os") || Contains(name, "darwin") || Contains(name, "osx")) score += 60;
    if (Contains(name, "universal")) score += 20;
    if (Contains(name, "spencer") || Contains(name, "macro") || Contains(name, "suspend")) score += 8;
    return score;
}

bool PlatformAutoApplySupported()
{
    return CurrentBundleCanBeReplaced(nullptr);
}

bool ApplyUpdateFromAsset(const ReleaseAsset& asset, const std::string&, const std::string&, std::string* errorMessage)
{
    const std::string assetName = Lower(asset.name);
    if (!EndsWith(assetName, ".zip")) {
        if (errorMessage) {
            *errorMessage = "macOS auto-update only supports ZIP release assets containing a .app bundle.";
        }
        return false;
    }

    if (!CurrentBundleCanBeReplaced(errorMessage)) {
        return false;
    }

    const auto currentBundle = CurrentAppBundlePath();
    if (!currentBundle) {
        if (errorMessage) {
            *errorMessage = "Could not resolve the current SMU app bundle.";
        }
        return false;
    }

    const std::filesystem::path workDir = CreateTemporaryDirectory(errorMessage);
    if (workDir.empty()) {
        return false;
    }

    std::error_code ec;
    const std::filesystem::path zipPath = workDir / "update.zip";
    if (!DownloadUrlToFile(asset.downloadUrl, zipPath, errorMessage)) {
        std::filesystem::remove_all(workDir, ec);
        return false;
    }

    if (!std::filesystem::exists(zipPath, ec) || ec || std::filesystem::file_size(zipPath, ec) == 0 || ec) {
        std::filesystem::remove_all(workDir, ec);
        if (errorMessage) {
            *errorMessage = "Downloaded macOS update package was missing or empty.";
        }
        return false;
    }

    const std::filesystem::path scriptPath = workDir / "apply-update.sh";
    if (!WriteUpdaterScript(scriptPath, errorMessage)) {
        std::filesystem::remove_all(workDir, ec);
        return false;
    }

    const pid_t child = fork();
    if (child < 0) {
        std::filesystem::remove_all(workDir, ec);
        if (errorMessage) {
            *errorMessage = std::string("Failed to launch macOS updater script: ") + std::strerror(errno);
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
            currentBundle->c_str(),
            workDir.c_str(),
            zipPath.c_str(),
            currentBundle->filename().c_str(),
            oldPid.c_str(),
            static_cast<char*>(nullptr));

        _exit(errno == ENOENT ? 127 : 126);
    }

    std::exit(0);
    return true;
}

} // namespace smu::updater::detail

#endif

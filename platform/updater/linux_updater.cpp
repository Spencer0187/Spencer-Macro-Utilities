#if defined(__linux__)

#include "updater.h"
#include "asset_selection.h"

#include <algorithm>
#include <cerrno>
#include <cctype>
#include <cstdint>
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

enum class LinuxAssetKind {
    Unsupported,
    AppImage,
    DistributionBundle,
};

LinuxAssetKind GetLinuxAssetKind(const std::string& lowerName)
{
    if (EndsWith(lowerName, ".appimage")) {
        return LinuxAssetKind::AppImage;
    }
    if (EndsWith(lowerName, ".zip") &&
        Contains(lowerName, "linux") &&
        !Contains(lowerName, "windows") &&
        !Contains(lowerName, "macos") &&
        !Contains(lowerName, "darwin")) {
        return LinuxAssetKind::DistributionBundle;
    }
    return LinuxAssetKind::Unsupported;
}

bool TargetsDifferentArchitecture(const std::string& lowerName)
{
#if defined(__x86_64__) || defined(__amd64__)
    return AssetTargetsDifferentLinuxArchitecture(lowerName, LinuxArchitecture::X86_64);
#elif defined(__aarch64__)
    return AssetTargetsDifferentLinuxArchitecture(lowerName, LinuxArchitecture::AArch64);
#else
    return false;
#endif
}

std::uint16_t CurrentElfMachine()
{
#if defined(__x86_64__) || defined(__amd64__)
    return 62; // EM_X86_64
#elif defined(__aarch64__)
    return 183; // EM_AARCH64
#else
    return 0;
#endif
}

bool HasExpectedAppImageHeader(const char* bytes, std::size_t size)
{
    // AppImages are ELF executables. Checking the fixed ELF fields catches an
    // accidentally selected package, HTML error response, or wrong-arch image
    // before the updater replaces the currently working executable.
    if (!bytes || size < 20 ||
        static_cast<unsigned char>(bytes[0]) != 0x7f ||
        bytes[1] != 'E' ||
        bytes[2] != 'L' ||
        bytes[3] != 'F' ||
        static_cast<unsigned char>(bytes[4]) != 2 || // ELFCLASS64
        static_cast<unsigned char>(bytes[5]) != 1) { // ELFDATA2LSB
        return false;
    }

    const std::uint16_t expectedMachine = CurrentElfMachine();
    if (expectedMachine == 0) {
        return true;
    }
    const std::uint16_t actualMachine =
        static_cast<std::uint16_t>(static_cast<unsigned char>(bytes[18])) |
        (static_cast<std::uint16_t>(static_cast<unsigned char>(bytes[19])) << 8);
    return actualMachine == expectedMachine;
}

bool ValidateAppImageFile(const std::filesystem::path& path, std::string* errorMessage)
{
    std::ifstream file(path, std::ios::binary);
    char header[20] {};
    if (!file.read(header, sizeof(header)) || !HasExpectedAppImageHeader(header, sizeof(header))) {
        if (errorMessage) {
            *errorMessage = "Downloaded update was not a compatible AppImage for this Linux architecture.";
        }
        return false;
    }
    return true;
}

bool WriteAppImage(
    const std::filesystem::path& destination,
    const std::vector<char>& bytes,
    std::string* errorMessage)
{
    if (!HasExpectedAppImageHeader(bytes.data(), bytes.size())) {
        if (errorMessage) {
            *errorMessage = "The Linux update bundle contained an incompatible or invalid AppImage.";
        }
        return false;
    }

    std::ofstream file(destination, std::ios::binary | std::ios::trunc);
    if (!file) {
        if (errorMessage) {
            *errorMessage = "Failed to create the temporary AppImage.";
        }
        return false;
    }
    file.write(bytes.data(), static_cast<std::streamsize>(bytes.size()));
    file.close();
    if (!file) {
        std::error_code ec;
        std::filesystem::remove(destination, ec);
        if (errorMessage) {
            *errorMessage = "Failed to finish writing the temporary AppImage.";
        }
        return false;
    }
    return true;
}

bool CreateTemporaryAppImage(
    const std::filesystem::path& directory,
    const std::filesystem::path& currentFileName,
    std::filesystem::path& temporaryPath,
    std::string* errorMessage)
{
    std::string pathTemplate =
        (directory / ("." + currentFileName.string() + ".update-XXXXXX")).string();
    std::vector<char> mutableTemplate(pathTemplate.begin(), pathTemplate.end());
    mutableTemplate.push_back('\0');

    const int fd = mkstemp(mutableTemplate.data());
    if (fd < 0) {
        if (errorMessage) {
            *errorMessage =
                std::string("Failed to create a temporary AppImage: ") + std::strerror(errno);
        }
        return false;
    }
    close(fd);
    temporaryPath = std::filesystem::path(mutableTemplate.data());
    return true;
}

bool PathIsWithin(
    const std::filesystem::path& path,
    const std::filesystem::path& directory)
{
    auto pathPart = path.begin();
    for (auto directoryPart = directory.begin();
         directoryPart != directory.end();
         ++directoryPart, ++pathPart) {
        if (pathPart == path.end() || *pathPart != *directoryPart) {
            return false;
        }
    }
    return pathPart != path.end();
}

bool RunningFromDeclaredAppImageMount(std::string* errorMessage)
{
    const char* appDirEnv = std::getenv("APPDIR");
    if (!appDirEnv || !*appDirEnv) {
        if (errorMessage) {
            *errorMessage =
                "Linux auto-update requires the APPDIR supplied by the AppImage runtime.";
        }
        return false;
    }

    std::error_code ec;
    const std::filesystem::path appDir =
        std::filesystem::canonical(std::filesystem::path(appDirEnv), ec);
    if (ec || appDir.empty()) {
        if (errorMessage) {
            *errorMessage = "Could not resolve the current AppImage mount directory.";
        }
        return false;
    }
    const std::filesystem::path executablePath =
        std::filesystem::canonical("/proc/self/exe", ec);
    if (ec || executablePath.empty() ||
        !PathIsWithin(executablePath, appDir)) {
        if (errorMessage) {
            *errorMessage =
                "APPIMAGE is set, but this SMU executable is not running from its AppImage mount.";
        }
        return false;
    }
    return true;
}

bool ResolveReplaceableAppImage(
    std::filesystem::path& currentAppImage,
    std::string* errorMessage)
{
    const char* appImageEnv = std::getenv("APPIMAGE");
    if (!appImageEnv || !*appImageEnv) {
        if (errorMessage) {
            *errorMessage =
                "Linux auto-update requires running SMU from an AppImage. APPIMAGE is not set.";
        }
        return false;
    }
    if (!RunningFromDeclaredAppImageMount(errorMessage)) {
        return false;
    }

    std::error_code ec;
    currentAppImage =
        std::filesystem::absolute(std::filesystem::path(appImageEnv), ec);
    if (ec || currentAppImage.empty()) {
        if (errorMessage) {
            *errorMessage = "Could not resolve the current AppImage path.";
        }
        return false;
    }
    if (!std::filesystem::is_regular_file(currentAppImage, ec) || ec) {
        if (errorMessage) {
            *errorMessage =
                "Current AppImage path is missing or is not a regular file: " +
                currentAppImage.string();
        }
        return false;
    }

    const std::filesystem::path containingDirectory =
        currentAppImage.parent_path();
    if (containingDirectory.empty() ||
        !std::filesystem::is_directory(containingDirectory, ec) ||
        ec ||
        access(containingDirectory.c_str(), W_OK | X_OK) != 0) {
        if (errorMessage) {
            *errorMessage =
                "Cannot update AppImage because its folder is not writable: " +
                containingDirectory.string();
        }
        return false;
    }

    // A write probe is not sufficient for sticky directories such as /tmp:
    // users may create a staging file there but still be unable to rename an
    // AppImage owned by another account.
    struct stat directoryStat {};
    struct stat appImageStat {};
    if (stat(containingDirectory.c_str(), &directoryStat) != 0 ||
        lstat(currentAppImage.c_str(), &appImageStat) != 0) {
        if (errorMessage) {
            *errorMessage =
                "Could not inspect the current AppImage location before updating.";
        }
        return false;
    }
    const uid_t effectiveUser = geteuid();
    if ((directoryStat.st_mode & S_ISVTX) != 0 &&
        effectiveUser != 0 &&
        effectiveUser != directoryStat.st_uid &&
        effectiveUser != appImageStat.st_uid) {
        if (errorMessage) {
            *errorMessage =
                "Cannot replace this AppImage because it is owned by another user in a protected folder.";
        }
        return false;
    }

    std::filesystem::path probePath;
    if (!CreateTemporaryAppImage(
            containingDirectory,
            currentAppImage.filename(),
            probePath,
            errorMessage)) {
        return false;
    }
    std::filesystem::remove(probePath, ec);
    if (ec) {
        if (errorMessage) {
            *errorMessage =
                "Could not clean up the AppImage update writeability probe: " +
                ec.message();
        }
        return false;
    }
    return true;
}

std::string CurrentArchitectureName()
{
#if defined(__x86_64__) || defined(__amd64__)
    return "x86_64";
#elif defined(__aarch64__)
    return "aarch64";
#else
    return {};
#endif
}

std::vector<std::string> AppImageNamesForCurrentArchitecture(
    const std::string& bundleAssetName,
    const std::string& newVersion)
{
    const std::string architecture = CurrentArchitectureName();
    if (architecture.empty()) {
        return {};
    }

    std::vector<std::string> names;
    const std::string lowerBundleName = Lower(bundleAssetName);
    if (EndsWith(lowerBundleName, ".zip") && bundleAssetName.size() > 4) {
        // The release workflow deliberately gives the bundle and its AppImage
        // the same versioned basename.
        names.push_back(bundleAssetName.substr(0, bundleAssetName.size() - 4) + ".AppImage");
    }
    if (!newVersion.empty()) {
        names.push_back(
            "Spencer-Macro-Utilities-V" + newVersion + "-Linux-" + architecture + ".AppImage");
    }
    names.push_back("Spencer-Macro-Utilities-" + architecture + ".AppImage");
#if defined(__x86_64__) || defined(__amd64__)
    names.push_back("Spencer-Macro-Utilities-amd64.AppImage");
#elif defined(__aarch64__)
    names.push_back("Spencer-Macro-Utilities-arm64.AppImage");
#endif

    names.erase(std::unique(names.begin(), names.end()), names.end());
    return names;
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
            "--max-filesize",
            "536870912",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
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
            "--max-filesize",
            "536870912",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
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
#if defined(__x86_64__) || defined(__amd64__)
    return ScoreLinuxAssetName(asset.name, LinuxArchitecture::X86_64);
#elif defined(__aarch64__)
    return ScoreLinuxAssetName(asset.name, LinuxArchitecture::AArch64);
#else
    return ScoreLinuxAssetName(asset.name, LinuxArchitecture::Unknown);
#endif
}

bool ExtractLinuxAppImageFromPackage(
    const std::vector<char>& packageBytes,
    const std::string& bundleAssetName,
    const std::string& newVersion,
    std::vector<char>& appImageBytes,
    std::string* errorMessage)
{
    appImageBytes.clear();
    const std::vector<std::string> candidates =
        AppImageNamesForCurrentArchitecture(bundleAssetName, newVersion);
    if (candidates.empty()) {
        if (errorMessage) {
            *errorMessage = "Linux auto-update does not recognize this CPU architecture.";
        }
        return false;
    }

    for (const std::string& candidate : candidates) {
        std::string extractionError;
        if (smu::updater::ExtractUpdatePackage(
                packageBytes,
                candidate,
                appImageBytes,
                &extractionError)) {
            if (!HasExpectedAppImageHeader(appImageBytes.data(), appImageBytes.size())) {
                appImageBytes.clear();
                if (errorMessage) {
                    *errorMessage = "The Linux update bundle contained an incompatible or invalid AppImage.";
                }
                return false;
            }
            return true;
        }
    }

    if (errorMessage) {
        *errorMessage =
            "The Linux update bundle did not contain the expected root-level AppImage for this architecture.";
    }
    return false;
}

std::string BuildLinuxUpdaterScript()
{
    return R"SMU_UPDATE(#!/bin/sh
set -u

OLD_APPIMAGE="$1"
NEW_APPIMAGE="$2"
OLD_PID="$3"
LOG_FILE="${TMPDIR:-/tmp}/smu-appimage-updater.log"

echo "SMU AppImage updater started" > "$LOG_FILE"
echo "Old AppImage: $OLD_APPIMAGE" >> "$LOG_FILE"
echo "New AppImage: $NEW_APPIMAGE" >> "$LOG_FILE"
echo "Old PID: $OLD_PID" >> "$LOG_FILE"

relaunch_previous() {
    if kill -0 "$OLD_PID" 2>/dev/null; then
        echo "Previous SMU process is still running; not starting a duplicate" >> "$LOG_FILE"
        return
    fi
    if [ ! -f "$OLD_APPIMAGE" ] || [ ! -x "$OLD_APPIMAGE" ]; then
        echo "Previous AppImage is unavailable and could not be relaunched" >> "$LOG_FILE"
        return
    fi
    nohup "$OLD_APPIMAGE" >/dev/null 2>&1 &
}

cleanup_before_swap() {
    rm -f -- "$NEW_APPIMAGE" "$0"
    relaunch_previous
}

i=0
while kill -0 "$OLD_PID" 2>/dev/null; do
    i=$((i + 1))
    if [ "$i" -gt 100 ]; then
        echo "Timed out waiting for old SMU process to exit" >> "$LOG_FILE"
        cleanup_before_swap
        exit 1
    fi
    sleep 0.1
done

sleep 0.5

if ! chmod 755 "$NEW_APPIMAGE" 2>> "$LOG_FILE"; then
    cleanup_before_swap
    exit 1
fi

BACKUP_APPIMAGE="${OLD_APPIMAGE}.old.${OLD_PID}.$$"
if [ -e "$BACKUP_APPIMAGE" ]; then
    echo "Refusing to overwrite an existing AppImage backup" >> "$LOG_FILE"
    cleanup_before_swap
    exit 1
fi
i=0
while true; do
    if mv -- "$OLD_APPIMAGE" "$BACKUP_APPIMAGE" 2>> "$LOG_FILE"; then
        break
    fi

    i=$((i + 1))
    if [ "$i" -gt 40 ]; then
        echo "Failed to move the old AppImage aside" >> "$LOG_FILE"
        cleanup_before_swap
        exit 1
    fi

    sleep 0.25
done

if ! mv -- "$NEW_APPIMAGE" "$OLD_APPIMAGE" 2>> "$LOG_FILE"; then
    echo "Failed to install the new AppImage; restoring the previous version" >> "$LOG_FILE"
    if mv -- "$BACKUP_APPIMAGE" "$OLD_APPIMAGE" >> "$LOG_FILE" 2>&1; then
        relaunch_previous
    else
        echo "Automatic rollback failed; backup remains at $BACKUP_APPIMAGE" >> "$LOG_FILE"
    fi
    rm -f -- "$NEW_APPIMAGE"
    rm -f -- "$0"
    exit 1
fi

nohup "$OLD_APPIMAGE" >/dev/null 2>&1 &
NEW_PID=$!
sleep 2
if ! kill -0 "$NEW_PID" 2>/dev/null; then
    echo "Updated AppImage exited during startup; restoring the previous version" >> "$LOG_FILE"
    rm -f -- "$OLD_APPIMAGE"
    if mv -- "$BACKUP_APPIMAGE" "$OLD_APPIMAGE" >> "$LOG_FILE" 2>&1; then
        relaunch_previous
    else
        echo "Automatic rollback failed; backup remains at $BACKUP_APPIMAGE" >> "$LOG_FILE"
    fi
    rm -f -- "$0"
    exit 1
fi

rm -f -- "$BACKUP_APPIMAGE"
rm -f -- "$0"
exit 0
)SMU_UPDATE";
}

bool PlatformAutoApplySupported()
{
    std::filesystem::path currentAppImage;
    return ResolveReplaceableAppImage(currentAppImage, nullptr);
}


bool ApplyUpdateFromAsset(
    const ReleaseAsset& asset,
    const std::string& newVersion,
    const std::string&,
    std::string* errorMessage)
{
    const std::string assetName = Lower(asset.name);
    const LinuxAssetKind assetKind = GetLinuxAssetKind(assetName);
    if (assetKind == LinuxAssetKind::Unsupported || TargetsDifferentArchitecture(assetName)) {
        if (errorMessage) {
            *errorMessage = "Linux auto-update requires a matching AppImage or Linux distribution bundle.";
        }
        return false;
    }

    std::filesystem::path currentAppImage;
    if (!ResolveReplaceableAppImage(currentAppImage, errorMessage)) {
        return false;
    }

    std::error_code ec;
    const std::filesystem::path appImageDir = currentAppImage.parent_path();
    std::filesystem::path tempAppImage;
    if (!CreateTemporaryAppImage(
            appImageDir,
            currentAppImage.filename(),
            tempAppImage,
            errorMessage)) {
        return false;
    }

    if (assetKind == LinuxAssetKind::AppImage) {
        if (!smu::updater::DownloadAssetToFile(asset, tempAppImage, errorMessage) ||
            !ValidateAppImageFile(tempAppImage, errorMessage)) {
            std::filesystem::remove(tempAppImage, ec);
            return false;
        }
    } else {
        std::vector<char> packageBytes;
        std::vector<char> appImageBytes;
        if (!smu::updater::DownloadAssetToMemory(asset, packageBytes, errorMessage) ||
            !ExtractLinuxAppImageFromPackage(
                packageBytes,
                asset.name,
                newVersion,
                appImageBytes,
                errorMessage) ||
            !WriteAppImage(tempAppImage, appImageBytes, errorMessage)) {
            std::filesystem::remove(tempAppImage, ec);
            return false;
        }
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

        script << BuildLinuxUpdaterScript();
    }

    if (chmod(scriptPath.c_str(), 0700) != 0) {
        std::filesystem::remove(tempAppImage, ec);
        std::filesystem::remove(scriptPath, ec);
        if (errorMessage) {
            *errorMessage = std::string("Failed to make updater script executable: ") + std::strerror(errno);
        }
        return false;
    }

    const std::string oldPid = std::to_string(getpid());
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

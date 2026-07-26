#if defined(__APPLE__)

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

constexpr std::uintmax_t kMaximumUpdateBytes = 512ULL * 1024ULL * 1024ULL;

std::string Lower(std::string value)
{
    std::transform(value.begin(), value.end(), value.begin(), [](unsigned char ch) {
        return static_cast<char>(std::tolower(ch));
    });
    return value;
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
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-time",
            "15",
            "--max-filesize",
            "536870912",
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
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-time",
            "120",
            "--max-filesize",
            "536870912",
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

bool WaitForChild(pid_t pid, int* status)
{
    for (;;) {
        if (waitpid(pid, status, 0) == pid) {
            return true;
        }
        if (errno != EINTR) {
            return false;
        }
    }
}

bool HasCertificateBackedReleaseSignature(
    const std::filesystem::path& bundlePath,
    std::string* errorMessage)
{
    const pid_t verifyPid = fork();
    if (verifyPid < 0) {
        if (errorMessage) {
            *errorMessage =
                std::string("Could not inspect the current macOS code signature: ") +
                std::strerror(errno);
        }
        return false;
    }
    if (verifyPid == 0) {
        execl(
            "/usr/bin/codesign",
            "codesign",
            "--verify",
            "--deep",
            "--strict",
            bundlePath.c_str(),
            static_cast<char*>(nullptr));
        _exit(errno == ENOENT ? 127 : 126);
    }

    int verifyStatus = 0;
    if (!WaitForChild(verifyPid, &verifyStatus) ||
        !WIFEXITED(verifyStatus) ||
        WEXITSTATUS(verifyStatus) != 0) {
        if (errorMessage) {
            *errorMessage =
                "Automatic updates require an installed SMU app with a valid release signature.";
        }
        return false;
    }

    char dirTemplate[] = "/tmp/smu-macos-signer-XXXXXX";
    char* signerDirectory = mkdtemp(dirTemplate);
    if (!signerDirectory) {
        if (errorMessage) {
            *errorMessage =
                std::string("Could not create a macOS signature-check directory: ") +
                std::strerror(errno);
        }
        return false;
    }

    const std::filesystem::path signerDir(signerDirectory);
    const std::filesystem::path signerPrefix = signerDir / "signer-";
    const std::string extractOption =
        "--extract-certificates=" + signerPrefix.string();
    const pid_t extractPid = fork();
    if (extractPid < 0) {
        std::filesystem::remove_all(signerDir);
        if (errorMessage) {
            *errorMessage =
                std::string("Could not inspect the macOS release certificate: ") +
                std::strerror(errno);
        }
        return false;
    }
    if (extractPid == 0) {
        execl(
            "/usr/bin/codesign",
            "codesign",
            "-d",
            extractOption.c_str(),
            bundlePath.c_str(),
            static_cast<char*>(nullptr));
        _exit(errno == ENOENT ? 127 : 126);
    }

    int extractStatus = 0;
    const bool extracted =
        WaitForChild(extractPid, &extractStatus) &&
        WIFEXITED(extractStatus) &&
        WEXITSTATUS(extractStatus) == 0;
    const std::filesystem::path leafCertificate =
        signerPrefix.string() + "0";
    std::error_code ec;
    const bool hasLeafCertificate =
        extracted &&
        std::filesystem::is_regular_file(leafCertificate, ec) &&
        !ec &&
        std::filesystem::file_size(leafCertificate, ec) > 0 &&
        !ec;
    std::filesystem::remove_all(signerDir, ec);

    if (!hasLeafCertificate && errorMessage) {
        *errorMessage =
            "Automatic updates require the official certificate-signed SMU app. "
            "This build must be updated manually.";
    }
    return hasLeafCertificate;
}

bool CanStageBundleInDirectory(
    const std::filesystem::path& directory,
    std::string* errorMessage)
{
    const std::string pathTemplate =
        (directory / ".smu-update-write-test-XXXXXX").string();
    std::vector<char> mutableTemplate(pathTemplate.begin(), pathTemplate.end());
    mutableTemplate.push_back('\0');
    char* probeDirectory = mkdtemp(mutableTemplate.data());
    if (!probeDirectory) {
        if (errorMessage) {
            *errorMessage =
                "SMU cannot update itself because its containing folder is not writable.";
        }
        return false;
    }

    std::error_code ec;
    std::filesystem::remove(std::filesystem::path(probeDirectory), ec);
    if (ec) {
        if (errorMessage) {
            *errorMessage =
                "Could not clean up the macOS update writeability probe: " +
                ec.message();
        }
        return false;
    }
    return true;
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
    if (!std::filesystem::is_directory(*bundlePath, ec) || ec) {
        if (errorMessage) {
            *errorMessage = "Could not resolve the current SMU app bundle on disk.";
        }
        return false;
    }
    if (!HasCertificateBackedReleaseSignature(*bundlePath, errorMessage)) {
        return false;
    }

    const std::filesystem::path parent = bundlePath->parent_path();
    if (parent.empty() || access(parent.c_str(), W_OK | X_OK) != 0) {
        if (errorMessage) {
            *errorMessage = "Cannot update SMU because its containing folder is not writable: " + parent.string();
        }
        return false;
    }

    struct stat parentStat {};
    struct stat bundleStat {};
    if (stat(parent.c_str(), &parentStat) != 0 ||
        lstat(bundlePath->c_str(), &bundleStat) != 0) {
        if (errorMessage) {
            *errorMessage =
                "Could not inspect the current SMU app location before updating.";
        }
        return false;
    }
    const uid_t effectiveUser = geteuid();
    if ((parentStat.st_mode & S_ISVTX) != 0 &&
        effectiveUser != 0 &&
        effectiveUser != parentStat.st_uid &&
        effectiveUser != bundleStat.st_uid) {
        if (errorMessage) {
            *errorMessage =
                "Cannot replace this app because it is owned by another user in a protected folder.";
        }
        return false;
    }

    return CanStageBundleInDirectory(parent, errorMessage);
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
        "CURRENT_MOVED=0\n"
        "STAGED_APP=\"\"\n"
        "\n"
        "cleanup_updater() {\n"
        "    status=\"$1\"\n"
        "    trap - EXIT HUP INT TERM\n"
        "    if [ \"$status\" -ne 0 ] && [ \"$CURRENT_MOVED\" -eq 0 ] && "
            "! kill -0 \"$OLD_PID\" 2>/dev/null && [ -d \"$CURRENT_APP\" ]; then\n"
        "        /usr/bin/open -n \"$CURRENT_APP\" >> \"$LOG_FILE\" 2>&1 || true\n"
        "    fi\n"
        "    if [ \"$status\" -ne 0 ] && [ -n \"$STAGED_APP\" ] && [ -e \"$STAGED_APP\" ]; then\n"
        "        /bin/rm -rf \"$STAGED_APP\"\n"
        "    fi\n"
        "    /bin/rm -rf \"$WORK_DIR\"\n"
        "    /bin/rm -f \"$0\"\n"
        "    exit \"$status\"\n"
        "}\n"
        "trap 'cleanup_updater $?' EXIT\n"
        "trap 'exit 1' HUP INT TERM\n"
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
        "    /bin/sleep 0.1\n"
        "done\n"
        "\n"
        "EXTRACT_DIR=\"$WORK_DIR/extracted\"\n"
        "/bin/mkdir -p \"$EXTRACT_DIR\" || exit 1\n"
        "/usr/bin/ditto -x -k \"$ZIP_PATH\" \"$EXTRACT_DIR\" >> \"$LOG_FILE\" 2>&1 || exit 1\n"
        "\n"
        "NEW_APP=\"$EXTRACT_DIR/$APP_NAME\"\n"
        "if [ ! -d \"$NEW_APP\" ]; then\n"
        "    NEW_APP=$(/usr/bin/find \"$EXTRACT_DIR\" -type d -name \"*.app\" -print | /usr/bin/head -n 1)\n"
        "fi\n"
        "if [ -z \"$NEW_APP\" ]; then\n"
        "    echo \"Downloaded update did not contain a .app bundle\" >> \"$LOG_FILE\"\n"
        "    exit 1\n"
        "fi\n"
        "\n"
        "if ! /usr/bin/codesign --verify --deep --strict \"$NEW_APP\" >> \"$LOG_FILE\" 2>&1; then\n"
        "    echo \"Downloaded app failed code-signature validation\" >> \"$LOG_FILE\"\n"
        "    exit 1\n"
        "fi\n"
        "CURRENT_CERT=\"$WORK_DIR/current-signer-\"\n"
        "NEW_CERT=\"$WORK_DIR/new-signer-\"\n"
        "if ! /usr/bin/codesign -d --extract-certificates=\"$CURRENT_CERT\" \"$CURRENT_APP\" >> \"$LOG_FILE\" 2>&1; then\n"
        "    echo \"Current app does not have a certificate-backed release signature\" >> \"$LOG_FILE\"\n"
        "    exit 1\n"
        "fi\n"
        "if ! /usr/bin/codesign -d --extract-certificates=\"$NEW_CERT\" \"$NEW_APP\" >> \"$LOG_FILE\" 2>&1; then\n"
        "    echo \"Downloaded app does not have a certificate-backed release signature\" >> \"$LOG_FILE\"\n"
        "    exit 1\n"
        "fi\n"
        "if [ ! -f \"${CURRENT_CERT}0\" ] || [ ! -f \"${NEW_CERT}0\" ] || "
            "! /usr/bin/cmp -s \"${CURRENT_CERT}0\" \"${NEW_CERT}0\"; then\n"
        "    echo \"Downloaded app was not signed by the same release certificate\" >> \"$LOG_FILE\"\n"
        "    exit 1\n"
        "fi\n"
        "\n"
        "STAGED_APP=\"${CURRENT_APP}.update.${OLD_PID}.$$\"\n"
        "BACKUP_APP=\"${CURRENT_APP}.old.${OLD_PID}.$$\"\n"
        "if [ -e \"$STAGED_APP\" ] || [ -e \"$BACKUP_APP\" ]; then\n"
        "    echo \"Refusing to overwrite an existing app staging path or backup\" >> \"$LOG_FILE\"\n"
        "    exit 1\n"
        "fi\n"
        "if ! /bin/mv \"$NEW_APP\" \"$STAGED_APP\" >> \"$LOG_FILE\" 2>&1; then\n"
        "    echo \"Failed to stage the downloaded app beside the current app\" >> \"$LOG_FILE\"\n"
        "    exit 1\n"
        "fi\n"
        "if ! /bin/mv \"$CURRENT_APP\" \"$BACKUP_APP\" >> \"$LOG_FILE\" 2>&1; then\n"
        "    echo \"Failed to move current app aside\" >> \"$LOG_FILE\"\n"
        "    /bin/rm -rf \"$STAGED_APP\"\n"
        "    exit 1\n"
        "fi\n"
        "CURRENT_MOVED=1\n"
        "if ! /bin/mv \"$STAGED_APP\" \"$CURRENT_APP\" >> \"$LOG_FILE\" 2>&1; then\n"
        "    echo \"Failed to move new app into place; restoring previous app\" >> \"$LOG_FILE\"\n"
        "    /bin/rm -rf \"$CURRENT_APP\" \"$STAGED_APP\"\n"
        "    if /bin/mv \"$BACKUP_APP\" \"$CURRENT_APP\" >> \"$LOG_FILE\" 2>&1; then\n"
        "        CURRENT_MOVED=0\n"
        "    else\n"
        "        echo \"Automatic rollback failed; backup remains at $BACKUP_APP\" >> \"$LOG_FILE\"\n"
        "    fi\n"
        "    exit 1\n"
        "fi\n"
        "\n"
        "SUPPORT_DIR=\"$HOME/Library/Application Support/Spencer Macro Utilities\"\n"
        "/bin/mkdir -p \"$SUPPORT_DIR\" >/dev/null 2>&1 || true\n"
        "/usr/bin/touch \"$SUPPORT_DIR/macos-permissions-may-need-repair\" >/dev/null 2>&1 || true\n"
        "/usr/bin/xattr -dr com.apple.quarantine \"$CURRENT_APP\" >/dev/null 2>&1 || true\n"
        "if ! /usr/bin/open -n \"$CURRENT_APP\" >> \"$LOG_FILE\" 2>&1; then\n"
        "    echo \"Updated app could not be opened; restoring the previous version\" >> \"$LOG_FILE\"\n"
        "    /bin/rm -rf \"$CURRENT_APP\"\n"
        "    if /bin/mv \"$BACKUP_APP\" \"$CURRENT_APP\" >> \"$LOG_FILE\" 2>&1; then\n"
        "        /usr/bin/open -n \"$CURRENT_APP\" >> \"$LOG_FILE\" 2>&1 || true\n"
        "    else\n"
        "        echo \"Automatic rollback failed; backup remains at $BACKUP_APP\" >> \"$LOG_FILE\"\n"
        "    fi\n"
        "    exit 1\n"
        "fi\n"
        "/bin/rm -rf \"$BACKUP_APP\"\n"
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
    return ScoreMacOSAssetName(asset.name);
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
    if (asset.sizeBytes == 0 || asset.sizeBytes > kMaximumUpdateBytes) {
        if (errorMessage) {
            *errorMessage = "macOS update package failed the updater size checks.";
        }
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
    if (!smu::updater::DownloadAssetToFile(asset, zipPath, errorMessage)) {
        std::filesystem::remove_all(workDir, ec);
        return false;
    }

    const std::uintmax_t downloadedSize = std::filesystem::file_size(zipPath, ec);
    if (!std::filesystem::exists(zipPath, ec) ||
        ec ||
        downloadedSize == 0 ||
        downloadedSize > kMaximumUpdateBytes ||
        downloadedSize != asset.sizeBytes) {
        std::filesystem::remove_all(workDir, ec);
        if (errorMessage) {
            *errorMessage = "Downloaded macOS update package was missing, truncated, or too large.";
        }
        return false;
    }

    const std::filesystem::path scriptPath = workDir / "apply-update.sh";
    if (!WriteUpdaterScript(scriptPath, errorMessage)) {
        std::filesystem::remove_all(workDir, ec);
        return false;
    }

    const std::string oldPid = std::to_string(getpid());
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

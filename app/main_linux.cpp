#include "app_main.h"
#include "app_context.h"
#include "macro_runtime.h"

#include "../core/app_state.h"
#include "../core/macro_state.h"
#include "../platform/input_backend.h"
#include "../platform/linux/input_evdev_uinput.h"
#include "../platform/linux/input_permissions.h"
#include "../platform/linux/process_proc_cgroup.h"
#include "../platform/logging.h"
#include "../platform/network_backend.h"
#include "../platform/process_backend.h"

#ifndef SMU_PORTABLE_GLOBALS
#define SMU_PORTABLE_GLOBALS
#endif
#include "Resource Files/globals.h"

#include <array>
#include <cerrno>
#include <cstdio>
#include <cstring>
#include <cstdlib>
#include <filesystem>
#include <memory>
#include <pwd.h>
#include <string>
#include <sys/wait.h>
#include <system_error>
#include <vector>
#include <unistd.h>

namespace {

std::string GetExecutablePath()
{
    std::array<char, 4096> buffer{};
    const ssize_t length = readlink("/proc/self/exe", buffer.data(), buffer.size() - 1);
    if (length > 0) {
        buffer[static_cast<std::size_t>(length)] = '\0';
        return buffer.data();
    }

    std::error_code ec;
    const std::filesystem::path cwd = std::filesystem::current_path(ec);
    if (ec) {
        return {};
    }
    return cwd.string();
}

std::string GetExecutableBasePath()
{
    const std::string executablePath = GetExecutablePath();
    if (executablePath.empty()) {
        std::error_code ec;
        return std::filesystem::current_path(ec).string();
    }

    return std::filesystem::path(executablePath).parent_path().string();
}

std::string GetCurrentWorkingDirectory()
{
    std::error_code ec;
    const std::filesystem::path cwd = std::filesystem::current_path(ec);
    if (ec) {
        return std::string("unavailable: ") + ec.message();
    }
    return cwd.string();
}

const char* PresenceLabel(const char* value)
{
    return (value && value[0] != '\0') ? "set" : "unset";
}

void LogLinuxStartupDiagnostics()
{
    const char* smuRealUid = std::getenv("SMU_REAL_UID");
    const char* smuRealGid = std::getenv("SMU_REAL_GID");
    const char* sudoUid = std::getenv("SUDO_UID");
    const char* sudoGid = std::getenv("SUDO_GID");

    LogInfo("Linux startup identity: ruid=" + std::to_string(getuid()) +
        ", euid=" + std::to_string(geteuid()) +
        ", smu_real_uid=" + (smuRealUid ? std::string(smuRealUid) : std::string("none")) +
        ", smu_real_gid=" + (smuRealGid ? std::string(smuRealGid) : std::string("none")) +
        ", sudo_uid=" + (sudoUid ? std::string(sudoUid) : std::string("none")) +
        ", sudo_gid=" + (sudoGid ? std::string(sudoGid) : std::string("none")));
    LogInfo("Linux startup working directory: " + GetCurrentWorkingDirectory());
    LogInfo("Linux startup executable base path: " + GetExecutableBasePath());
    LogInfo(std::string("Linux GUI environment: DISPLAY=") + PresenceLabel(std::getenv("DISPLAY")) +
        ", XAUTHORITY=" + PresenceLabel(std::getenv("XAUTHORITY")) +
        ", WAYLAND_DISPLAY=" + PresenceLabel(std::getenv("WAYLAND_DISPLAY")) +
        ", XDG_RUNTIME_DIR=" + PresenceLabel(std::getenv("XDG_RUNTIME_DIR")) +
        ", DBUS_SESSION_BUS_ADDRESS=" + PresenceLabel(std::getenv("DBUS_SESSION_BUS_ADDRESS")));
}

bool PathExists(const std::string& path)
{
    std::error_code ec;
    return !path.empty() && std::filesystem::exists(path, ec) && !ec;
}

std::string GetCurrentUserName()
{
    for (const char* name : {"SMU_REAL_USER", "SUDO_USER", "USER"}) {
        if (const char* user = std::getenv(name)) {
            if (user[0] != '\0' && std::string(user) != "root") {
                return user;
            }
        }
    }

    if (const char* user = std::getenv("USER")) {
        if (user[0] != '\0') {
            return user;
        }
    }

    if (passwd* pwd = getpwuid(getuid())) {
        if (pwd->pw_name) {
            return pwd->pw_name;
        }
    }

    return {};
}

std::string ShellQuote(const std::string& value)
{
    std::string quoted = "'";
    for (char ch : value) {
        if (ch == '\'') {
            quoted += "'\\''";
        } else {
            quoted += ch;
        }
    }
    quoted += "'";
    return quoted;
}

std::string ResolveLinuxSetupScriptPath()
{
    const std::filesystem::path packagedPath = std::filesystem::path(GetExecutableBasePath()) /
        "scripts" / "install_linux_permissions.sh";
    if (PathExists(packagedPath.string())) {
        return packagedPath.string();
    }

#ifdef SMU_SOURCE_ROOT
    const std::filesystem::path sourcePath = std::filesystem::path(SMU_SOURCE_ROOT) /
        "scripts" / "install_linux_permissions.sh";
    if (PathExists(sourcePath.string())) {
        return sourcePath.string();
    }
#endif

    const std::filesystem::path cwdPath = std::filesystem::path(GetCurrentWorkingDirectory()) /
        "scripts" / "install_linux_permissions.sh";
    if (PathExists(cwdPath.string())) {
        return cwdPath.string();
    }

    return packagedPath.string();
}

std::string ResolveLinuxSetupDocsPath()
{
    const std::filesystem::path packagedPath = std::filesystem::path(GetExecutableBasePath()) / "LINUX_SETUP.md";
    if (PathExists(packagedPath.string())) {
        return packagedPath.string();
    }

#ifdef SMU_SOURCE_ROOT
    const std::filesystem::path sourcePath = std::filesystem::path(SMU_SOURCE_ROOT) / "LINUX_SETUP.md";
    if (PathExists(sourcePath.string())) {
        return sourcePath.string();
    }
#endif

    const std::filesystem::path cwdPath = std::filesystem::path(GetCurrentWorkingDirectory()) / "LINUX_SETUP.md";
    if (PathExists(cwdPath.string())) {
        return cwdPath.string();
    }

    return packagedPath.string();
}

std::string BuildManualSudoCommand(const std::string& scriptPath)
{
    const std::filesystem::path cwdScript = std::filesystem::path(GetCurrentWorkingDirectory()) /
        "scripts" / "install_linux_permissions.sh";
    if (PathExists(cwdScript.string())) {
        return "sudo ./scripts/install_linux_permissions.sh";
    }

    return "sudo " + ShellQuote(scriptPath);
}

std::string BuildPermissionDetails(const smu::platform::linux::InputPermissionStatus& status)
{
    std::string details = "Input events: " + status.inputEventsMessage + "\n";
    details += "uinput: " + status.uinputMessage;
    return details;
}

void UpdatePermissionContext(
    smu::app::AppContext& context,
    const smu::platform::linux::InputPermissionStatus& status)
{
    context.linuxInputSetupRequired = !status.ready();
    context.linuxInputPermissionSummary = status.ready()
        ? "Linux input permissions are configured."
        : "Linux input permissions are not configured yet.";
    context.linuxInputPermissionDetails = BuildPermissionDetails(status);
}

void InitializeLinuxInputBackend(
    smu::app::AppContext& context,
    std::shared_ptr<smu::platform::InputBackend>& inputBackend)
{
    const smu::platform::linux::InputPermissionStatus permissionStatus =
        smu::platform::linux::GetInputPermissionStatus();
    UpdatePermissionContext(context, permissionStatus);

    if (!permissionStatus.ready()) {
        context.inputBackendAvailable = false;
        context.inputBackendError = context.linuxInputPermissionDetails;
        inputBackend.reset();
        smu::platform::SetInputBackend(nullptr);
        LogInfo("Linux input backend setup required. " + context.linuxInputPermissionDetails);
        return;
    }

    inputBackend = smu::platform::linux::CreateEvdevUinputInputBackend();
    smu::platform::SetInputBackend(inputBackend);
    context.inputBackendError.clear();
    context.inputBackendAvailable = inputBackend->init(&context.inputBackendError);
    if (!context.inputBackendAvailable) {
        context.linuxInputSetupRequired = true;
        if (!context.inputBackendError.empty()) {
            context.linuxInputPermissionDetails += "\nBackend initialization: " + context.inputBackendError;
            LogWarning(context.inputBackendError);
        }
        inputBackend.reset();
        smu::platform::SetInputBackend(nullptr);
        return;
    }

    context.linuxInputSetupRequired = false;
    LogInfo("Linux input backend initialized.");
}

int RunPermissionInstallerWithPkexec(const std::string& scriptPath)
{
    if (!PathExists(scriptPath)) {
        LogWarning("Linux permission installer script was not found at " + scriptPath);
        return 127;
    }

    const std::string targetUser = GetCurrentUserName();
    if (targetUser.empty()) {
        LogWarning("Linux permission installer could not determine the current user.");
        return 1;
    }

    const std::string targetUserEnv = "SMU_TARGET_USER=" + targetUser;
    const pid_t pid = fork();
    if (pid < 0) {
        LogWarning(std::string("Failed to fork pkexec installer: ") + std::strerror(errno));
        return 1;
    }

    if (pid == 0) {
        execlp("pkexec",
            "pkexec",
            "/usr/bin/env",
            targetUserEnv.c_str(),
            scriptPath.c_str(),
            static_cast<char*>(nullptr));
        _exit(errno == ENOENT ? 127 : 126);
    }

    int status = 0;
    if (waitpid(pid, &status, 0) < 0) {
        LogWarning(std::string("Failed waiting for pkexec installer: ") + std::strerror(errno));
        return 1;
    }

    if (WIFEXITED(status)) {
        return WEXITSTATUS(status);
    }
    return 1;
}

} // namespace

int main(int argc, char** argv)
{
    smu::log::SetFileLoggingEnabled(true);
    LogInfo("Starting Spencer Macro Utilities native Linux app.");
    LogLinuxStartupDiagnostics();
    (void)argc;
    (void)argv;

    smu::core::InitializeMacroSections(false);
    std::snprintf(smu::core::GetAppState().settingsBuffer, sizeof(smu::core::GetAppState().settingsBuffer), "sober");
    Globals::g_isLinuxWine = true;
    std::snprintf(Globals::settingsBuffer, sizeof(Globals::settingsBuffer), "sober");

    smu::app::AppContext context = smu::app::CreateAppContext();
    context.linuxInputInstallerPath = ResolveLinuxSetupScriptPath();
    context.linuxInputSudoCommand = BuildManualSudoCommand(context.linuxInputInstallerPath);
    context.linuxInputSetupDocsPath = ResolveLinuxSetupDocsPath();

    std::shared_ptr<smu::platform::InputBackend> inputBackend;
    InitializeLinuxInputBackend(context, inputBackend);
    context.refreshLinuxInputPermissions = [&context, &inputBackend]() {
        InitializeLinuxInputBackend(context, inputBackend);
    };
    context.installLinuxPermissionsWithPkexec = [&context, &inputBackend]() {
        const int exitCode = RunPermissionInstallerWithPkexec(context.linuxInputInstallerPath);
        if (exitCode == 0) {
            context.linuxInputSetupActionMessage =
                "Permission installer completed. Log out and back in or reboot if access is still missing.";
            InitializeLinuxInputBackend(context, inputBackend);
            return;
        }

        context.linuxInputSetupActionMessage =
            "No graphical polkit authentication agent appears to be available, or authorization was cancelled. "
            "Install/start a polkit agent such as hyprpolkitagent, polkit-kde-agent, or polkit-gnome, "
            "or run this manually: sudo ./scripts/install_linux_permissions.sh";
        LogWarning(context.linuxInputSetupActionMessage);
    };

    std::shared_ptr<smu::platform::ProcessBackend> processBackend = smu::platform::linux::CreateProcCgroupProcessBackend();
    smu::platform::SetProcessBackend(processBackend);
    context.processBackendAvailable = processBackend->init(&context.processBackendError);
    if (!context.processBackendAvailable && !context.processBackendError.empty()) {
        LogWarning(context.processBackendError);
    } else if (context.processBackendAvailable) {
        LogInfo("Linux process backend initialized.");
    }

    smu::platform::SetNetworkLagBackend(nullptr);
    if (auto networkBackend = smu::platform::GetNetworkLagBackend()) {
        context.networkBackendAvailable = networkBackend->isAvailable();
        context.networkBackendError = networkBackend->unsupportedReason();
        LogWarning(context.networkBackendError);
    }

    for (const std::string& warning : context.capabilities.warnings) {
        LogWarning(warning);
    }
    for (const std::string& error : context.capabilities.criticalErrors) {
        LogCritical(error);
    }

    if (Globals::presskey_instances.empty()) Globals::presskey_instances.emplace_back();
    if (Globals::wallhop_instances.empty()) Globals::wallhop_instances.emplace_back();
    if (Globals::spamkey_instances.empty()) Globals::spamkey_instances.emplace_back();

    smu::app::MacroRuntime macroRuntime;
    macroRuntime.start();

    const int result = smu::app::RunSharedApp(context);

    macroRuntime.stop();

    if (auto networkBackend = smu::platform::GetNetworkLagBackend()) {
        networkBackend->shutdown();
    }
    if (processBackend) {
        processBackend->shutdown();
    }
    if (inputBackend) {
        inputBackend->shutdown();
    }

    LogInfo("Spencer Macro Utilities native Linux app stopped.");
    return result;
}

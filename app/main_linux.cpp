#include "app_main.h"
#include "app_context.h"
#include "macro_runtime.h"

#include "../core/app_state.h"
#include "../core/macro_state.h"
#include "../platform/input_backend.h"
#include "../platform/linux/input_evdev_uinput.h"
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
#include <system_error>
#include <vector>
#include <unistd.h>

#define SDL_MAIN_HANDLED
#include <SDL3/SDL.h>

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

void ShowErrorMessageBox(const char* title, const char* message)
{
    std::fprintf(stderr, "%s\n", message);
    LogCritical(message);

    if (SDL_ShowSimpleMessageBox(SDL_MESSAGEBOX_ERROR, title, message, nullptr)) {
        return;
    }

    const char* sdlError = SDL_GetError();
    if (sdlError && sdlError[0] != '\0') {
        LogWarning(std::string("Failed to show root requirement dialog: ") + sdlError);
    }
}

void AppendEnvAssignment(std::vector<std::string>& args, const char* name, const char* value)
{
    if (!name || !value || value[0] == '\0') {
        return;
    }

    args.push_back(std::string(name) + "=" + value);
}

void AppendEnvAssignment(std::vector<std::string>& args, const char* name, const std::string& value)
{
    if (!name || value.empty()) {
        return;
    }

    args.push_back(std::string(name) + "=" + value);
}

std::string GetCurrentUserName()
{
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

int RelaunchWithPkexec(int argc, char** argv)
{
    const std::string executablePath = GetExecutablePath();
    if (executablePath.empty()) {
        ShowErrorMessageBox("Root Required",
            "Native Linux input requires root. Could not resolve the executable path for pkexec relaunch. Launch with sudo -E ./suspend");
        return 1;
    }

    LogInfo("Attempting pkexec relaunch for native Linux input access.");

    const std::string realUid = std::to_string(getuid());
    const std::string realGid = std::to_string(getgid());
    const std::string realUser = GetCurrentUserName();

    std::vector<std::string> args;
    args.emplace_back("pkexec");
    args.emplace_back("/usr/bin/env");

    AppendEnvAssignment(args, "DISPLAY", std::getenv("DISPLAY"));
    AppendEnvAssignment(args, "XAUTHORITY", std::getenv("XAUTHORITY"));
    AppendEnvAssignment(args, "WAYLAND_DISPLAY", std::getenv("WAYLAND_DISPLAY"));
    AppendEnvAssignment(args, "XDG_RUNTIME_DIR", std::getenv("XDG_RUNTIME_DIR"));
    AppendEnvAssignment(args, "DBUS_SESSION_BUS_ADDRESS", std::getenv("DBUS_SESSION_BUS_ADDRESS"));
    AppendEnvAssignment(args, "XDG_CONFIG_HOME", std::getenv("XDG_CONFIG_HOME"));
    AppendEnvAssignment(args, "HOME", std::getenv("HOME"));
    AppendEnvAssignment(args, "LD_LIBRARY_PATH", std::getenv("LD_LIBRARY_PATH"));
    AppendEnvAssignment(args, "DEBUG", std::getenv("DEBUG"));
    AppendEnvAssignment(args, "SMU_DEBUG", std::getenv("SMU_DEBUG"));

    AppendEnvAssignment(args, "SMU_REAL_UID", realUid);
    AppendEnvAssignment(args, "SMU_REAL_GID", realGid);
    AppendEnvAssignment(args, "SMU_REAL_USER", realUser);
    AppendEnvAssignment(args, "SMU_REAL_HOME", std::getenv("HOME"));
    AppendEnvAssignment(args, "SMU_PKEXEC_RELAUNCHED", "1");

    args.push_back(executablePath);
    for (int i = 1; i < argc; ++i) {
        if (argv[i]) {
            args.emplace_back(argv[i]);
        }
    }

    std::vector<char*> execArgs;
    execArgs.reserve(args.size() + 1);
    for (std::string& arg : args) {
        execArgs.push_back(arg.data());
    }
    execArgs.push_back(nullptr);

    execvp("pkexec", execArgs.data());

    const int launchErrno = errno;
    std::string message = "Native Linux input requires root. Failed to launch pkexec";
    if (launchErrno != 0) {
        message += ": ";
        message += std::strerror(launchErrno);
    }
    if (launchErrno == ENOENT) {
        message += ". Install pkexec or launch with sudo -E ./suspend";
    }
    ShowErrorMessageBox("Root Required", message.c_str());
    return 1;
}

} // namespace

int main(int argc, char** argv)
{
    smu::log::SetFileLoggingEnabled(true);
    LogInfo("Starting Spencer Macro Utilities native Linux app.");
    LogLinuxStartupDiagnostics();

    if (geteuid() != 0) {
        const char* relaunched = std::getenv("SMU_PKEXEC_RELAUNCHED");
        if (relaunched && std::string(relaunched) == "1") {
            ShowErrorMessageBox("Root Required",
                "Native Linux input requires root. pkexec did not relaunch the app with elevated privileges. Launch with sudo -E ./suspend");
            return 1;
        }

        return RelaunchWithPkexec(argc, argv);
    }

    smu::core::InitializeMacroSections(false);
    std::snprintf(smu::core::GetAppState().settingsBuffer, sizeof(smu::core::GetAppState().settingsBuffer), "sober");
    Globals::g_isLinuxWine = true;
    std::snprintf(Globals::settingsBuffer, sizeof(Globals::settingsBuffer), "sober");

    smu::app::AppContext context = smu::app::CreateAppContext();

    std::shared_ptr<smu::platform::InputBackend> inputBackend = smu::platform::linux::CreateEvdevUinputInputBackend();
    smu::platform::SetInputBackend(inputBackend);
    context.inputBackendAvailable = inputBackend->init(&context.inputBackendError);
    if (!context.inputBackendAvailable && !context.inputBackendError.empty()) {
        LogWarning(context.inputBackendError);
    } else if (context.inputBackendAvailable) {
        LogInfo("Linux input backend initialized.");
    }

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

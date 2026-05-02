#include "app_main.h"
#include "app_context.h"
#include "macro_runtime.h"
#include "askpass.h"

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

#include <cstdio>
#include <memory>
#include <string>
#include <unistd.h>

int main(int argc, char** argv)
{
    (void)argc;
    (void)argv;

    smu::log::SetFileLoggingEnabled(true);
    LogInfo("Starting Spencer Macro Utilities native Linux app.");

    bool isRoot = (geteuid() == 0);
    if (!isRoot) {
        std::string password = smu::app::AskPassword(
            "Authentication Required",
            "Please enter your password to allow SMC to access input devices.");

        if (password.empty()) {
            LogCritical("Root authentication cancelled or failed; exiting.");
            return 1;
        }

        char full_path[512];
        if (realpath(argv[0], full_path) == nullptr) {
            LogCritical("Failed to resolve full application path.");
            return 1;
        }

        char cmd[1024];
        std::snprintf(cmd, sizeof(cmd),
            "echo '%s' | su -c '%s' > /dev/null 2>&1 &",
            password.c_str(), full_path);

        std::string().swap(password); // clear from std::string memory
        int ret = system(cmd);
        return (ret != 0) ? 1 : 0;
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

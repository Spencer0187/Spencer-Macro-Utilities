#if defined(_WIN32)

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <mmsystem.h>

#include "app_context.h"
#include "app_main.h"
#include "macro_runtime.h"
#include "../platform/logging.h"
#include "../platform/input_backend.h"
#include "../platform/network_backend.h"
#include "../platform/process_backend.h"
#include "../platform/windows/windows_backends.h"
#include "../platform/windows/lagswitch_overlay.h"

#include <algorithm>
#include <string>

namespace {

// Some older Windows SDKs do not expose this Windows 11 power-throttling bit yet.
// The runtime SetProcessInformation call below also retries without it if the OS
// rejects the flag.
#ifndef PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION
constexpr ULONG kProcessPowerThrottlingIgnoreTimerResolution = 0x4;
#else
constexpr ULONG kProcessPowerThrottlingIgnoreTimerResolution = PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION;
#endif

#ifndef PROCESS_POWER_THROTTLING_EXECUTION_SPEED
constexpr ULONG kProcessPowerThrottlingExecutionSpeed = 0x1;
#else
constexpr ULONG kProcessPowerThrottlingExecutionSpeed = PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
#endif

UINT g_windowsTimerResolutionPeriod = 0;

std::string FormatWindowsError(DWORD error)
{
    return std::to_string(static_cast<unsigned long>(error));
}

void ConfigureWindowsTiming()
{
    TIMECAPS caps = {};
    UINT requestedPeriod = 1;
    if (timeGetDevCaps(&caps, sizeof(caps)) == MMSYSERR_NOERROR) {
        requestedPeriod = std::max(caps.wPeriodMin, requestedPeriod);
        if (caps.wPeriodMax > 0) {
            requestedPeriod = std::min(requestedPeriod, caps.wPeriodMax);
        }
    }

    if (timeBeginPeriod(requestedPeriod) == TIMERR_NOERROR) {
        g_windowsTimerResolutionPeriod = requestedPeriod;
        LogInfo("Requested high-resolution Windows timer period for macro/script timing.");
    } else {
        LogWarning("Windows refused the high-resolution timer-period request; short script sleeps may fall back to default scheduler granularity.");
    }

    PROCESS_POWER_THROTTLING_STATE powerThrottling = {};
    powerThrottling.Version = PROCESS_POWER_THROTTLING_CURRENT_VERSION;
    powerThrottling.ControlMask = kProcessPowerThrottlingExecutionSpeed | kProcessPowerThrottlingIgnoreTimerResolution;
    powerThrottling.StateMask = 0; // Disable EcoQoS execution throttling and force Windows to honor timer-resolution requests.

    if (!SetProcessInformation(GetCurrentProcess(), ProcessPowerThrottling, &powerThrottling, sizeof(powerThrottling))) {
        const DWORD firstError = GetLastError();

        // Older OS builds may reject PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION.
        // Still disable execution-speed throttling when available.
        powerThrottling = {};
        powerThrottling.Version = PROCESS_POWER_THROTTLING_CURRENT_VERSION;
        powerThrottling.ControlMask = kProcessPowerThrottlingExecutionSpeed;
        powerThrottling.StateMask = 0;

        if (!SetProcessInformation(GetCurrentProcess(), ProcessPowerThrottling, &powerThrottling, sizeof(powerThrottling))) {
            LogWarning("Windows refused process power-throttling configuration; first error=" +
                FormatWindowsError(firstError) + ", retry error=" + FormatWindowsError(GetLastError()) + ".");
        } else {
            LogWarning("Windows refused the ignore-timer-resolution power-throttling flag; execution-speed throttling was still disabled. Error=" +
                FormatWindowsError(firstError) + ".");
        }
    }
}

void RestoreWindowsTiming()
{
    if (g_windowsTimerResolutionPeriod != 0) {
        timeEndPeriod(g_windowsTimerResolutionPeriod);
        g_windowsTimerResolutionPeriod = 0;
    }
}

} // namespace

int WINAPI wWinMain(HINSTANCE hInstance, HINSTANCE hPrevInstance, LPWSTR lpCmdLine, int nCmdShow)
{
    (void)hInstance;
    (void)hPrevInstance;
    (void)lpCmdLine;
    (void)nCmdShow;

    constexpr const char kWindowsInputBackendInitWarningId[] = "windows_input_backend_init_failed";
    constexpr const char kWindowsProcessBackendInitWarningId[] = "windows_process_backend_init_failed";

    const bool workingDirectoryUpdated = smu::app::SetWorkingDirectoryToExecutablePath();
    smu::log::SetFileLoggingEnabled(smu::log::IsDebugLoggingEnabled());
    if (!workingDirectoryUpdated) {
        LogWarning("Failed to set the working directory to the Windows executable path.");
    }
    LogInfo("Starting Spencer Macro Utilities native Windows app.");
    ConfigureWindowsTiming();

    smu::platform::windows::InitializeWindowsPlatformBackends();

    smu::app::AppContext context = smu::app::CreateAppContext();

    if (auto inputBackend = smu::platform::GetInputBackend()) {
        context.inputBackendAvailable = inputBackend->init(&context.inputBackendError);
        if (!context.inputBackendAvailable && !context.inputBackendError.empty()) {
            LogWarning(context.inputBackendError, kWindowsInputBackendInitWarningId, true);
        }
    }

    if (auto processBackend = smu::platform::GetProcessBackend()) {
        context.processBackendAvailable = processBackend->init(&context.processBackendError);
        if (!context.processBackendAvailable && !context.processBackendError.empty()) {
            LogWarning(context.processBackendError, kWindowsProcessBackendInitWarningId, true);
        }
    }

    if (auto networkBackend = smu::platform::GetNetworkLagBackend()) {
        context.networkBackendAvailable = true;
    }

    smu::app::MacroRuntime macroRuntime;
    macroRuntime.start();

    const int result = smu::app::RunSharedApp(context);

    macroRuntime.stop();

    if (auto networkBackend = smu::platform::GetNetworkLagBackend()) {
        networkBackend->shutdown();
    }
#if defined(_WIN32)
    smu::platform::windows::CleanupLagswitchOverlay();
#endif
    if (auto processBackend = smu::platform::GetProcessBackend()) {
        processBackend->shutdown();
    }
    if (auto inputBackend = smu::platform::GetInputBackend()) {
        inputBackend->shutdown();
    }

    LogInfo("Spencer Macro Utilities native Windows app stopped.");
    RestoreWindowsTiming();
    return result;
}

#endif

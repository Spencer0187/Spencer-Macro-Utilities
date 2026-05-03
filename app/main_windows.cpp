#if defined(_WIN32)

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>

#include "app_context.h"
#include "app_main.h"
#include "macro_runtime.h"
#include "../platform/logging.h"
#include "../platform/input_backend.h"
#include "../platform/network_backend.h"
#include "../platform/process_backend.h"
#include "../platform/windows/windows_backends.h"
#include "../platform/windows/lagswitch_overlay.h"

int WINAPI wWinMain(HINSTANCE hInstance, HINSTANCE hPrevInstance, LPWSTR lpCmdLine, int nCmdShow)
{
    (void)hInstance;
    (void)hPrevInstance;
    (void)lpCmdLine;
    (void)nCmdShow;

    smu::log::SetFileLoggingEnabled(true);
    LogInfo("Starting Spencer Macro Utilities native Windows app.");

    smu::platform::windows::InitializeWindowsPlatformBackends();

    smu::app::AppContext context = smu::app::CreateAppContext();

    if (auto inputBackend = smu::platform::GetInputBackend()) {
        context.inputBackendAvailable = inputBackend->init(&context.inputBackendError);
        if (!context.inputBackendAvailable && !context.inputBackendError.empty()) {
            LogWarning(context.inputBackendError);
        }
    }

    if (auto processBackend = smu::platform::GetProcessBackend()) {
        context.processBackendAvailable = processBackend->init(&context.processBackendError);
        if (!context.processBackendAvailable && !context.processBackendError.empty()) {
            LogWarning(context.processBackendError);
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
    return result;
}

#endif

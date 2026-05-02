#if defined(_WIN32)

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>

#if SMU_USE_SDL_UI

#include "app_context.h"
#include "app_main.h"
#include "macro_runtime.h"
#include "../platform/logging.h"
#include "Resource Files/wine_compatibility_layer.h"

int WINAPI wWinMain(HINSTANCE hInstance, HINSTANCE hPrevInstance, LPWSTR lpCmdLine, int nCmdShow)
{
    (void)hInstance;
    (void)hPrevInstance;
    (void)lpCmdLine;
    (void)nCmdShow;

    smu::log::SetFileLoggingEnabled(true);
    LogInfo("Starting Spencer Macro Utilities SDL3 Windows app.");
    InitLinuxCompatLayer();
    smu::app::AppContext context = smu::app::CreateAppContext();
    smu::app::MacroRuntime macroRuntime;
    macroRuntime.start();
    const int result = smu::app::RunSharedApp(context);
    macroRuntime.stop();
    ShutdownLinuxCompatLayer();
    LogInfo("Spencer Macro Utilities SDL3 Windows app stopped.");
    return result;
}

#else

int WINAPI SMU_RunWindowsLegacyApp(HINSTANCE hInstance, HINSTANCE hPrevInstance, LPWSTR lpCmdLine, int nCmdShow);

int WINAPI wWinMain(HINSTANCE hInstance, HINSTANCE hPrevInstance, LPWSTR lpCmdLine, int nCmdShow)
{
    return SMU_RunWindowsLegacyApp(hInstance, hPrevInstance, lpCmdLine, nCmdShow);
}

#endif

#endif

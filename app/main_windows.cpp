#if defined(_WIN32)

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>

#if SMU_USE_SDL_UI

#include "app_context.h"
#include "app_main.h"
#include "../platform/logging.h"

int WINAPI wWinMain(HINSTANCE hInstance, HINSTANCE hPrevInstance, LPWSTR lpCmdLine, int nCmdShow)
{
    (void)hInstance;
    (void)hPrevInstance;
    (void)lpCmdLine;
    (void)nCmdShow;

    smu::log::SetFileLoggingEnabled(true);
    LogInfo("Starting Spencer Macro Utilities SDL3 Windows app.");
    smu::app::AppContext context = smu::app::CreateAppContext();
    const int result = smu::app::RunSharedApp(context);
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

#if defined(_WIN32)

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <mmsystem.h>

#include "app_context.h"
#include "app_main.h"
#include "app_profile_bridge.h"
#include "macro_runtime.h"
#include "../platform/logging.h"
#include "../platform/input_backend.h"
#include "../platform/network_backend.h"
#include "../platform/process_backend.h"
#include "../platform/windows/windows_backends.h"
#include "../platform/windows/lagswitch_overlay.h"
#include "smu_version.h"

#include <algorithm>
#include <array>
#include <cwchar>
#include <string>
#include <string_view>
#include <vector>

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

// V3.4.0-ONLY LEGACY BRIDGE.
//
// V3.0-V3.3.x Windows updaters can only install a root-level file named
// suspend.exe. The V3.4.0 Windows ZIP intentionally keeps that one historical
// filename so those clients can cross onto the manifest-based updater. Before
// any GUI/macro/backend initialization, this build copies itself to the proper
// public V3.4.0 filename, exits, deletes suspend.exe after the image unlocks,
// and relaunches the correctly named executable.
//
// DELETE THIS ENTIRE MIGRATION BLOCK FOR V3.4.1. The static_assert is an
// intentional tripwire: bumping the Windows build past V3.4.0 without removing
// this compatibility path must fail compilation rather than silently carrying
// suspend.exe support into another release.
static_assert(
    std::string_view(SMU_VERSION_STRING) == "3.4.0",
    "Remove the V3.4.0-only suspend.exe filename migration before building V3.4.1 or later.");

bool GetCurrentExecutablePath(std::wstring& executablePath)
{
    std::vector<wchar_t> buffer(32768, L'\0');
    const DWORD length = GetModuleFileNameW(
        nullptr,
        buffer.data(),
        static_cast<DWORD>(buffer.size()));
    if (length == 0 || length >= buffer.size()) {
        return false;
    }

    executablePath.assign(buffer.data(), length);
    return true;
}

bool FilesHaveIdenticalBytes(const std::wstring& lhsPath, const std::wstring& rhsPath)
{
    const DWORD shareMode = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
    HANDLE lhs = CreateFileW(
        lhsPath.c_str(),
        GENERIC_READ,
        shareMode,
        nullptr,
        OPEN_EXISTING,
        FILE_ATTRIBUTE_NORMAL,
        nullptr);
    if (lhs == INVALID_HANDLE_VALUE) {
        return false;
    }

    HANDLE rhs = CreateFileW(
        rhsPath.c_str(),
        GENERIC_READ,
        shareMode,
        nullptr,
        OPEN_EXISTING,
        FILE_ATTRIBUTE_NORMAL,
        nullptr);
    if (rhs == INVALID_HANDLE_VALUE) {
        CloseHandle(lhs);
        return false;
    }

    LARGE_INTEGER lhsSize = {};
    LARGE_INTEGER rhsSize = {};
    if (!GetFileSizeEx(lhs, &lhsSize) ||
        !GetFileSizeEx(rhs, &rhsSize) ||
        lhsSize.QuadPart != rhsSize.QuadPart) {
        CloseHandle(rhs);
        CloseHandle(lhs);
        return false;
    }

    std::array<unsigned char, 64 * 1024> lhsBuffer = {};
    std::array<unsigned char, 64 * 1024> rhsBuffer = {};
    bool identical = true;
    for (;;) {
        DWORD lhsRead = 0;
        DWORD rhsRead = 0;
        if (!ReadFile(lhs, lhsBuffer.data(), static_cast<DWORD>(lhsBuffer.size()), &lhsRead, nullptr) ||
            !ReadFile(rhs, rhsBuffer.data(), static_cast<DWORD>(rhsBuffer.size()), &rhsRead, nullptr) ||
            lhsRead != rhsRead ||
            !std::equal(lhsBuffer.begin(), lhsBuffer.begin() + lhsRead, rhsBuffer.begin())) {
            identical = false;
            break;
        }
        if (lhsRead == 0) {
            break;
        }
    }

    CloseHandle(rhs);
    CloseHandle(lhs);
    return identical;
}

bool WriteLegacyFilenameMigrationScript(
    const std::wstring& scriptPath,
    const std::string& scriptContents)
{
    HANDLE script = CreateFileW(
        scriptPath.c_str(),
        GENERIC_WRITE,
        0,
        nullptr,
        CREATE_NEW,
        FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_TEMPORARY,
        nullptr);
    if (script == INVALID_HANDLE_VALUE) {
        return false;
    }

    DWORD bytesWritten = 0;
    const bool wroteAll = WriteFile(
        script,
        scriptContents.data(),
        static_cast<DWORD>(scriptContents.size()),
        &bytesWritten,
        nullptr) &&
        static_cast<std::size_t>(bytesWritten) == scriptContents.size();
    if (wroteAll) {
        FlushFileBuffers(script);
    }
    CloseHandle(script);
    return wroteAll;
}

bool ScheduleV340LegacyFilenameMigration()
{
    std::wstring currentPath;
    if (!GetCurrentExecutablePath(currentPath)) {
        return false;
    }

    const std::size_t separator = currentPath.find_last_of(L"\\/");
    const std::wstring currentName = separator == std::wstring::npos
        ? currentPath
        : currentPath.substr(separator + 1);
    if (_wcsicmp(currentName.c_str(), L"suspend.exe") != 0) {
        return false;
    }

    const std::wstring directory = separator == std::wstring::npos
        ? L"."
        : currentPath.substr(0, separator);
    const std::wstring targetPath =
        directory + L"\\Spencer-Macro-Utilities-V3.4.0-Windows.exe";

    bool createdTarget = false;
    const DWORD targetAttributes = GetFileAttributesW(targetPath.c_str());
    if (targetAttributes == INVALID_FILE_ATTRIBUTES) {
        if (!CopyFileW(currentPath.c_str(), targetPath.c_str(), TRUE)) {
            MessageBoxW(
                nullptr,
                L"Spencer Macro Utilities V3.4.0 was installed through the legacy updater, but Windows prevented it from creating the new versioned executable filename. The app will continue using suspend.exe for this launch and will retry next time. You can also download the versioned executable from the GitHub releases page.",
                L"Spencer Macro Utilities filename migration",
                MB_OK | MB_ICONWARNING);
            return false;
        }
        createdTarget = true;
    } else if ((targetAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0 ||
               !FilesHaveIdenticalBytes(currentPath, targetPath)) {
        MessageBoxW(
            nullptr,
            L"Spencer Macro Utilities V3.4.0 needs to rename the legacy suspend.exe installation, but a different file already exists at Spencer-Macro-Utilities-V3.4.0-Windows.exe. Nothing was overwritten. The app will continue using suspend.exe for this launch.",
            L"Spencer Macro Utilities filename migration",
            MB_OK | MB_ICONWARNING);
        return false;
    }

    const std::wstring scriptPath =
        directory + L"\\.smu-v3.4.0-filename-migration-" +
        std::to_wstring(GetCurrentProcessId()) + L"-" +
        std::to_wstring(GetTickCount64()) + L".cmd";
    const std::string scriptContents =
        "@echo off\r\n"
        "setlocal\r\n"
        "set \"SMU_OLD=%~1\"\r\n"
        "set \"SMU_NEW=%~2\"\r\n"
        ":wait_for_old_image\r\n"
        "del /F /Q \"%SMU_OLD%\" > NUL 2>&1\r\n"
        "if not exist \"%SMU_OLD%\" goto launch_new\r\n"
        "timeout /t 1 /nobreak > NUL\r\n"
        "goto wait_for_old_image\r\n"
        ":launch_new\r\n"
        "start \"\" \"%SMU_NEW%\"\r\n"
        "endlocal\r\n"
        "set \"SMU_HELPER=%~f0\"\r\n"
        "start \"\" /B powershell.exe -NoProfile -NonInteractive -WindowStyle Hidden -Command \"Start-Sleep -Milliseconds 500; Remove-Item -LiteralPath $env:SMU_HELPER -Force\"\r\n"
        "exit /B\r\n";

    if (!WriteLegacyFilenameMigrationScript(scriptPath, scriptContents)) {
        DeleteFileW(scriptPath.c_str());
        if (createdTarget) {
            DeleteFileW(targetPath.c_str());
        }
        MessageBoxW(
            nullptr,
            L"Spencer Macro Utilities V3.4.0 could not create its one-time filename migration helper. The app will continue using suspend.exe for this launch and will retry next time.",
            L"Spencer Macro Utilities filename migration",
            MB_OK | MB_ICONWARNING);
        return false;
    }

    wchar_t systemDirectory[MAX_PATH] = {};
    const UINT systemDirectoryLength = GetSystemDirectoryW(systemDirectory, MAX_PATH);
    if (systemDirectoryLength == 0 || systemDirectoryLength >= MAX_PATH) {
        DeleteFileW(scriptPath.c_str());
        if (createdTarget) {
            DeleteFileW(targetPath.c_str());
        }
        MessageBoxW(
            nullptr,
            L"Spencer Macro Utilities V3.4.0 could not resolve the Windows command interpreter needed for its filename migration. The app will continue using suspend.exe for this launch and will retry next time.",
            L"Spencer Macro Utilities filename migration",
            MB_OK | MB_ICONWARNING);
        return false;
    }

    const std::wstring commandInterpreter =
        std::wstring(systemDirectory, systemDirectoryLength) + L"\\cmd.exe";
    std::wstring commandLine =
        L"cmd.exe /D /Q /C \"\"" + scriptPath + L"\" \"" +
            currentPath + L"\" \"" + targetPath + L"\"\"";

    STARTUPINFOW startupInfo = {};
    startupInfo.cb = sizeof(startupInfo);
    PROCESS_INFORMATION processInfo = {};
    if (!CreateProcessW(
            commandInterpreter.c_str(),
            commandLine.data(),
            nullptr,
            nullptr,
            FALSE,
            CREATE_NO_WINDOW,
            nullptr,
            directory.c_str(),
            &startupInfo,
            &processInfo)) {
        DeleteFileW(scriptPath.c_str());
        if (createdTarget) {
            DeleteFileW(targetPath.c_str());
        }
        MessageBoxW(
            nullptr,
            L"Spencer Macro Utilities V3.4.0 could not start its one-time filename migration helper. The app will continue using suspend.exe for this launch and will retry next time.",
            L"Spencer Macro Utilities filename migration",
            MB_OK | MB_ICONWARNING);
        return false;
    }

    CloseHandle(processInfo.hThread);
    CloseHandle(processInfo.hProcess);
    return true;
}

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
            // This mask was added with Windows 11. Older Windows versions still
            // honor the timeBeginPeriod request above; they simply cannot opt out
            // of Windows 11's hidden-window timer-resolution policy. The retry
            // succeeded, so this is a normal compatibility path, not a user issue.
            LogInfo("Windows does not support the ignore-timer-resolution power-throttling flag; using the compatible execution-speed-only policy. Error=" +
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

    // V3.4.0-only: legacy updaters install this build as suspend.exe. Schedule
    // the one-time filename migration and exit before any GUI/backend/macro work.
    if (ScheduleV340LegacyFilenameMigration()) {
        return 0;
    }

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

    // Save while imported script records still exist; MacroRuntime::stop() clears them.
    smu::app::ShutdownSharedProfiles();
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

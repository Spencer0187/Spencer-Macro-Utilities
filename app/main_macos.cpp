#if defined(__APPLE__)

#include "app_context.h"
#include "app_main.h"
#include "macro_runtime.h"

#include "../core/app_state.h"
#include "../core/legacy_globals.h"
#include "../core/macro_state.h"
#include "../platform/input_backend.h"
#include "../platform/macos/app_icon_macos.h"
#include "../platform/logging.h"
#include "../platform/macos/macos_backends.h"
#include "../platform/macos/permissions_macos.h"
#include "../platform/network_backend.h"
#include "../platform/process_backend.h"

#include <atomic>
#include <cerrno>
#include <cstdint>
#include <cstring>
#include <cstdlib>
#include <filesystem>
#include <mach-o/dyld.h>
#include <memory>
#include <optional>
#include <spawn.h>
#include <string>
#include <system_error>
#include <vector>

extern char** environ;

namespace {

constexpr const char kMacOSInputBackendInitWarningId[] = "macos_input_backend_init_failed";
constexpr const char kMacOSProcessBackendInitWarningId[] = "macos_process_backend_init_failed";
constexpr const char kMacOSNetworkBackendUnavailableWarningId[] = "macos_network_backend_unavailable";
constexpr const char kMacOSCapabilityWarningId[] = "macos_platform_capability_unavailable";
constexpr const char kMacOSPermissionRepairMarkerName[] = "macos-permissions-may-need-repair";

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

bool RelaunchCurrentMacOSApp()
{
    const auto bundlePath = CurrentAppBundlePath();
    if (!bundlePath) {
        LogWarning("Could not resolve current macOS app bundle path for restart.");
        return false;
    }

    const std::string bundlePathString = bundlePath->string();
    pid_t child = 0;
    char* const argv[] = {
        const_cast<char*>("/usr/bin/open"),
        const_cast<char*>("-n"),
        const_cast<char*>(bundlePathString.c_str()),
        nullptr,
    };
    const int result = posix_spawn(&child, "/usr/bin/open", nullptr, nullptr, argv, environ);
    if (result != 0) {
        LogWarning("Could not relaunch SMU through /usr/bin/open: " + std::string(std::strerror(result)));
        return false;
    }

    auto& appState = smu::core::GetAppState();
    appState.done.store(true, std::memory_order_release);
    appState.running.store(false, std::memory_order_release);
    Globals::done.store(true, std::memory_order_release);
    Globals::running.store(false, std::memory_order_release);
    return true;
}

std::filesystem::path MacOSApplicationSupportDirectory()
{
    const char* home = std::getenv("HOME");
    if (!home || home[0] == '\0') {
        return {};
    }

    return std::filesystem::path(home) /
        "Library" /
        "Application Support" /
        "Spencer Macro Utilities";
}

std::filesystem::path MacOSPermissionRepairMarkerPath()
{
    const std::filesystem::path supportDir = MacOSApplicationSupportDirectory();
    if (supportDir.empty()) {
        return {};
    }
    return supportDir / kMacOSPermissionRepairMarkerName;
}

bool ConsumeMacOSPermissionRepairMarker()
{
    const std::filesystem::path markerPath = MacOSPermissionRepairMarkerPath();
    if (markerPath.empty()) {
        return false;
    }

    std::error_code ec;
    const bool exists = std::filesystem::exists(markerPath, ec);
    if (!exists || ec) {
        return false;
    }

    std::filesystem::remove(markerPath, ec);
    return true;
}

std::string BuildMacOSPermissionDetails(
    const smu::platform::macos::MacosPermissionStatus& status,
    bool permissionRepairRecommended)
{
    std::string details =
        "Accessibility for macro input: ";
    details += status.accessibilityTrusted ? "Granted" : "Missing";
    details += "\nScreen Recording for Lua pixel reads: ";
    details += status.screenRecordingTrusted ? "Granted" : "Missing";
    if (!status.inputSetupReady()) {
        details +=
            "\n\nIf System Settings already shows SMU enabled but this still says Missing, "
            "use Reset macOS Permission Entries below, approve the current app again, "
            "and restart SMU.";
    }
    if (permissionRepairRecommended) {
        details +=
            "\n\nSMU was just updated. If permissions do not work after this launch, "
            "reset the macOS permission entries and approve SMU again.";
    }
    return details;
}

void UpdateMacOSPermissionContext(
    smu::app::AppContext& context,
    const smu::platform::macos::MacosPermissionStatus& status,
    bool permissionRepairRecommended = false)
{
    context.platformSetupRequired = !status.inputSetupReady();
    context.platformSetupTitle = "macOS Input Permissions";
    context.platformSetupSummary = status.inputSetupReady()
        ? "macOS permissions are configured."
        : "Grant Accessibility and Screen Recording, then restart SMU.";
    context.platformSetupDetails = BuildMacOSPermissionDetails(status, permissionRepairRecommended);
    context.platformAccessibilityPermissionGranted = status.accessibilityTrusted;
    context.platformScreenRecordingPermissionGranted = status.screenRecordingTrusted;
    if (!context.platformSetupRequired) {
        context.platformSetupDismissed = false;
    }
}

void InitializeMacOSInputBackend(
    smu::app::AppContext& context,
    const std::shared_ptr<smu::platform::InputBackend>& inputBackend,
    bool permissionRepairRecommended = false)
{
    const smu::platform::macos::MacosPermissionStatus permissions =
        smu::platform::macos::GetMacosPermissionStatus();
    UpdateMacOSPermissionContext(context, permissions, permissionRepairRecommended);

    if (!inputBackend) {
        context.inputBackendAvailable = false;
        context.inputBackendError = "macOS input backend is unavailable.";
        return;
    }

    if (!permissions.accessibilityTrusted) {
        inputBackend->shutdown();
        context.inputBackendAvailable = false;
        context.inputBackendError =
            "macOS Accessibility permission is required to send global keyboard/mouse input.";
        return;
    }

    if (!permissions.screenRecordingTrusted) {
        inputBackend->shutdown();
        context.inputBackendAvailable = false;
        context.inputBackendError =
            "macOS Screen Recording permission is required before SMU starts macro input.";
        return;
    }

    context.inputBackendError.clear();
    context.inputBackendAvailable = inputBackend->init(&context.inputBackendError);
    if (!context.inputBackendAvailable && !context.inputBackendError.empty()) {
        LogWarning(context.inputBackendError, kMacOSInputBackendInitWarningId, true);
    }
}

} // namespace

int main(int argc, char** argv)
{
    (void)argc;
    (void)argv;

    const bool workingDirectoryUpdated = smu::app::SetWorkingDirectoryToExecutablePath();
    smu::log::SetFileLoggingEnabled(smu::log::IsDebugLoggingEnabled());
    if (!workingDirectoryUpdated) {
        LogWarning("Failed to set the working directory to the macOS executable path.");
    }
    LogInfo("Starting Spencer Macro Utilities native macOS app.");
    if (!smu::platform::macos::ApplyApplicationIconFromBundle()) {
        LogWarning("Failed to apply the bundled macOS application icon at runtime.");
    }

    smu::core::InitializeMacroSections(false);
    if (Globals::presskey_instances.empty()) Globals::presskey_instances.emplace_back();
    if (Globals::wallhop_instances.empty()) Globals::wallhop_instances.emplace_back();
    if (Globals::spamkey_instances.empty()) Globals::spamkey_instances.emplace_back();

    smu::platform::macos::InitializeMacOSPlatformBackends();
    smu::app::AppContext context = smu::app::CreateAppContext();
    const bool permissionRepairRecommended = ConsumeMacOSPermissionRepairMarker();

    const std::shared_ptr<smu::platform::InputBackend> inputBackend =
        smu::platform::GetInputBackend();
    InitializeMacOSInputBackend(context, inputBackend, permissionRepairRecommended);
    context.refreshPlatformPermissions = [&context, inputBackend]() {
        context.platformSetupDismissed = false;
        context.platformSetupActionMessage = "Rechecked macOS privacy permissions.";
        InitializeMacOSInputBackend(context, inputBackend);
    };
    context.requestAccessibilityPermission = [&context, inputBackend]() {
        context.platformSetupDismissed = false;
        const bool trusted = smu::platform::macos::RequestAccessibilityPermission();
        const smu::platform::macos::MacosPermissionStatus status =
            smu::platform::macos::GetMacosPermissionStatus();
        if (trusted || status.accessibilityTrusted) {
            context.platformSetupActionMessage =
                "Accessibility permission is granted. Restart and check permissions.";
        } else if (smu::platform::macos::OpenAccessibilitySettings()) {
            context.platformSetupActionMessage =
                "Accessibility permission requested. Approve SMU, then restart and check permissions.";
        } else {
            context.platformSetupActionMessage =
                "macOS could not open Accessibility settings automatically.";
        }
        UpdateMacOSPermissionContext(context, status);
        InitializeMacOSInputBackend(context, inputBackend);
    };
    context.requestScreenRecordingPermission = [&context]() {
        context.platformSetupDismissed = false;
        if (smu::platform::macos::HasScreenRecordingPermission()) {
            context.platformSetupActionMessage = "Screen Recording permission is already granted.";
            UpdateMacOSPermissionContext(
                context,
                smu::platform::macos::GetMacosPermissionStatus());
            return;
        }

        const bool trusted = smu::platform::macos::RequestScreenRecordingPermission();
        const smu::platform::macos::MacosPermissionStatus status =
            smu::platform::macos::GetMacosPermissionStatus();
        if (trusted || status.screenRecordingTrusted) {
            context.platformSetupActionMessage =
                "Screen Recording permission is granted. Restart and check permissions.";
        } else if (smu::platform::macos::OpenScreenRecordingSettings()) {
            context.platformSetupActionMessage =
                "Screen Recording settings opened. Approve SMU, then restart and check permissions.";
        } else {
            context.platformSetupActionMessage =
                "macOS could not open Screen Recording settings automatically.";
        }
        UpdateMacOSPermissionContext(context, status);
    };
    context.resetMacOSPermissionEntries = [&context, inputBackend]() {
        context.platformSetupDismissed = false;
        const smu::platform::macos::MacosPermissionResetResult resetResult =
            smu::platform::macos::ResetMacosPermissionEntries();
        context.platformSetupActionMessage = resetResult.message;
        if (resetResult.ok()) {
            smu::platform::macos::OpenPrivacySecuritySettings();
        }
        InitializeMacOSInputBackend(context, inputBackend);
    };
    context.openPrivacySettings = [&context]() {
        context.platformSetupDismissed = false;
        if (smu::platform::macos::OpenPrivacySecuritySettings()) {
            context.platformSetupActionMessage =
                "Privacy & Security Settings opened. Approve SMU, then restart and check permissions.";
        } else {
            context.platformSetupActionMessage =
                "macOS could not open Privacy & Security Settings automatically.";
        }
    };
    context.restartApplication = [&context]() {
        if (RelaunchCurrentMacOSApp()) {
            context.platformSetupActionMessage = "Restarting SMU...";
            return true;
        }
        context.platformSetupActionMessage = "Could not restart SMU automatically. Quit and reopen the app.";
        return false;
    };

    if (auto processBackend = smu::platform::GetProcessBackend()) {
        context.processBackendAvailable = processBackend->init(&context.processBackendError);
        if (!context.processBackendAvailable && !context.processBackendError.empty()) {
            LogWarning(context.processBackendError, kMacOSProcessBackendInitWarningId, true);
        }
    }

    if (auto networkBackend = smu::platform::GetNetworkLagBackend()) {
        context.networkBackendAvailable = networkBackend->isAvailable();
        context.networkBackendError = networkBackend->unsupportedReason();
        if (!context.networkBackendError.empty()) {
            LogWarning(context.networkBackendError, kMacOSNetworkBackendUnavailableWarningId, true);
        }
    }

    for (const std::string& warning : context.capabilities.warnings) {
        LogWarning(warning, kMacOSCapabilityWarningId, true);
    }
    for (const std::string& error : context.capabilities.criticalErrors) {
        LogCritical(error);
    }

    smu::app::MacroRuntime macroRuntime;
    macroRuntime.start();

    const int result = smu::app::RunSharedApp(context);

    macroRuntime.stop();

    if (auto networkBackend = smu::platform::GetNetworkLagBackend()) {
        networkBackend->shutdown();
    }
    if (auto processBackend = smu::platform::GetProcessBackend()) {
        processBackend->shutdown();
    }
    if (auto inputBackend = smu::platform::GetInputBackend()) {
        inputBackend->shutdown();
    }

    LogInfo("Spencer Macro Utilities native macOS app stopped.");
    return result;
}

#endif

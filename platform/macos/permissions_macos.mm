#include "permissions_macos.h"

#if defined(__APPLE__)

#import <AppKit/AppKit.h>
#include <ApplicationServices/ApplicationServices.h>
#include <CoreFoundation/CoreFoundation.h>
#include <CoreGraphics/CoreGraphics.h>

#include <cerrno>
#include <cstring>
#include <sstream>
#include <sys/wait.h>
#include <unistd.h>

namespace smu::platform::macos {
namespace {

constexpr const char kSmuBundleIdentifier[] = "com.spencer0187.smu";

bool OpenSettingsUrl(NSString* urlString)
{
    @autoreleasepool {
        NSURL* url = [NSURL URLWithString:urlString];
        return url && [[NSWorkspace sharedWorkspace] openURL:url];
    }
}

bool OpenSettingsPane(NSString* pane)
{
    @autoreleasepool {
        NSString* urlString = [@"x-apple.systempreferences:com.apple.preference.security?"
            stringByAppendingString:pane];
        return OpenSettingsUrl(urlString) ||
            OpenSettingsUrl(@"x-apple.systempreferences:com.apple.preference.security?Privacy");
    }
}

std::string WaitStatusMessage(const char* service, int status)
{
    std::ostringstream message;
    message << "tccutil reset " << service;
    if (WIFEXITED(status)) {
        message << " exited with code " << WEXITSTATUS(status);
    } else if (WIFSIGNALED(status)) {
        message << " was terminated by signal " << WTERMSIG(status);
    } else {
        message << " did not complete successfully";
    }
    message << ".";
    return message.str();
}

bool ResetTccService(const char* service, std::string& errorMessage)
{
    const pid_t pid = fork();
    if (pid < 0) {
        errorMessage = std::string("Failed to launch tccutil for ") + service + ": " + std::strerror(errno);
        return false;
    }

    if (pid == 0) {
        execl(
            "/usr/bin/tccutil",
            "tccutil",
            "reset",
            service,
            kSmuBundleIdentifier,
            static_cast<char*>(nullptr));
        _exit(errno == ENOENT ? 127 : 126);
    }

    int status = 0;
    if (waitpid(pid, &status, 0) < 0) {
        errorMessage = std::string("Failed waiting for tccutil reset ") + service + ": " + std::strerror(errno);
        return false;
    }

    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        errorMessage = WaitStatusMessage(service, status);
        return false;
    }

    return true;
}

} // namespace

bool IsAccessibilityTrusted()
{
    return AXIsProcessTrusted();
}

bool RequestAccessibilityPermission()
{
    const void* keys[] = { kAXTrustedCheckOptionPrompt };
    const void* values[] = { kCFBooleanTrue };
    CFDictionaryRef options = CFDictionaryCreate(
        kCFAllocatorDefault,
        keys,
        values,
        1,
        &kCFTypeDictionaryKeyCallBacks,
        &kCFTypeDictionaryValueCallBacks);
    if (!options) {
        return AXIsProcessTrusted();
    }

    const bool trusted = AXIsProcessTrustedWithOptions(options);
    CFRelease(options);
    return trusted;
}

bool HasScreenRecordingPermission()
{
    return CGPreflightScreenCaptureAccess();
}

bool RequestScreenRecordingPermission()
{
    return CGRequestScreenCaptureAccess();
}

bool OpenAccessibilitySettings()
{
    return OpenSettingsPane(@"Privacy_Accessibility");
}

bool OpenScreenRecordingSettings()
{
    return OpenSettingsPane(@"Privacy_ScreenCapture");
}

bool OpenPrivacySecuritySettings()
{
    return OpenSettingsPane(@"Privacy");
}

MacosPermissionResetResult ResetMacosPermissionEntries()
{
    MacosPermissionResetResult result;

    std::string accessibilityError;
    result.accessibilityReset = ResetTccService("Accessibility", accessibilityError);

    std::string screenRecordingError;
    result.screenRecordingReset = ResetTccService("ScreenCapture", screenRecordingError);

    if (result.ok()) {
        result.message =
            "macOS privacy entries for SMU were reset. Re-enable Accessibility and Screen Recording, then restart SMU.";
    } else {
        std::ostringstream message;
        message << "Could not reset every macOS privacy entry.";
        if (!accessibilityError.empty()) {
            message << " Accessibility: " << accessibilityError;
        }
        if (!screenRecordingError.empty()) {
            message << " Screen Recording: " << screenRecordingError;
        }
        result.message = message.str();
    }

    return result;
}

MacosPermissionStatus GetMacosPermissionStatus()
{
    return MacosPermissionStatus{
        IsAccessibilityTrusted(),
        HasScreenRecordingPermission(),
    };
}

} // namespace smu::platform::macos

#endif

#pragma once

#include <string>

namespace smu::platform::macos {

struct MacosPermissionStatus {
    bool accessibilityTrusted = false;
    bool screenRecordingTrusted = false;

    bool inputSetupReady() const
    {
        return accessibilityTrusted && screenRecordingTrusted;
    }
};

struct MacosPermissionResetResult {
    bool accessibilityReset = false;
    bool screenRecordingReset = false;
    std::string message;

    bool ok() const
    {
        return accessibilityReset && screenRecordingReset;
    }
};

bool IsAccessibilityTrusted();
bool RequestAccessibilityPermission();
bool HasScreenRecordingPermission();
bool RequestScreenRecordingPermission();
bool OpenAccessibilitySettings();
bool OpenScreenRecordingSettings();
bool OpenPrivacySecuritySettings();
MacosPermissionResetResult ResetMacosPermissionEntries();
MacosPermissionStatus GetMacosPermissionStatus();

} // namespace smu::platform::macos

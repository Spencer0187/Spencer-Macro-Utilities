#pragma once

#include <string>

namespace smu::app {

inline constexpr const char kAdminElevationWarningId[] = "admin_elevation_warning";

bool IsNotificationSuppressed(const std::string& id);
void SetNotificationSuppressed(const std::string& id, bool suppressed);

} // namespace smu::app

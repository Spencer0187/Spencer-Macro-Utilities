#pragma once

#include "../platform_types.h"

#include <optional>
#include <string>
#include <vector>

namespace smu::platform::linux {

bool IsX11ForegroundDetectionAvailable();
std::optional<PlatformPid> GetX11ForegroundProcess(std::string* errorMessage = nullptr);
bool IsX11ForegroundProcess(const std::vector<PlatformPid>& targetPids, std::string* errorMessage = nullptr);

} // namespace smu::platform::linux

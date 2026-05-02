#pragma once

#include <filesystem>
#include <string>

namespace smu::app {

std::filesystem::path GetExecutableBasePath();
std::filesystem::path FindRuntimeAsset(const std::string& assetName);

} // namespace smu::app

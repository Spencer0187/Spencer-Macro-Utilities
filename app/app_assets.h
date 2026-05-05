#pragma once

#include <cstddef>
#include <filesystem>
#include <optional>
#include <string>

namespace smu::app {

struct RuntimeAssetData {
    const unsigned char* data = nullptr;
    std::size_t size = 0;
};

std::filesystem::path GetExecutableBasePath();
std::filesystem::path FindRuntimeAsset(const std::string& assetName);
std::optional<RuntimeAssetData> FindRuntimeAssetData(const std::string& assetName);

} // namespace smu::app

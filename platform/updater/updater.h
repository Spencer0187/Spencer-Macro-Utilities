#pragma once

#include "updater_types.h"

#include <filesystem>
#include <optional>
#include <string>
#include <vector>

namespace smu::updater {

std::string NormalizeVersion(std::string version);
int CompareVersions(const std::string& lhs, const std::string& rhs);

UpdaterStatus CheckForUpdate(const std::string& localVersion);
std::optional<ReleaseInfo> FetchLatestRelease(std::string* errorMessage = nullptr);
std::optional<ReleaseAsset> SelectUpdateAsset(const ReleaseInfo& release);

bool DownloadAssetToFile(
    const ReleaseAsset& asset,
    const std::filesystem::path& destination,
    std::string* errorMessage = nullptr);

bool DownloadAssetToMemory(
    const ReleaseAsset& asset,
    std::vector<char>& data,
    std::string* errorMessage = nullptr);

bool ExtractUpdatePackage(
    const std::vector<char>& packageBytes,
    const std::string& fileNameToExtract,
    std::vector<char>& extractedData,
    std::string* errorMessage = nullptr);

bool ApplyUpdate(
    const ReleaseInfo& release,
    const std::string& localVersion,
    std::string* errorMessage = nullptr);

bool IsAutoApplySupported();

} // namespace smu::updater

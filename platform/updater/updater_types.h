#pragma once

#include <optional>
#include <string>
#include <vector>

namespace smu::updater {

struct ReleaseAsset {
    std::string name;
    std::string downloadUrl;
    std::size_t sizeBytes = 0;
};

struct ReleaseInfo {
    std::string tagName;
    std::string version;
    std::string htmlUrl;
    std::vector<ReleaseAsset> assets;
};

struct UpdaterStatus {
    bool checkSucceeded = false;
    bool updateAvailable = false;
    bool autoApplySupported = false;
    std::string localVersion;
    std::string latestVersion;
    std::string message;
    std::optional<ReleaseInfo> latestRelease;
    std::optional<ReleaseAsset> selectedAsset;
};

} // namespace smu::updater

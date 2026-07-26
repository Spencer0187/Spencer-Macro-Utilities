#pragma once

#include <algorithm>
#include <cctype>
#include <string>

namespace smu::updater::detail {

enum class LinuxArchitecture {
    Unknown,
    X86_64,
    AArch64,
};

inline std::string LowerAssetName(std::string value)
{
    std::transform(value.begin(), value.end(), value.begin(), [](unsigned char ch) {
        return static_cast<char>(std::tolower(ch));
    });
    return value;
}

inline bool AssetNameContains(const std::string& value, const char* needle)
{
    return value.find(needle) != std::string::npos;
}

inline bool AssetNameEndsWith(const std::string& value, const char* suffix)
{
    const std::string suffixText(suffix);
    return value.size() >= suffixText.size() &&
        value.compare(value.size() - suffixText.size(), suffixText.size(), suffixText) == 0;
}

inline int ScoreWindowsAssetName(const std::string& assetName)
{
    const std::string name = LowerAssetName(assetName);
    if (!AssetNameEndsWith(name, ".zip")) {
        return 0;
    }
    if (AssetNameContains(name, "linux") ||
        AssetNameContains(name, "appimage") ||
        AssetNameContains(name, "macos") ||
        AssetNameContains(name, "mac-os") ||
        AssetNameContains(name, "darwin") ||
        AssetNameContains(name, "osx") ||
        AssetNameContains(name, "universal")) {
        return 0;
    }

    // Keep accepting the pre-3.3 generic Spencer-Macro-Utilities-Vx.y.z.zip
    // naming while preferring an explicitly named Windows package.
    int score = 10;
    if (AssetNameContains(name, "windows") ||
        AssetNameContains(name, "win64") ||
        AssetNameContains(name, "win32") ||
        AssetNameContains(name, "win-x64") ||
        AssetNameContains(name, "win_x64")) {
        score += 50;
    }
    if (AssetNameContains(name, "x64") || AssetNameContains(name, "amd64")) {
        score += 10;
    }
    if (AssetNameContains(name, "spencer") ||
        AssetNameContains(name, "macro") ||
        AssetNameContains(name, "suspend")) {
        score += 8;
    }
    return score;
}

inline int ScoreMacOSAssetName(const std::string& assetName)
{
    const std::string name = LowerAssetName(assetName);
    if (!AssetNameEndsWith(name, ".zip")) {
        return 0;
    }
    if (AssetNameContains(name, "windows") ||
        AssetNameContains(name, "win64") ||
        AssetNameContains(name, "win32") ||
        AssetNameContains(name, "win-x64") ||
        AssetNameContains(name, "win_x64") ||
        AssetNameContains(name, "linux") ||
        AssetNameContains(name, "appimage")) {
        return 0;
    }

    const bool namesMacOS =
        AssetNameContains(name, "macos") ||
        AssetNameContains(name, "mac-os") ||
        AssetNameContains(name, "darwin") ||
        AssetNameContains(name, "osx") ||
        AssetNameContains(name, "universal");
    if (!namesMacOS) {
        return 0;
    }

    int score = 80;
    if (AssetNameContains(name, "universal")) {
        score += 20;
    }
    if (AssetNameContains(name, "spencer") ||
        AssetNameContains(name, "macro") ||
        AssetNameContains(name, "suspend")) {
        score += 8;
    }
    return score;
}

inline bool AssetTargetsDifferentLinuxArchitecture(
    const std::string& lowerName,
    LinuxArchitecture architecture)
{
    if (architecture == LinuxArchitecture::X86_64) {
        return AssetNameContains(lowerName, "aarch64") ||
            AssetNameContains(lowerName, "arm64");
    }
    if (architecture == LinuxArchitecture::AArch64) {
        return AssetNameContains(lowerName, "x86_64") ||
            AssetNameContains(lowerName, "x86-64") ||
            AssetNameContains(lowerName, "amd64") ||
            AssetNameContains(lowerName, "x64");
    }
    return false;
}

inline int ScoreLinuxAssetName(
    const std::string& assetName,
    LinuxArchitecture architecture)
{
    const std::string name = LowerAssetName(assetName);
    const bool appImage = AssetNameEndsWith(name, ".appimage");
    const bool namesDifferentPlatform =
        AssetNameContains(name, "windows") ||
        AssetNameContains(name, "win64") ||
        AssetNameContains(name, "win32") ||
        AssetNameContains(name, "win-x64") ||
        AssetNameContains(name, "win_x64") ||
        AssetNameContains(name, "macos") ||
        AssetNameContains(name, "mac-os") ||
        AssetNameContains(name, "darwin") ||
        AssetNameContains(name, "osx");
    const bool distributionBundle =
        AssetNameEndsWith(name, ".zip") &&
        AssetNameContains(name, "linux") &&
        !namesDifferentPlatform;
    if ((!appImage && !distributionBundle) ||
        namesDifferentPlatform ||
        AssetTargetsDifferentLinuxArchitecture(name, architecture)) {
        return 0;
    }

    int score = appImage ? 300 : 200;
    if (AssetNameContains(name, "spencer") || AssetNameContains(name, "macro")) {
        score += 10;
    }
    return score;
}

} // namespace smu::updater::detail

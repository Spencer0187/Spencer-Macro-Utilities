#include "app_assets.h"

#define SDL_MAIN_HANDLED
#include <SDL3/SDL.h>

#include <algorithm>
#include <system_error>
#include <vector>

#if defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#endif

namespace smu::app {
namespace {

std::string NormalizeAssetName(std::string assetName)
{
    std::replace(assetName.begin(), assetName.end(), '\\', '/');
    constexpr const char* assetsPrefix = "assets/";
    if (assetName.rfind(assetsPrefix, 0) == 0) {
        assetName.erase(0, std::char_traits<char>::length(assetsPrefix));
    }
    return assetName;
}

#if defined(_WIN32)
const wchar_t* ResourceNameForAsset(const std::string& assetName)
{
    if (assetName == "LSANS.TTF") {
        return L"LSANS_TTF";
    }
    if (assetName == "smu_icon.bmp") {
        return L"SMU_ICON_BMP";
    }
    if (assetName == "macro_tutorials/fullgeardesync.png") {
        return L"SMU_TUTORIAL_FULLGEARDESYNC_PNG";
    }
    if (assetName == "macro_tutorials/gear-clip.jpg") {
        return L"SMU_TUTORIAL_GEAR_CLIP_JPG";
    }
    if (assetName == "macro_tutorials/laugh.jpg") {
        return L"SMU_TUTORIAL_LAUGH_JPG";
    }
    if (assetName == "macro_tutorials/wallhop.jpg") {
        return L"SMU_TUTORIAL_WALLHOP_JPG";
    }
    if (assetName == "macro_tutorials/wallwalk.jpg") {
        return L"SMU_TUTORIAL_WALLWALK_JPG";
    }
    return nullptr;
}
#endif

} // namespace

std::filesystem::path GetExecutableBasePath()
{
    const char* basePath = SDL_GetBasePath();
    if (basePath && basePath[0] != '\0') {
        return std::filesystem::path(basePath);
    }
    return std::filesystem::current_path();
}

std::filesystem::path FindRuntimeAsset(const std::string& assetName)
{
#if defined(_WIN32)
    (void)assetName;
    return {};
#else
    std::error_code ec;
    const std::filesystem::path normalizedAssetName = NormalizeAssetName(assetName);
    const std::filesystem::path basePath = GetExecutableBasePath();
    std::vector<std::filesystem::path> candidates{
        basePath / "assets" / normalizedAssetName,
    };

#if defined(__APPLE__)
    std::filesystem::path macOSPath = basePath;
    if (macOSPath.filename().empty()) {
        macOSPath = macOSPath.parent_path();
    }
    const std::filesystem::path contentsPath = macOSPath.parent_path();
    candidates.push_back(contentsPath / "Resources" / "assets" / normalizedAssetName);
#endif

    for (const auto& assetPath : candidates) {
        if (std::filesystem::exists(assetPath, ec)) {
            return assetPath;
        }
        ec.clear();
    }
    return {};
#endif
}

std::optional<RuntimeAssetData> FindRuntimeAssetData(const std::string& assetName)
{
#if defined(_WIN32)
    const std::string normalizedAssetName = NormalizeAssetName(assetName);
    const wchar_t* resourceName = ResourceNameForAsset(normalizedAssetName);
    if (!resourceName) {
        return std::nullopt;
    }

    HMODULE moduleHandle = GetModuleHandleW(nullptr);
    HRSRC resource = FindResourceW(moduleHandle, resourceName, RT_RCDATA);
    if (!resource) {
        return std::nullopt;
    }

    HGLOBAL loadedResource = LoadResource(moduleHandle, resource);
    if (!loadedResource) {
        return std::nullopt;
    }

    const DWORD resourceSize = SizeofResource(moduleHandle, resource);
    void* resourceData = LockResource(loadedResource);
    if (!resourceData || resourceSize == 0) {
        return std::nullopt;
    }

    return RuntimeAssetData{
        static_cast<const unsigned char*>(resourceData),
        static_cast<std::size_t>(resourceSize),
    };
#else
    (void)assetName;
    return std::nullopt;
#endif
}

} // namespace smu::app

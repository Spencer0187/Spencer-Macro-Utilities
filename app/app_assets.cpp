#include "app_assets.h"

#define SDL_MAIN_HANDLED
#include <SDL3/SDL.h>

#include <system_error>

namespace smu::app {

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
    std::error_code ec;
    const std::filesystem::path assetPath = GetExecutableBasePath() / "assets" / assetName;
    if (std::filesystem::exists(assetPath, ec)) {
        return assetPath;
    }
    return {};
}

} // namespace smu::app

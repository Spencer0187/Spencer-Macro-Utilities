#include "macro_tutorial_assets.h"

#include "app_assets.h"
#include "../platform/logging.h"

#include "imgui.h"

#define SDL_MAIN_HANDLED
#include <SDL3/SDL.h>
#include <SDL3/SDL_opengl.h>

#include <algorithm>
#include <array>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <string>
#include <vector>

#define STB_IMAGE_STATIC
#define STBI_ONLY_PNG
#define STBI_ONLY_JPEG
#define STBI_NO_GIF
#define STBI_NO_HDR
#define STBI_NO_LINEAR
#define STBI_NO_STDIO
#define STBI_NO_SIMD
#define STB_IMAGE_IMPLEMENTATION
#include "../third_party/SDL/src/video/stb_image.h"

namespace smu::app {
namespace {

struct TutorialTexture {
    const char* assetPath = "";
    GLuint texture = 0;
    int width = 0;
    int height = 0;
};

constexpr std::size_t ImageIndex(MacroTutorialImage image)
{
    return static_cast<std::size_t>(image);
}

std::array<TutorialTexture, 4> g_textures = {{
    {"macro_tutorials/laugh.jpg", 0, 0, 0},
    {"macro_tutorials/gear-clip.jpg", 0, 0, 0},
    {"macro_tutorials/wallhop.jpg", 0, 0, 0},
    {"macro_tutorials/wallwalk.jpg", 0, 0, 0},
}};

bool LoadTexture(TutorialTexture& texture)
{
    const std::filesystem::path path = FindRuntimeAsset(texture.assetPath);
    if (path.empty()) {
        LogWarning(std::string("Macro tutorial image is missing: assets/") + texture.assetPath);
        return false;
    }

    std::ifstream file(path, std::ios::binary);
    if (!file.is_open()) {
        LogWarning(std::string("Macro tutorial image could not be opened: ") + path.string());
        return false;
    }

    std::vector<unsigned char> bytes(
        (std::istreambuf_iterator<char>(file)),
        std::istreambuf_iterator<char>());
    if (bytes.empty() || bytes.size() > 16u * 1024u * 1024u) {
        LogWarning(std::string("Macro tutorial image has an invalid size: ") + path.string());
        return false;
    }

    int width = 0;
    int height = 0;
    int channels = 0;
    unsigned char* pixels = stbi_load_from_memory(bytes.data(), static_cast<int>(bytes.size()), &width, &height, &channels, 4);
    if (!pixels || width <= 0 || height <= 0) {
        LogWarning(std::string("Macro tutorial image could not be loaded: ") + path.string());
        if (pixels) {
            stbi_image_free(pixels);
        }
        return false;
    }

    GLuint glTexture = 0;
    glGenTextures(1, &glTexture);
    glBindTexture(GL_TEXTURE_2D, glTexture);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
    glPixelStorei(GL_UNPACK_ALIGNMENT, 1);
    glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA, width, height, 0, GL_RGBA, GL_UNSIGNED_BYTE, pixels);
    glBindTexture(GL_TEXTURE_2D, 0);
    stbi_image_free(pixels);

    texture.texture = glTexture;
    texture.width = width;
    texture.height = height;
    return true;
}

} // namespace

bool LoadMacroTutorialTextures()
{
    bool anyLoaded = false;
    for (TutorialTexture& texture : g_textures) {
        if (texture.texture == 0) {
            anyLoaded = LoadTexture(texture) || anyLoaded;
        }
    }
    return anyLoaded;
}

void UnloadMacroTutorialTextures()
{
    for (TutorialTexture& texture : g_textures) {
        if (texture.texture != 0) {
            glDeleteTextures(1, &texture.texture);
            texture.texture = 0;
            texture.width = 0;
            texture.height = 0;
        }
    }
}

void RenderCenteredMacroTutorialImage(MacroTutorialImage image, float displayWidth, float displayHeight)
{
    const std::size_t index = ImageIndex(image);
    if (index >= g_textures.size()) {
        return;
    }

    const TutorialTexture& texture = g_textures[index];
    if (texture.texture == 0 || texture.width <= 0 || texture.height <= 0) {
        return;
    }

    const float nativeWidth = static_cast<float>(texture.width);
    const float nativeHeight = static_cast<float>(texture.height);
    float width = displayWidth > 0.0f ? displayWidth : nativeWidth;
    float height = displayHeight > 0.0f ? displayHeight : nativeHeight;
    const float aspect = nativeWidth / nativeHeight;

    if (displayWidth > 0.0f && displayHeight <= 0.0f) {
        height = width / aspect;
    } else if (displayHeight > 0.0f && displayWidth <= 0.0f) {
        width = height * aspect;
    }

    const float availableWidth = ImGui::GetContentRegionAvail().x;
    if (availableWidth > 0.0f && width > availableWidth) {
        const float scale = availableWidth / width;
        width *= scale;
        height *= scale;
    }

    const float offset = std::max(0.0f, (availableWidth - width) * 0.5f);
    ImGui::SetCursorPosX(ImGui::GetCursorPosX() + offset);
    ImGui::Image(static_cast<ImTextureID>(texture.texture), ImVec2(width, height));
}

} // namespace smu::app

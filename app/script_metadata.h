#pragma once

#include "../core/key_codes.h"

#include <filesystem>
#include <optional>
#include <string>

namespace smu::app {

struct ImportedScriptMetadata {
    std::string name;
    std::string description;
    std::string author;
    std::string version;
};

ImportedScriptMetadata ParseScriptMetadata(const std::filesystem::path& path);
std::optional<unsigned int> ParseScriptHotkeyMetadata(const std::filesystem::path& path);
std::optional<unsigned int> ParseScriptHotkeyString(const std::string& text);
std::optional<core::KeyCode> ParseScriptKeyName(const std::string& text);
bool IsSupportedScriptExtension(const std::filesystem::path& path);

} // namespace smu::app

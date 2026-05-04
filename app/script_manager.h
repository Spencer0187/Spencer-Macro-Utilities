#pragma once

#include "script_metadata.h"

#include "json.hpp"

#include <deque>
#include <filesystem>
#include <memory>
#include <string>

#include "json.hpp"

namespace smu::app {

inline constexpr unsigned int kScriptUnboundHotkey = 0x10000000u;
inline bool IsScriptHotkeyBound(unsigned int hotkey)
{
    return (hotkey & smu::core::HOTKEY_KEY_MASK) != 0;
}

class ScriptInstance;

struct ImportedScriptRecord {
    std::filesystem::path path;
    ImportedScriptMetadata metadata;
    unsigned int hotkey = 0;
    bool enabled = false;
    bool disableOutsideRoblox = true;
    bool loaded = false;
    bool missing = false;
    bool running = false;
    std::string lastError;
    nlohmann::json uiState = nlohmann::json::object();
    std::unique_ptr<ScriptInstance> instance;
};

class ScriptManager {
public:
    using Container = std::deque<ImportedScriptRecord>;

    static ScriptManager& Get();

    bool importScript(const std::filesystem::path& path);
    bool importScriptFromSave(const std::filesystem::path& path, unsigned int hotkey, bool enabled, bool disableOutsideRoblox);
    bool reloadScript(std::size_t index);
    bool removeScript(std::size_t index);
    bool executeScript(std::size_t index);
    void clear();
    std::size_t count() const;
    ImportedScriptRecord* get(std::size_t index);
    const ImportedScriptRecord* get(std::size_t index) const;
    Container& scripts() { return scripts_; }
    const Container& scripts() const { return scripts_; }
    nlohmann::json serialize() const;
    void deserialize(const nlohmann::json& value);
    void shutdown();

private:
    bool loadRecord(ImportedScriptRecord& record);
    bool isDuplicatePath(const std::filesystem::path& path) const;
    static std::filesystem::path NormalizePath(const std::filesystem::path& path);

    Container scripts_;
};

} // namespace smu::app

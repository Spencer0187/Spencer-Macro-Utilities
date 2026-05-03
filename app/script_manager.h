#pragma once

#include "script_metadata.h"

#include <deque>
#include <filesystem>
#include <memory>
#include <string>

#include "json.hpp"

namespace smu::app {

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

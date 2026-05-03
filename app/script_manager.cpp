#include "script_manager.h"

#include "script_instance.h"
#include "../platform/logging.h"

#include <system_error>
#include <chrono>
#include <thread>

namespace smu::app {
namespace {

std::string PathForJson(const std::filesystem::path& path)
{
    return path.string();
}

} // namespace

ScriptManager& ScriptManager::Get()
{
    static ScriptManager manager;
    return manager;
}

bool ScriptManager::importScript(const std::filesystem::path& path)
{
    ImportedScriptRecord record;
    record.path = NormalizePath(path);
    record.enabled = false;
    record.disableOutsideRoblox = true;

    if (!IsSupportedScriptExtension(record.path)) {
        LogWarning("Rejected imported script with unsupported extension: " + record.path.string());
        return false;
    }
    if (isDuplicatePath(record.path)) {
        LogWarning("Script is already imported: " + record.path.string());
        return false;
    }

    scripts_.push_back(std::move(record));
    ImportedScriptRecord& stored = scripts_.back();
    if (!loadRecord(stored)) {
        return false;
    }

    if (auto hotkey = ParseScriptHotkeyMetadata(stored.path)) {
        stored.hotkey = *hotkey;
    }
    return true;
}

bool ScriptManager::importScriptFromSave(const std::filesystem::path& path, unsigned int hotkey, bool enabled, bool disableOutsideRoblox)
{
    ImportedScriptRecord record;
    record.path = NormalizePath(path);
    record.hotkey = hotkey;
    record.enabled = enabled;
    record.disableOutsideRoblox = disableOutsideRoblox;

    if (isDuplicatePath(record.path)) {
        return false;
    }

    scripts_.push_back(std::move(record));
    loadRecord(scripts_.back());
    return true;
}

bool ScriptManager::reloadScript(std::size_t index)
{
    ImportedScriptRecord* record = get(index);
    if (!record || record->running) {
        return false;
    }

    const unsigned int preservedHotkey = record->hotkey;
    const bool preservedEnabled = record->enabled;
    const bool preservedDisableOutside = record->disableOutsideRoblox;
    record->instance.reset();
    record->loaded = false;
    record->missing = false;
    record->lastError.clear();

    const bool loaded = loadRecord(*record);
    if (auto hotkey = ParseScriptHotkeyMetadata(record->path)) {
        record->hotkey = *hotkey;
    } else {
        record->hotkey = preservedHotkey;
    }
    record->enabled = preservedEnabled;
    record->disableOutsideRoblox = preservedDisableOutside;
    return loaded;
}

bool ScriptManager::removeScript(std::size_t index)
{
    ImportedScriptRecord* record = get(index);
    if (!record || record->running) {
        return false;
    }
    for (const auto& script : scripts_) {
        if (script.running) {
            return false;
        }
    }
    scripts_.erase(scripts_.begin() + static_cast<std::ptrdiff_t>(index));
    for (ImportedScriptRecord& script : scripts_) {
        if (script.instance) {
            script.instance->setOwner(script);
        }
    }
    return true;
}

bool ScriptManager::executeScript(std::size_t index)
{
    ImportedScriptRecord* record = get(index);
    if (!record || record->running || record->missing || !record->loaded || !record->instance) {
        return false;
    }

    record->running = true;
    const auto clearRunning = [&] {
        record->running = false;
    };

    bool ok = false;
    try {
        ok = record->instance->callOnExecute();
    } catch (const std::exception& e) {
        record->lastError = e.what();
        LogWarning("Imported script threw an exception: " + record->lastError);
    } catch (...) {
        record->lastError = "Script execution failed with an unknown C++ exception.";
        LogWarning(record->lastError);
    }

    clearRunning();
    return ok;
}

void ScriptManager::clear()
{
    const auto waitUntil = std::chrono::steady_clock::now() + std::chrono::seconds(35);
    while (std::chrono::steady_clock::now() < waitUntil) {
        bool anyRunning = false;
        for (const auto& script : scripts_) {
            anyRunning = anyRunning || script.running;
        }
        if (!anyRunning) {
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }

    bool anyRunning = false;
    for (const auto& script : scripts_) {
        anyRunning = anyRunning || script.running;
    }
    if (anyRunning) {
        LogWarning("Imported script was still running during script manager shutdown; keeping it alive until it finishes.");
        for (auto it = scripts_.begin(); it != scripts_.end();) {
            if (it->running) {
                ++it;
                continue;
            }
            if (it->instance) {
                it->instance->cleanup();
            }
            it = scripts_.erase(it);
        }
        return;
    }

    for (auto& script : scripts_) {
        if (script.instance) {
            script.instance->cleanup();
        }
    }
    scripts_.clear();
}

std::size_t ScriptManager::count() const
{
    return scripts_.size();
}

ImportedScriptRecord* ScriptManager::get(std::size_t index)
{
    if (index >= scripts_.size()) {
        return nullptr;
    }
    return &scripts_[index];
}

const ImportedScriptRecord* ScriptManager::get(std::size_t index) const
{
    if (index >= scripts_.size()) {
        return nullptr;
    }
    return &scripts_[index];
}

nlohmann::json ScriptManager::serialize() const
{
    nlohmann::json array = nlohmann::json::array();
    for (const ImportedScriptRecord& script : scripts_) {
        nlohmann::json item;
        item["path"] = PathForJson(script.path);
        item["hotkey"] = script.hotkey;
        item["enabled"] = script.enabled;
        item["disable_outside_roblox"] = script.disableOutsideRoblox;
        array.push_back(std::move(item));
    }
    return array;
}

void ScriptManager::deserialize(const nlohmann::json& value)
{
    clear();
    if (!value.is_array()) {
        return;
    }

    for (const auto& item : value) {
        if (!item.is_object() || !item.contains("path") || !item["path"].is_string()) {
            continue;
        }

        unsigned int hotkey = 0;
        if (item.contains("hotkey") && item["hotkey"].is_number_unsigned()) {
            hotkey = item["hotkey"].get<unsigned int>();
        } else if (item.contains("hotkey") && item["hotkey"].is_number_integer()) {
            const int value = item["hotkey"].get<int>();
            hotkey = value > 0 ? static_cast<unsigned int>(value) : 0;
        }
        if ((hotkey & smu::core::HOTKEY_KEY_MASK) == 0) {
            hotkey = 0;
        }
        const bool enabled = item.contains("enabled") && item["enabled"].is_boolean()
            ? item["enabled"].get<bool>()
            : false;
        const bool disableOutside = item.contains("disable_outside_roblox") && item["disable_outside_roblox"].is_boolean()
            ? item["disable_outside_roblox"].get<bool>()
            : true;
        importScriptFromSave(item["path"].get<std::string>(), hotkey, enabled, disableOutside);
    }
}

void ScriptManager::shutdown()
{
    clear();
}

bool ScriptManager::loadRecord(ImportedScriptRecord& record)
{
    std::error_code ec;
    if (!std::filesystem::exists(record.path, ec) || ec) {
        record.metadata.name = record.path.stem().string();
        record.missing = true;
        record.loaded = false;
        record.lastError = "Script file is missing.";
        return false;
    }

    if (!IsSupportedScriptExtension(record.path)) {
        record.metadata.name = record.path.stem().string();
        record.loaded = false;
        record.lastError = "Unsupported script extension.";
        return false;
    }

    record.metadata = ParseScriptMetadata(record.path);
    record.instance = std::make_unique<ScriptInstance>(record);
    if (!record.instance->init()) {
        record.loaded = false;
        return false;
    }
    if (!record.instance->loadFile(record.path)) {
        record.loaded = false;
        return false;
    }
    if (!record.instance->hasFunction("onExecute")) {
        record.loaded = false;
        record.lastError = "Script does not define onExecute().";
        return false;
    }

    record.missing = false;
    record.loaded = true;
    return true;
}

bool ScriptManager::isDuplicatePath(const std::filesystem::path& path) const
{
    const std::filesystem::path normalized = NormalizePath(path);
    for (const ImportedScriptRecord& script : scripts_) {
        if (NormalizePath(script.path) == normalized) {
            return true;
        }
    }
    return false;
}

std::filesystem::path ScriptManager::NormalizePath(const std::filesystem::path& path)
{
    std::error_code ec;
    std::filesystem::path absolute = std::filesystem::absolute(path, ec);
    if (ec) {
        absolute = path;
    }

    std::filesystem::path normalized = std::filesystem::weakly_canonical(absolute, ec);
    if (ec) {
        normalized = absolute.lexically_normal();
    }
    return normalized;
}

} // namespace smu::app

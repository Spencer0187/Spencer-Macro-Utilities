#include "script_manager.h"

#include "script_instance.h"
#include "../platform/logging.h"

#include <system_error>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <limits>
#include <thread>

namespace smu::app {
namespace {

std::string PathForJson(const std::filesystem::path& path)
{
    return path.string();
}

unsigned int NormalizeScriptHotkey(unsigned int hotkey)
{
    if (!IsScriptHotkeyBound(hotkey)) {
        return kScriptUnboundHotkey;
    }
    return hotkey;
}

bool IsSafeUiStateKey(const std::string& key)
{
    return !key.empty() &&
        key.size() <= kMaxUiIdBytes &&
        std::memchr(key.data(), '\0', key.size()) == nullptr;
}

nlohmann::json SanitizeUiState(const nlohmann::json& state)
{
    nlohmann::json sanitized = nlohmann::json::object();
    if (!state.is_object()) {
        return sanitized;
    }

    for (const auto& [key, value] : state.items()) {
        if (sanitized.size() >= kMaxUiStateEntries) {
            break;
        }
        if (!IsSafeUiStateKey(key)) {
            continue;
        }

        if (value.is_boolean()) {
            sanitized[key] = value.get<bool>();
        } else if (value.is_number_integer()) {
            sanitized[key] = value.get<std::int64_t>();
        } else if (value.is_number_unsigned()) {
            const auto unsignedValue = value.get<std::uint64_t>();
            if (unsignedValue <= static_cast<std::uint64_t>(std::numeric_limits<std::int64_t>::max())) {
                sanitized[key] = static_cast<std::int64_t>(unsignedValue);
            }
        } else if (value.is_number_float()) {
            const double number = value.get<double>();
            if (std::isfinite(number)) {
                sanitized[key] = number;
            }
        } else if (value.is_string()) {
            std::string text = value.get<std::string>();
            if (text.size() > kMaxUiStateStringBytes) {
                text.resize(kMaxUiStateStringBytes);
            }
            sanitized[key] = std::move(text);
        }
    }

    return sanitized;
}

} // namespace

ScriptManager& ScriptManager::Get()
{
    static ScriptManager manager;
    return manager;
}

bool ScriptManager::importScript(const std::filesystem::path& path)
{
    auto record = std::make_shared<ImportedScriptRecord>();
    record->path = NormalizePath(path);
    record->hotkey.store(kScriptUnboundHotkey, std::memory_order_release);
    record->enabled.store(false, std::memory_order_release);
    record->disableOutsideRoblox.store(true, std::memory_order_release);

    if (!IsSupportedScriptExtension(record->path)) {
        LogWarning("Rejected imported script with unsupported extension: " + record->path.string());
        return false;
    }
    {
        std::lock_guard<std::mutex> lock(scriptsMutex_);
        if (isDuplicatePath(record->path)) {
            LogWarning("Script is already imported: " + record->path.string());
            return false;
        }
        scripts_.push_back(record);
    }

    if (!loadRecord(*record)) {
        if (record->instance) {
            record->instance->cleanup();
            record->instance.reset();
        }
        std::lock_guard<std::mutex> lock(scriptsMutex_);
        for (auto it = scripts_.begin(); it != scripts_.end(); ++it) {
            if (*it == record) {
                scripts_.erase(it);
                break;
            }
        }
        return false;
    }

    if (auto hotkey = ParseScriptHotkeyMetadata(record->path)) {
        record->hotkey.store(NormalizeScriptHotkey(*hotkey), std::memory_order_release);
    } else {
        record->hotkey.store(kScriptUnboundHotkey, std::memory_order_release);
    }
    return true;
}

bool ScriptManager::importScriptFromSave(const std::filesystem::path& path, unsigned int hotkey, bool enabled, bool disableOutsideRoblox)
{
    auto record = std::make_shared<ImportedScriptRecord>();
    record->path = NormalizePath(path);
    record->hotkey.store(NormalizeScriptHotkey(hotkey), std::memory_order_release);
    record->enabled.store(enabled, std::memory_order_release);
    record->disableOutsideRoblox.store(disableOutsideRoblox, std::memory_order_release);

    {
        std::lock_guard<std::mutex> lock(scriptsMutex_);
        if (isDuplicatePath(record->path)) {
            return false;
        }
        scripts_.push_back(record);
    }

    loadRecord(*record);
    return true;
}

bool ScriptManager::reloadScript(std::size_t index)
{
    RecordPtr record;
    {
        std::lock_guard<std::mutex> lock(scriptsMutex_);
        if (index >= scripts_.size()) {
            return false;
        }
        record = scripts_[index];
        if (!record) {
            return false;
        }
    }

    if (!record) {
        return false;
    }

    if (record->running.load(std::memory_order_acquire)) {
        if (record->instance) {
            record->instance->requestCancel();
        }
        const auto waitUntil = std::chrono::steady_clock::now() + std::chrono::seconds(5);
        while (record->running.load(std::memory_order_acquire) && std::chrono::steady_clock::now() < waitUntil) {
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }
    }

    {
        std::lock_guard<std::mutex> lock(scriptsMutex_);
        if (!record || record->running.load(std::memory_order_acquire)) {
            return false;
        }
        record->running.store(true, std::memory_order_release);
    }

    const unsigned int preservedHotkey = record->hotkey.load(std::memory_order_acquire);
    const bool preservedEnabled = record->enabled.load(std::memory_order_acquire);
    const bool preservedDisableOutside = record->disableOutsideRoblox.load(std::memory_order_acquire);
    record->instance.reset();
    record->loaded.store(false, std::memory_order_release);
    record->missing.store(false, std::memory_order_release);
    record->clearLastError();

    const bool loaded = loadRecord(*record);
    if (auto hotkey = ParseScriptHotkeyMetadata(record->path)) {
        record->hotkey.store(NormalizeScriptHotkey(*hotkey), std::memory_order_release);
    } else {
        record->hotkey.store(preservedHotkey, std::memory_order_release);
    }
    record->enabled.store(preservedEnabled, std::memory_order_release);
    record->disableOutsideRoblox.store(preservedDisableOutside, std::memory_order_release);
    record->running.store(false, std::memory_order_release);
    return loaded;
}

bool ScriptManager::removeScript(std::size_t index)
{
    RecordPtr removed;
    {
        std::lock_guard<std::mutex> lock(scriptsMutex_);
        if (index >= scripts_.size() || !scripts_[index]) {
            return false;
        }
        removed = scripts_[index];
        if (removed && removed->running.load(std::memory_order_acquire) && removed->instance) {
            removed->instance->requestCancel();
        }
        scripts_.erase(scripts_.begin() + static_cast<std::ptrdiff_t>(index));
    }

    if (removed && removed->instance && !removed->running.load(std::memory_order_acquire)) {
        removed->instance->cleanup();
    }
    return true;
}

bool ScriptManager::executeScript(std::size_t index)
{
    RecordPtr record;
    {
        std::lock_guard<std::mutex> lock(scriptsMutex_);
        if (index >= scripts_.size()) {
            return false;
        }
        record = scripts_[index];
        if (!record ||
            record->running.load(std::memory_order_acquire) ||
            record->missing.load(std::memory_order_acquire) ||
            !record->loaded.load(std::memory_order_acquire) ||
            !record->instance) {
            return false;
        }
        record->running.store(true, std::memory_order_release);
    }

    bool ok = false;
    try {
        ok = record->instance->callOnExecute();
    } catch (const std::exception& e) {
        record->setLastError(e.what());
        LogWarning("Imported script threw an exception: " + record->lastErrorCopy());
    } catch (...) {
        record->setLastError("Script execution failed with an unknown C++ exception.");
        LogWarning(record->lastErrorCopy());
    }

    record->running.store(false, std::memory_order_release);
    return ok;
}

void ScriptManager::clear()
{
    {
        std::lock_guard<std::mutex> lock(scriptsMutex_);
        for (const auto& script : scripts_) {
            if (script && script->running.load(std::memory_order_acquire) && script->instance) {
                script->instance->requestCancel();
            }
        }
    }
    const auto waitUntil = std::chrono::steady_clock::now() + std::chrono::seconds(35);
    while (std::chrono::steady_clock::now() < waitUntil) {
        bool anyRunning = false;
        {
            std::lock_guard<std::mutex> lock(scriptsMutex_);
            for (const auto& script : scripts_) {
                anyRunning = anyRunning || (script && script->running.load(std::memory_order_acquire));
            }
        }
        if (!anyRunning) {
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }

    Container toCleanup;
    bool anyRunning = false;
    {
        std::lock_guard<std::mutex> lock(scriptsMutex_);
        for (const auto& script : scripts_) {
            anyRunning = anyRunning || (script && script->running.load(std::memory_order_acquire));
        }

        if (anyRunning) {
            LogWarning("Imported script was still running during script manager shutdown; keeping it alive until it finishes.");
            for (auto it = scripts_.begin(); it != scripts_.end();) {
                if ((*it) && (*it)->running.load(std::memory_order_acquire)) {
                    ++it;
                    continue;
                }
                toCleanup.push_back(*it);
                it = scripts_.erase(it);
            }
        } else {
            toCleanup.swap(scripts_);
        }
    }

    for (auto& script : toCleanup) {
        if (script && script->instance) {
            script->instance->cleanup();
        }
    }
}

std::size_t ScriptManager::count() const
{
    std::lock_guard<std::mutex> lock(scriptsMutex_);
    return scripts_.size();
}

ScriptManager::RecordPtr ScriptManager::get(std::size_t index)
{
    std::lock_guard<std::mutex> lock(scriptsMutex_);
    if (index >= scripts_.size()) {
        return nullptr;
    }
    return scripts_[index];
}

ScriptManager::ConstRecordPtr ScriptManager::get(std::size_t index) const
{
    std::lock_guard<std::mutex> lock(scriptsMutex_);
    if (index >= scripts_.size()) {
        return nullptr;
    }
    return scripts_[index];
}

std::vector<ScriptManager::RecordPtr> ScriptManager::snapshot() const
{
    std::lock_guard<std::mutex> lock(scriptsMutex_);
    return {scripts_.begin(), scripts_.end()};
}

nlohmann::json ScriptManager::serialize() const
{
    nlohmann::json array = nlohmann::json::array();
    const auto records = snapshot();
    for (const auto& script : records) {
        if (!script) {
            continue;
        }
        nlohmann::json item;
        item["path"] = PathForJson(script->path);
        item["hotkey"] = script->hotkey.load(std::memory_order_acquire);
        item["enabled"] = script->enabled.load(std::memory_order_acquire);
        item["disable_outside_roblox"] = script->disableOutsideRoblox.load(std::memory_order_acquire);
        item["ui_state"] = script->uiState;
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
            const auto value = item["hotkey"].get<std::uint64_t>();
            hotkey = value <= static_cast<std::uint64_t>(std::numeric_limits<unsigned int>::max())
                ? static_cast<unsigned int>(value)
                : 0;
        } else if (item.contains("hotkey") && item["hotkey"].is_number_integer()) {
            const auto value = item["hotkey"].get<std::int64_t>();
            hotkey = value > 0 && value <= static_cast<std::int64_t>(std::numeric_limits<unsigned int>::max())
                ? static_cast<unsigned int>(value)
                : 0;
        }
        hotkey = NormalizeScriptHotkey(hotkey);
        const bool enabled = item.contains("enabled") && item["enabled"].is_boolean()
            ? item["enabled"].get<bool>()
            : false;
        const bool disableOutside = item.contains("disable_outside_roblox") && item["disable_outside_roblox"].is_boolean()
            ? item["disable_outside_roblox"].get<bool>()
            : true;
        const bool imported = importScriptFromSave(item["path"].get<std::string>(), hotkey, enabled, disableOutside);
        if (!imported) {
            continue;
        }

        RecordPtr record;
        {
            std::lock_guard<std::mutex> lock(scriptsMutex_);
            record = scripts_.empty() ? nullptr : scripts_.back();
        }
        if (record && item.contains("ui_state") && item["ui_state"].is_object()) {
            record->uiState = SanitizeUiState(item["ui_state"]);
            if (record->instance) {
                record->instance->syncSettingsTable();
            }
        }
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
        record.missing.store(true, std::memory_order_release);
        record.loaded.store(false, std::memory_order_release);
        record.setLastError("Script file is missing.");
        return false;
    }

    if (!IsSupportedScriptExtension(record.path)) {
        record.metadata.name = record.path.stem().string();
        record.loaded.store(false, std::memory_order_release);
        record.setLastError("Unsupported script extension.");
        return false;
    }

    record.metadata = ParseScriptMetadata(record.path);
    record.instance = std::make_unique<ScriptInstance>(record);
    if (!record.instance->init()) {
        record.loaded.store(false, std::memory_order_release);
        return false;
    }
    if (!record.instance->loadFile(record.path)) {
        record.loaded.store(false, std::memory_order_release);
        return false;
    }
    if (!record.instance->hasFunction("onExecute")) {
        record.loaded.store(false, std::memory_order_release);
        record.setLastError("Script does not define onExecute().");
        return false;
    }

    record.missing.store(false, std::memory_order_release);
    record.loaded.store(true, std::memory_order_release);
    return true;
}

bool ScriptManager::isDuplicatePath(const std::filesystem::path& path) const
{
    const std::filesystem::path normalized = NormalizePath(path);
    for (const auto& script : scripts_) {
        if (script && NormalizePath(script->path) == normalized) {
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

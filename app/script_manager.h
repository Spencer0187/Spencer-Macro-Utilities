#pragma once

#include "script_metadata.h"

#include "json.hpp"

#include <atomic>
#include <deque>
#include <filesystem>
#include <memory>
#include <mutex>
#include <string>
#include <utility>
#include <vector>

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
    std::atomic_uint hotkey{0};
    std::atomic_bool enabled{false};
    std::atomic_bool disableOutsideRoblox{true};
    std::atomic_bool loaded{false};
    std::atomic_bool missing{false};
    std::atomic_bool running{false};
    std::string lastError;
    std::string lastWarning;
    nlohmann::json uiState = nlohmann::json::object();
    std::unique_ptr<ScriptInstance> instance;
    mutable std::mutex errorMutex;
    mutable std::mutex uiStateMutex;

    void setLastError(std::string value)
    {
        std::lock_guard<std::mutex> lock(errorMutex);
        lastError = std::move(value);
    }

    void clearLastError()
    {
        std::lock_guard<std::mutex> lock(errorMutex);
        lastError.clear();
    }

    std::string lastErrorCopy() const
    {
        std::lock_guard<std::mutex> lock(errorMutex);
        return lastError;
    }

    void setLastWarning(std::string value)
    {
        std::lock_guard<std::mutex> lock(errorMutex);
        lastWarning = std::move(value);
    }

    void clearLastWarning()
    {
        std::lock_guard<std::mutex> lock(errorMutex);
        lastWarning.clear();
    }

    std::string lastWarningCopy() const
    {
        std::lock_guard<std::mutex> lock(errorMutex);
        return lastWarning;
    }
};

enum class ScriptImportResult {
    Success,
    UnsupportedExtension,
    AlreadyImported,
    LoadFailed,
};

class ScriptManager {
public:
    using RecordPtr = std::shared_ptr<ImportedScriptRecord>;
    using ConstRecordPtr = std::shared_ptr<const ImportedScriptRecord>;
    using Container = std::deque<RecordPtr>;

    static ScriptManager& Get();

    bool importScript(const std::filesystem::path& path);
    ScriptImportResult importScriptWithResult(const std::filesystem::path& path);
    bool importScriptFromSave(const std::filesystem::path& path, unsigned int hotkey, bool enabled, bool disableOutsideRoblox);
    bool reloadScript(std::size_t index);
    bool removeScript(std::size_t index);
    bool executeScript(std::size_t index);
    bool forceStopScript(std::size_t index);
    void clear();
    std::size_t count() const;
    RecordPtr get(std::size_t index);
    ConstRecordPtr get(std::size_t index) const;
    std::vector<RecordPtr> snapshot() const;
    nlohmann::json serialize() const;
    void deserialize(const nlohmann::json& value);
    void shutdown();

private:
    bool loadRecord(ImportedScriptRecord& record);
    bool isDuplicatePath(const std::filesystem::path& path) const;
    static std::filesystem::path NormalizePath(const std::filesystem::path& path);

    Container scripts_;
    mutable std::mutex scriptsMutex_;
};

} // namespace smu::app

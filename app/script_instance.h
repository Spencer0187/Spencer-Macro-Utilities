#pragma once

#include "../platform/platform_types.h"

#include <array>
#include <chrono>
#include <filesystem>
#include <cstddef>
#include <string>
#include <unordered_map>
#include <vector>

extern "C" {
#include "lauxlib.h"
#include "lua.h"
}

namespace smu::app {

struct ImportedScriptRecord;

inline constexpr std::size_t kDefaultScriptMemoryLimitMB = 64;
inline constexpr std::size_t kMinScriptMemoryLimitMB = 16;
inline constexpr std::size_t kMaxScriptMemoryLimitMB = 256;
inline constexpr std::size_t kMaxUiIdBytes = 128;
inline constexpr std::size_t kMaxUiControlsPerSettingsCall = 512;
inline constexpr std::size_t kMaxUiStateStringBytes = 4096;

class ScriptInstance {
public:
    using UiStringBuffer = std::array<char, kMaxUiStateStringBytes + 1>;

    explicit ScriptInstance(ImportedScriptRecord& owner);
    ~ScriptInstance();

    ScriptInstance(const ScriptInstance&) = delete;
    ScriptInstance& operator=(const ScriptInstance&) = delete;

    bool init();
    bool loadFile(const std::filesystem::path& path);
    bool hasFunction(const char* name) const;
    bool callOnExecute();
    bool callOnSettings(bool renderMode);
    void syncSettingsTable();
    void cleanup();
    void* reallocateLuaMemory(void* ptr, std::size_t oldSize, std::size_t newSize);

    ImportedScriptRecord& owner() { return *owner_; }
    const ImportedScriptRecord& owner() const { return *owner_; }
    void setOwner(ImportedScriptRecord& owner) { owner_ = &owner; }
    lua_State* luaState() const { return L_; }

    bool checkDeadline() const;
    void sleepWithDeadline(int ms);
    void scheduleCoroutineSleep(lua_State* thread, int ms);
    void setFreeze(bool enabled);
    void setLagSwitch(bool enabled);
    void setSettingsRenderMode(bool enabled) { settingsRenderMode_ = enabled; }
    bool isSettingsRenderMode() const { return settingsRenderMode_; }
    void resetSettingsUiControlCount() { settingsUiControlCount_ = 0; }
    bool tryConsumeSettingsUiControl();
    UiStringBuffer& textboxBuffer(const std::string& id);
    UiStringBuffer& dynamicTextboxBuffer(const std::string& id);
    unsigned int& keybindValue(const std::string& id, unsigned int defaultValue);
    bool tryGetTransientUiValue(const std::string& key, std::string& out) const;
    void setTransientUiValue(const std::string& key, std::string value);

private:
    bool callProtected(int argCount, const char* context);
    void beginTimedCall(std::chrono::steady_clock::duration maxRuntime);
    void beginTimedCall();
    void endTimedCall();
    bool drainSleepingCoroutines();
    void configureMemoryLimit();

    lua_State* L_ = nullptr;
    ImportedScriptRecord* owner_ = nullptr;
    std::chrono::steady_clock::time_point deadline_{};
    struct SleepingCoroutine {
        lua_State* thread = nullptr;
        int registryRef = LUA_NOREF;
        std::chrono::steady_clock::time_point wakeTime{};
    };
    void releaseSleepingCoroutine(SleepingCoroutine& coroutine);
    void releaseAllSleepingCoroutines();
    std::vector<SleepingCoroutine> sleepingCoroutines_;
    std::vector<smu::platform::PlatformPid> frozenPids_;
    std::unordered_map<std::string, std::string> transientUi_;
    std::unordered_map<std::string, UiStringBuffer> textboxBuffers_;
    std::unordered_map<std::string, UiStringBuffer> dynamicTextboxBuffers_;
    std::unordered_map<std::string, unsigned int> keybindValues_;
    std::size_t settingsUiControlCount_ = 0;
    std::size_t memoryLimitBytes_ = kDefaultScriptMemoryLimitMB * 1024u * 1024u;
    std::size_t memoryUsedBytes_ = 0;
    bool settingsRenderMode_ = false;
};

ScriptInstance* GetScriptInstance(lua_State* L);

} // namespace smu::app

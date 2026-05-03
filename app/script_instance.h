#pragma once

#include "../platform/platform_types.h"

#include <chrono>
#include <filesystem>
#include <string>
#include <vector>

extern "C" {
#include "lua.h"
}

namespace smu::app {

struct ImportedScriptRecord;

class ScriptInstance {
public:
    explicit ScriptInstance(ImportedScriptRecord& owner);
    ~ScriptInstance();

    ScriptInstance(const ScriptInstance&) = delete;
    ScriptInstance& operator=(const ScriptInstance&) = delete;

    bool init();
    bool loadFile(const std::filesystem::path& path);
    bool hasFunction(const char* name) const;
    bool callOnExecute();
    void cleanup();

    ImportedScriptRecord& owner() { return *owner_; }
    const ImportedScriptRecord& owner() const { return *owner_; }
    void setOwner(ImportedScriptRecord& owner) { owner_ = &owner; }
    lua_State* luaState() const { return L_; }

    bool checkDeadline() const;
    void sleepWithDeadline(int ms);
    void scheduleCoroutineSleep(lua_State* thread, int ms);
    void setFreeze(bool enabled);
    void setLagSwitch(bool enabled);

private:
    bool callProtected(int argCount, const char* context);
    void beginTimedCall();
    void endTimedCall();
    bool drainSleepingCoroutines();

    lua_State* L_ = nullptr;
    ImportedScriptRecord* owner_ = nullptr;
    std::chrono::steady_clock::time_point deadline_{};
    struct SleepingCoroutine {
        lua_State* thread = nullptr;
        std::chrono::steady_clock::time_point wakeTime{};
    };
    std::vector<SleepingCoroutine> sleepingCoroutines_;
    std::vector<smu::platform::PlatformPid> frozenPids_;
};

ScriptInstance* GetScriptInstance(lua_State* L);

} // namespace smu::app

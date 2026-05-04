#include "script_instance.h"

#include "script_api.h"
#include "profile_manager.h"
#include "script_manager.h"
#include "../core/legacy_globals.h"
#include "../platform/logging.h"
#include "../platform/network_backend.h"
#include "../platform/process_backend.h"

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <optional>
#include <type_traits>
#include <thread>
#include <variant>

extern "C" {
#include "lauxlib.h"
#include "lualib.h"
}

namespace smu::app {
namespace {

constexpr const char* kRegistryInstanceKey = "SMU.ScriptInstance";
constexpr auto kMaxScriptRuntime = std::chrono::seconds(30);

void OpenRestrictedLuaLibraries(lua_State* L)
{
    luaL_requiref(L, "_G", luaopen_base, 1);
    lua_pop(L, 1);
    lua_pushnil(L);
    lua_setglobal(L, "dofile");
    lua_pushnil(L);
    lua_setglobal(L, "loadfile");
    luaL_requiref(L, LUA_TABLIBNAME, luaopen_table, 1);
    lua_pop(L, 1);
    luaL_requiref(L, LUA_STRLIBNAME, luaopen_string, 1);
    lua_pop(L, 1);
    luaL_requiref(L, LUA_MATHLIBNAME, luaopen_math, 1);
    lua_pop(L, 1);
    luaL_requiref(L, LUA_UTF8LIBNAME, luaopen_utf8, 1);
    lua_pop(L, 1);
    luaL_requiref(L, LUA_COLIBNAME, luaopen_coroutine, 1);
    lua_pop(L, 1);
}

void PushLuaSettingValue(lua_State* L, const SavedSettingValue& value)
{
    std::visit([L](const auto& storedValue) {
        using ValueType = std::decay_t<decltype(storedValue)>;
        if constexpr (std::is_same_v<ValueType, bool>) {
            lua_pushboolean(L, storedValue);
        } else if constexpr (std::is_same_v<ValueType, std::int64_t>) {
            lua_pushinteger(L, static_cast<lua_Integer>(storedValue));
        } else if constexpr (std::is_same_v<ValueType, double>) {
            lua_pushnumber(L, static_cast<lua_Number>(storedValue));
        } else {
            lua_pushlstring(L, storedValue.c_str(), storedValue.size());
        }
    }, value);
}

std::optional<SavedSettingValue> JsonToSavedValue(const nlohmann::json& value)
{
    if (value.is_boolean()) {
        return SavedSettingValue{value.get<bool>()};
    }
    if (value.is_number_integer()) {
        return SavedSettingValue{static_cast<std::int64_t>(value.get<std::int64_t>())};
    }
    if (value.is_number_unsigned()) {
        return SavedSettingValue{static_cast<std::int64_t>(value.get<std::uint64_t>())};
    }
    if (value.is_number_float()) {
        return SavedSettingValue{value.get<double>()};
    }
    if (value.is_string()) {
        return SavedSettingValue{value.get<std::string>()};
    }
    return std::nullopt;
}

void SyncLuaSettingsTable(lua_State* L, const nlohmann::json& state)
{
    lua_newtable(L);
    if (state.is_object()) {
        for (const auto& [key, value] : state.items()) {
            if (auto stored = JsonToSavedValue(value)) {
                PushLuaSettingValue(L, *stored);
                lua_setfield(L, -2, key.c_str());
            }
        }
    }
    lua_setglobal(L, "settings");
}

bool HasSettingsCallback(lua_State* L)
{
    lua_getglobal(L, "onSettings");
    const bool exists = lua_isfunction(L, -1);
    lua_pop(L, 1);
    return exists;
}

void TimeoutHook(lua_State* L, lua_Debug*)
{
    ScriptInstance* instance = GetScriptInstance(L);
    if (instance && !instance->checkDeadline()) {
        luaL_error(L, "script execution timed out");
    }
}

std::vector<smu::platform::PlatformPid> CurrentTargetPids()
{
    auto backend = smu::platform::GetProcessBackend();
    if (!backend) {
        return {};
    }

    if (Globals::takeallprocessids) {
        return backend->findAllProcesses(Globals::settingsBuffer);
    }

    if (auto pid = backend->findMainProcess(Globals::settingsBuffer)) {
        return {*pid};
    }

    return {};
}

} // namespace

ScriptInstance::ScriptInstance(ImportedScriptRecord& owner)
    : owner_(&owner)
{
}

ScriptInstance::~ScriptInstance()
{
    cleanup();
}

bool ScriptInstance::init()
{
    cleanup();
    L_ = luaL_newstate();
    if (!L_) {
        owner_->lastError = "Could not create Lua state.";
        return false;
    }

    lua_pushlightuserdata(L_, this);
    lua_setfield(L_, LUA_REGISTRYINDEX, kRegistryInstanceKey);

    OpenRestrictedLuaLibraries(L_);
    RegisterScriptApi(L_);
    SyncLuaSettingsTable(L_, owner_->uiState);
    return true;
}

bool ScriptInstance::loadFile(const std::filesystem::path& path)
{
    if (!L_ && !init()) {
        return false;
    }

    const std::string pathString = path.string();
    if (luaL_loadfile(L_, pathString.c_str()) != LUA_OK) {
        const char* message = lua_tostring(L_, -1);
        owner_->lastError = message ? message : "Could not load script file.";
        lua_pop(L_, 1);
        return false;
    }

    const bool loaded = callProtected(0, "load script");
    if (loaded && HasSettingsCallback(L_)) {
        callOnSettings(false);
    }
    return loaded;
}

bool ScriptInstance::hasFunction(const char* name) const
{
    if (!L_) {
        return false;
    }

    lua_getglobal(L_, name);
    const bool exists = lua_isfunction(L_, -1);
    lua_pop(L_, 1);
    return exists;
}

bool ScriptInstance::callOnExecute()
{
    if (!L_) {
        owner_->lastError = "Script is not loaded.";
        return false;
    }

    lua_getglobal(L_, "onExecute");
    if (!lua_isfunction(L_, -1)) {
        lua_pop(L_, 1);
        owner_->lastError = "Script does not define onExecute().";
        return false;
    }

    beginTimedCall();
    const int status = lua_pcall(L_, 0, 0, 0);
    if (status != LUA_OK) {
        endTimedCall();
        const char* message = lua_tostring(L_, -1);
        owner_->lastError = message ? message : "Lua call failed.";
        lua_pop(L_, 1);
        LogWarning(std::string("Imported script failed during run onExecute: ") + owner_->lastError);
        return false;
    }

    if (!drainSleepingCoroutines()) {
        endTimedCall();
        return false;
    }

    endTimedCall();
    owner_->lastError.clear();
    return true;
}

void ScriptInstance::syncSettingsTable()
{
    if (L_) {
        SyncLuaSettingsTable(L_, owner_->uiState);
    }
}

bool ScriptInstance::tryGetTransientUiValue(const std::string& key, std::string& out) const
{
    const auto it = transientUi_.find(key);
    if (it == transientUi_.end()) {
        return false;
    }
    out = it->second;
    return true;
}

void ScriptInstance::setTransientUiValue(const std::string& key, std::string value)
{
    transientUi_[key] = std::move(value);
}

bool ScriptInstance::callOnSettings(bool renderMode)
{
    if (!L_) {
        owner_->lastError = "Script is not loaded.";
        return false;
    }

    lua_getglobal(L_, "onSettings");
    if (!lua_isfunction(L_, -1)) {
        lua_pop(L_, 1);
        return true;
    }

    setSettingsRenderMode(renderMode);
    const int status = lua_pcall(L_, 0, 0, 0);
    setSettingsRenderMode(false);
    if (status != LUA_OK) {
        const char* message = lua_tostring(L_, -1);
        owner_->lastError = message ? message : "Lua settings callback failed.";
        lua_pop(L_, 1);
        LogWarning(std::string("Imported script failed during onSettings: ") + owner_->lastError);
        return false;
    }

    return true;
}

void ScriptInstance::cleanup()
{
    setFreeze(false);
    if (L_) {
        lua_close(L_);
        L_ = nullptr;
    }
}

bool ScriptInstance::checkDeadline() const
{
    return deadline_ == std::chrono::steady_clock::time_point{} ||
        std::chrono::steady_clock::now() <= deadline_;
}

void ScriptInstance::sleepWithDeadline(int ms)
{
    int remaining = std::max(0, ms);
    deadline_ += std::chrono::milliseconds(remaining);
    while (remaining > 0) {
        if (!checkDeadline()) {
            luaL_error(L_, "script execution timed out");
        }
        const int chunk = std::min(remaining, 25);
        std::this_thread::sleep_for(std::chrono::milliseconds(chunk));
        remaining -= chunk;
    }
}

void ScriptInstance::scheduleCoroutineSleep(lua_State* thread, int ms)
{
    const auto wakeTime = std::chrono::steady_clock::now() + std::chrono::milliseconds(std::max(0, ms));
    for (auto& coroutine : sleepingCoroutines_) {
        if (coroutine.thread == thread) {
            coroutine.wakeTime = wakeTime;
            return;
        }
    }

    sleepingCoroutines_.push_back(SleepingCoroutine{thread, wakeTime});
}

bool ScriptInstance::drainSleepingCoroutines()
{
    while (!sleepingCoroutines_.empty()) {
        const auto now = std::chrono::steady_clock::now();
        auto nextWake = sleepingCoroutines_.front().wakeTime;
        for (const auto& coroutine : sleepingCoroutines_) {
            nextWake = std::min(nextWake, coroutine.wakeTime);
        }

        const auto deadline = deadline_;
        const auto sleepUntil = deadline == std::chrono::steady_clock::time_point{}
            ? nextWake
            : std::min(nextWake, deadline);
        if (sleepUntil > now) {
            std::this_thread::sleep_until(sleepUntil);
        }

        if (!checkDeadline()) {
            owner_->lastError = "script execution timed out";
            LogWarning("Imported script failed during coroutine sleep resume: " + owner_->lastError);
            return false;
        }

        const auto resumeTime = std::chrono::steady_clock::now();
        for (auto it = sleepingCoroutines_.begin(); it != sleepingCoroutines_.end();) {
            if (it->wakeTime > resumeTime) {
                ++it;
                continue;
            }

            lua_State* thread = it->thread;
            if (!thread) {
                it = sleepingCoroutines_.erase(it);
                continue;
            }

            if (lua_status(thread) != LUA_YIELD) {
                it = sleepingCoroutines_.erase(it);
                continue;
            }

            lua_sethook(thread, TimeoutHook, LUA_MASKCOUNT, 10000);
            int resultCount = 0;
            const int status = lua_resume(thread, L_, 0, &resultCount);
            lua_sethook(thread, nullptr, 0, 0);

            if (status == LUA_OK) {
                it = sleepingCoroutines_.erase(it);
                continue;
            }

            if (status == LUA_YIELD) {
                ++it;
                continue;
            }

            const char* message = lua_tostring(thread, -1);
            owner_->lastError = message ? message : "Lua coroutine failed.";
            lua_pop(thread, 1);
            LogWarning(std::string("Imported script failed during coroutine sleep resume: ") + owner_->lastError);
            sleepingCoroutines_.clear();
            return false;
        }
    }

    return true;
}

void ScriptInstance::setFreeze(bool enabled)
{
    auto backend = smu::platform::GetProcessBackend();
    if (!backend) {
        return;
    }

    if (enabled) {
        if (frozenPids_.empty()) {
            frozenPids_ = CurrentTargetPids();
        }
        for (smu::platform::PlatformPid pid : frozenPids_) {
            backend->suspend(pid);
        }
    } else {
        for (smu::platform::PlatformPid pid : frozenPids_) {
            backend->resume(pid);
        }
        frozenPids_.clear();
    }
}

void ScriptInstance::setLagSwitch(bool enabled)
{
    auto backend = smu::platform::GetNetworkLagBackend();
    if (!backend) {
        return;
    }

    smu::platform::LagSwitchConfig config = backend->config();
    config.enabled = Globals::bWinDivertEnabled || enabled;
    config.inboundHardBlock = Globals::lagswitchinbound;
    config.outboundHardBlock = Globals::lagswitchoutbound;
    config.fakeLagEnabled = Globals::lagswitchlag;
    config.inboundFakeLag = Globals::lagswitchlaginbound;
    config.outboundFakeLag = Globals::lagswitchlagoutbound;
    config.fakeLagDelayMs = Globals::lagswitchlagdelay;
    config.targetRobloxOnly = Globals::lagswitchtargetroblox;
    config.useTcp = Globals::lagswitchusetcp;
    config.useUdp = !Globals::lagswitchusetcp;
    config.preventDisconnect = Globals::prevent_disconnect;
    config.autoUnblock = Globals::lagswitch_autounblock;
    config.maxDurationSeconds = Globals::lagswitch_max_duration;
    config.unblockDurationMs = Globals::lagswitch_unblock_ms;
    backend->setConfig(config);

    if (enabled && !Globals::bWinDivertEnabled) {
        std::string error;
        if (backend->init(&error)) {
            Globals::bWinDivertEnabled = true;
        } else if (!error.empty()) {
            owner_->lastError = error;
            LogWarning(error);
            return;
        }
    }

    backend->setBlockingActive(enabled);
    Globals::g_windivert_blocking.store(enabled, std::memory_order_relaxed);
}

bool ScriptInstance::callProtected(int argCount, const char* context)
{
    beginTimedCall();
    const int status = lua_pcall(L_, argCount, 0, 0);
    endTimedCall();

    if (status != LUA_OK) {
        const char* message = lua_tostring(L_, -1);
        owner_->lastError = message ? message : "Lua call failed.";
        lua_pop(L_, 1);
        LogWarning(std::string("Imported script failed during ") + context + ": " + owner_->lastError);
        return false;
    }

    owner_->lastError.clear();
    return true;
}

void ScriptInstance::beginTimedCall()
{
    deadline_ = std::chrono::steady_clock::now() + kMaxScriptRuntime;
    lua_sethook(L_, TimeoutHook, LUA_MASKCOUNT, 10000);
}

void ScriptInstance::endTimedCall()
{
    lua_sethook(L_, nullptr, 0, 0);
    deadline_ = {};
}

ScriptInstance* GetScriptInstance(lua_State* L)
{
    lua_getfield(L, LUA_REGISTRYINDEX, kRegistryInstanceKey);
    auto* instance = static_cast<ScriptInstance*>(lua_touserdata(L, -1));
    lua_pop(L, 1);
    return instance;
}

} // namespace smu::app

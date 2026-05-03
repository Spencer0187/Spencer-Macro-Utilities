#include "script_instance.h"

#include "script_api.h"
#include "script_manager.h"
#include "../core/legacy_globals.h"
#include "../platform/logging.h"
#include "../platform/network_backend.h"
#include "../platform/process_backend.h"

#include <algorithm>
#include <chrono>
#include <thread>

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

    return callProtected(0, "load script");
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

    return callProtected(0, "run onExecute");
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

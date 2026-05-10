#include "script_instance.h"

#include "app_ui_controls.h"
#include "script_api.h"
#include "profile_manager.h"
#include "script_manager.h"
#include "../core/legacy_globals.h"
#include "../platform/logging.h"
#include "../platform/network_backend.h"
#include "../platform/process_backend.h"

#if defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include "../platform/windows/admin_elevation.h"
#endif

#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstdint>
#include <cstring>
#include <cmath>
#include <limits>
#include <optional>
#include <type_traits>
#include <thread>
#include <utility>
#include <variant>

#include "imgui.h"

extern "C" {
#include "lauxlib.h"
#include "lualib.h"
}

namespace smu::app {
namespace {

constexpr auto kPreciseSleepSpinThreshold = std::chrono::microseconds(800);

void CpuRelax()
{
#if defined(_WIN32)
    YieldProcessor();
#elif defined(__x86_64__) || defined(__i386__)
    __builtin_ia32_pause();
#else
    std::atomic_signal_fence(std::memory_order_seq_cst);
#endif
}

#if defined(_WIN32)
#ifndef CREATE_WAITABLE_TIMER_HIGH_RESOLUTION
constexpr DWORD kCreateWaitableTimerHighResolution = 0x00000002;
#else
constexpr DWORD kCreateWaitableTimerHighResolution = CREATE_WAITABLE_TIMER_HIGH_RESOLUTION;
#endif

HANDLE CreateScriptSleepTimer()
{
    HANDLE timer = CreateWaitableTimerExW(nullptr, nullptr, kCreateWaitableTimerHighResolution, TIMER_MODIFY_STATE | SYNCHRONIZE);
    if (!timer) {
        timer = CreateWaitableTimerW(nullptr, FALSE, nullptr);
    }
    return timer;
}

bool IsCancelEventSet(HANDLE cancelEvent)
{
    return cancelEvent && WaitForSingleObject(cancelEvent, 0) == WAIT_OBJECT_0;
}

bool SpinUntilWithWindowsCancel(std::chrono::steady_clock::time_point wakeTime, HANDLE cancelEvent)
{
    unsigned int pollCounter = 0;
    while (std::chrono::steady_clock::now() < wakeTime) {
        if ((++pollCounter & 0x3fu) == 0u && IsCancelEventSet(cancelEvent)) {
            return false;
        }
        CpuRelax();
    }
    return !IsCancelEventSet(cancelEvent);
}

bool WaitUntilWithWindowsTimer(std::chrono::steady_clock::time_point wakeTime, HANDLE cancelEvent, bool precise)
{
    if (IsCancelEventSet(cancelEvent)) {
        return false;
    }

    if (wakeTime == std::chrono::steady_clock::time_point::max()) {
        if (!cancelEvent) {
            Sleep(INFINITE);
            return true;
        }
        WaitForSingleObject(cancelEvent, INFINITE);
        return false;
    }

    HANDLE timer = CreateScriptSleepTimer();
    if (!timer) {
        return precise ? SpinUntilWithWindowsCancel(wakeTime, cancelEvent) : false;
    }

    constexpr LONGLONG kHundredNanosecondsPerSecond = 10000000LL;
    constexpr LONGLONG kMaxSingleTimerChunk = 60LL * kHundredNanosecondsPerSecond;

    bool reachedWakeTime = false;
    while (true) {
        if (IsCancelEventSet(cancelEvent)) {
            break;
        }

        const auto now = std::chrono::steady_clock::now();
        if (now >= wakeTime) {
            reachedWakeTime = true;
            break;
        }

        auto remaining = wakeTime - now;
        if (precise && remaining <= kPreciseSleepSpinThreshold) {
            reachedWakeTime = SpinUntilWithWindowsCancel(wakeTime, cancelEvent);
            break;
        }

        if (precise && remaining > kPreciseSleepSpinThreshold) {
            remaining -= kPreciseSleepSpinThreshold;
        }

        const auto remainingHundredNs = std::chrono::duration_cast<std::chrono::nanoseconds>(remaining).count() / 100;
        const LONGLONG chunkHundredNs = std::clamp<LONGLONG>(remainingHundredNs, 1, kMaxSingleTimerChunk);

        LARGE_INTEGER dueTime = {};
        dueTime.QuadPart = -chunkHundredNs;
        if (!SetWaitableTimer(timer, &dueTime, 0, nullptr, nullptr, FALSE)) {
            break;
        }

        HANDLE handles[2] = {timer, cancelEvent};
        const DWORD handleCount = cancelEvent ? 2u : 1u;
        const DWORD waitResult = WaitForMultipleObjects(handleCount, handles, FALSE, INFINITE);
        if (waitResult == WAIT_OBJECT_0) {
            continue;
        }
        if (waitResult == WAIT_OBJECT_0 + 1) {
            break;
        }
        break;
    }

    CloseHandle(timer);
    return reachedWakeTime;
}
#endif

constexpr const char* kRegistryInstanceKey = "SMU.ScriptInstance";
constexpr const char* kLagSwitchRequiresAdminWarning =
    "Lag switch was requested, but SMU is not running as Administrator. The script continued, but lag-switch actions were skipped.";
constexpr auto kMaxScriptRuntime = std::chrono::seconds(30);
constexpr auto kMaxSettingsRuntime = std::chrono::seconds(5);
constexpr int kLuaHookInstructionCount = 10000;
// Allow effectively infinite sleeps; wake times are clamped to time_point::max().
constexpr std::int64_t kMaxSingleSleepMs = std::numeric_limits<std::int64_t>::max();
void TimeoutHook(lua_State* L, lua_Debug*);

void* LuaAllocator(void* userdata, void* ptr, std::size_t osize, std::size_t nsize)
{
    auto* instance = static_cast<ScriptInstance*>(userdata);
    if (!instance) {
        return nullptr;
    }
    return instance->reallocateLuaMemory(ptr, osize, nsize);
}

void InstallTimeoutHook(lua_State* L)
{
    lua_sethook(L, TimeoutHook, LUA_MASKCOUNT, kLuaHookInstructionCount);
}

void RestoreHook(lua_State* L, lua_Hook hook, int mask, int count)
{
    lua_sethook(L, hook, mask, count);
}

int SafeAuxResume(lua_State* L, lua_State* co, int narg)
{
    int status = LUA_OK;
    int resultCount = 0;

    if (!lua_checkstack(co, narg)) {
        lua_pushliteral(L, "too many arguments to resume");
        return -1;
    }

    lua_xmove(L, co, narg);
    const lua_Hook previousHook = lua_gethook(co);
    const int previousMask = lua_gethookmask(co);
    const int previousCount = lua_gethookcount(co);
    InstallTimeoutHook(co);
    status = lua_resume(co, L, narg, &resultCount);
    RestoreHook(co, previousHook, previousMask, previousCount);

    if (status == LUA_OK || status == LUA_YIELD) {
        if (!lua_checkstack(L, resultCount + 1)) {
            lua_pop(co, resultCount);
            lua_pushliteral(L, "too many results to resume");
            return -1;
        }
        lua_xmove(co, L, resultCount);
        return resultCount;
    }

    lua_xmove(co, L, 1);
    return -1;
}

int LuaSafeCoroutineResume(lua_State* L)
{
    lua_State* co = lua_tothread(L, 1);
    luaL_argexpected(L, co, 1, "thread");

    const int resultCount = SafeAuxResume(L, co, lua_gettop(L) - 1);
    if (ScriptInstance* instance = GetScriptInstance(L)) {
        instance->throwStopIfRequested(L);
    }
    if (resultCount < 0) {
        lua_pushboolean(L, 0);
        lua_insert(L, -2);
        return 2;
    }

    lua_pushboolean(L, 1);
    lua_insert(L, -(resultCount + 1));
    return resultCount + 1;
}

int LuaSafeCoroutineWrapClosure(lua_State* L)
{
    lua_State* co = lua_tothread(L, lua_upvalueindex(1));
    const int resultCount = SafeAuxResume(L, co, lua_gettop(L));
    if (ScriptInstance* instance = GetScriptInstance(L)) {
        instance->throwStopIfRequested(L);
    }
    if (resultCount < 0) {
        return lua_error(L);
    }
    return resultCount;
}

int LuaSafeCoroutineWrap(lua_State* L)
{
    luaL_checktype(L, 1, LUA_TFUNCTION);
    lua_State* co = lua_newthread(L);
    lua_pushvalue(L, 1);
    lua_xmove(L, co, 1);
    lua_pushcclosure(L, LuaSafeCoroutineWrapClosure, 1);
    return 1;
}

void InstallSafeCoroutineWrappers(lua_State* L)
{
    lua_getglobal(L, "coroutine");
    if (!lua_istable(L, -1)) {
        lua_pop(L, 1);
        return;
    }

    lua_pushcfunction(L, LuaSafeCoroutineResume);
    lua_setfield(L, -2, "resume");
    lua_pushcfunction(L, LuaSafeCoroutineWrap);
    lua_setfield(L, -2, "wrap");
    lua_pop(L, 1);
}

int LuaSafePCallContinuation(lua_State* L, int, lua_KContext)
{
    if (ScriptInstance* instance = GetScriptInstance(L)) {
        instance->throwStopIfRequested(L);
    }
    return lua_gettop(L);
}

int LuaSafePCall(lua_State* L)
{
    const int argc = lua_gettop(L);
    lua_pushvalue(L, lua_upvalueindex(1));
    lua_insert(L, 1);
    lua_callk(L, argc, LUA_MULTRET, 0, LuaSafePCallContinuation);
    return LuaSafePCallContinuation(L, LUA_OK, 0);
}

int LuaSafeXpcallContinuation(lua_State* L, int, lua_KContext)
{
    if (ScriptInstance* instance = GetScriptInstance(L)) {
        instance->throwStopIfRequested(L);
    }
    return lua_gettop(L);
}

int LuaSafeXpcall(lua_State* L)
{
    const int argc = lua_gettop(L);
    lua_pushvalue(L, lua_upvalueindex(1));
    lua_insert(L, 1);
    lua_callk(L, argc, LUA_MULTRET, 0, LuaSafeXpcallContinuation);
    return LuaSafeXpcallContinuation(L, LUA_OK, 0);
}

void InstallSafeCallWrappers(lua_State* L)
{
    lua_getglobal(L, "pcall");
    if (lua_isfunction(L, -1)) {
        lua_pushcclosure(L, LuaSafePCall, 1);
        lua_setglobal(L, "pcall");
    } else {
        lua_pop(L, 1);
    }

    lua_getglobal(L, "xpcall");
    if (lua_isfunction(L, -1)) {
        lua_pushcclosure(L, LuaSafeXpcall, 1);
        lua_setglobal(L, "xpcall");
    } else {
        lua_pop(L, 1);
    }
}

void OpenRestrictedLuaLibraries(lua_State* L)
{
    luaL_requiref(L, "_G", luaopen_base, 1);
    lua_pop(L, 1);
    lua_pushnil(L);
    lua_setglobal(L, "dofile");
    lua_pushnil(L);
    lua_setglobal(L, "loadfile");
    lua_pushnil(L);
    lua_setglobal(L, "load");
    lua_pushnil(L);
    lua_setglobal(L, "collectgarbage");
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
    InstallSafeCoroutineWrappers(L);
    InstallSafeCallWrappers(L);
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

template <std::size_t N>
void CopyToBuffer(std::array<char, N>& buffer, const std::string& text)
{
    std::snprintf(buffer.data(), buffer.size(), "%s", text.c_str());
}

std::string ClampStoredUiText(std::string text)
{
    if (text.size() > kMaxUiStateStringBytes) {
        text.resize(kMaxUiStateStringBytes);
    }
    return text;
}

std::string ClampDynamicUiText(std::string text)
{
    if (text.size() > kMaxUiStateStringBytes) {
        text = text.substr(text.size() - kMaxUiStateStringBytes);
    }
    return text;
}

unsigned int NormalizeCachedHotkey(unsigned int hotkey)
{
    return IsScriptHotkeyBound(hotkey) ? hotkey : kScriptUnboundHotkey;
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
        const auto unsignedValue = value.get<std::uint64_t>();
        if (unsignedValue > static_cast<std::uint64_t>(std::numeric_limits<std::int64_t>::max())) {
            return std::nullopt;
        }
        return SavedSettingValue{static_cast<std::int64_t>(unsignedValue)};
    }
    if (value.is_number_float()) {
        const double number = value.get<double>();
        if (!std::isfinite(number)) {
            return std::nullopt;
        }
        return SavedSettingValue{number};
    }
    if (value.is_string()) {
        std::string text = value.get<std::string>();
        if (text.size() > kMaxUiStateStringBytes) {
            text.resize(kMaxUiStateStringBytes);
        }
        return SavedSettingValue{std::move(text)};
    }
    return std::nullopt;
}

bool IsSafeUiStateKey(const std::string& key)
{
    return !key.empty() &&
        key.size() <= kMaxUiIdBytes &&
        std::memchr(key.data(), '\0', key.size()) == nullptr;
}

std::optional<nlohmann::json> LuaSettingToJson(lua_State* L, int index)
{
    switch (lua_type(L, index)) {
    case LUA_TBOOLEAN:
        return nlohmann::json(lua_toboolean(L, index) != 0);
    case LUA_TNUMBER:
        if (lua_isinteger(L, index)) {
            return nlohmann::json(static_cast<std::int64_t>(lua_tointeger(L, index)));
        } else {
            const double value = static_cast<double>(lua_tonumber(L, index));
            if (std::isfinite(value)) {
                return nlohmann::json(value);
            }
        }
        break;
    case LUA_TSTRING: {
        std::size_t length = 0;
        const char* text = lua_tolstring(L, index, &length);
        return nlohmann::json(ClampStoredUiText(std::string(text ? text : "", length)));
    }
    default:
        break;
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

void SyncUiStateFromLuaSettings(lua_State* L, ImportedScriptRecord& owner)
{
    lua_getglobal(L, "settings");
    if (!lua_istable(L, -1)) {
        lua_pop(L, 1);
        return;
    }

    nlohmann::json updates = nlohmann::json::object();
    lua_pushnil(L);
    while (lua_next(L, -2) != 0) {
        std::size_t keyLength = 0;
        const char* keyText = lua_tolstring(L, -2, &keyLength);
        if (keyText) {
            std::string key(keyText, keyLength);
            if (IsSafeUiStateKey(key) && updates.size() < kMaxUiStateEntries) {
                if (auto value = LuaSettingToJson(L, -1)) {
                    updates[key] = std::move(*value);
                }
            }
        }
        lua_pop(L, 1);
    }
    lua_pop(L, 1);

    if (updates.empty()) {
        return;
    }

    std::lock_guard<std::mutex> uiLock(owner.uiStateMutex);
    if (!owner.uiState.is_object()) {
        owner.uiState = nlohmann::json::object();
    }
    for (auto& [key, value] : updates.items()) {
        owner.uiState[key] = std::move(value);
    }
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
    if (ScriptInstance* instance = GetScriptInstance(L)) {
        instance->throwStopIfRequested(L);
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
#if defined(_WIN32)
    cancelEvent_ = CreateEventW(nullptr, TRUE, FALSE, nullptr);
#endif
}

ScriptInstance::~ScriptInstance()
{
    cleanup();
#if defined(_WIN32)
    if (cancelEvent_) {
        CloseHandle(static_cast<HANDLE>(cancelEvent_));
        cancelEvent_ = nullptr;
    }
#endif
}

void ScriptInstance::configureMemoryLimit()
{
    const std::size_t requestedMB = owner_ && owner_->metadata.memoryLimitMB
        ? *owner_->metadata.memoryLimitMB
        : kDefaultScriptMemoryLimitMB;
    const std::size_t clampedMB = std::clamp(requestedMB, kMinScriptMemoryLimitMB, kMaxScriptMemoryLimitMB);
    memoryLimitBytes_ = clampedMB * 1024u * 1024u;
}

void* ScriptInstance::reallocateLuaMemory(void* ptr, std::size_t oldSize, std::size_t newSize)
{
    if (newSize == 0) {
        if (ptr) {
            memoryUsedBytes_ -= std::min(memoryUsedBytes_, oldSize);
            std::free(ptr);
        }
        return nullptr;
    }

    const std::size_t trackedOldSize = ptr ? oldSize : 0;
    if (newSize > trackedOldSize) {
        const std::size_t delta = newSize - trackedOldSize;
        if (delta > memoryLimitBytes_ || memoryUsedBytes_ > memoryLimitBytes_ - delta) {
            return nullptr;
        }
    }

    void* newPtr = ptr ? std::realloc(ptr, newSize) : std::malloc(newSize);
    if (!newPtr) {
        return nullptr;
    }

    if (newSize > trackedOldSize) {
        memoryUsedBytes_ += newSize - trackedOldSize;
    } else {
        memoryUsedBytes_ -= std::min(memoryUsedBytes_, trackedOldSize - newSize);
    }
    return newPtr;
}

bool ScriptInstance::init()
{
    cleanup();
    configureMemoryLimit();
    memoryUsedBytes_ = 0;
    L_ = lua_newstate(LuaAllocator, this);
    if (!L_) {
        owner_->setLastError("Could not create Lua state.");
        return false;
    }

    lua_pushlightuserdata(L_, this);
    lua_setfield(L_, LUA_REGISTRYINDEX, kRegistryInstanceKey);

    OpenRestrictedLuaLibraries(L_);
    RegisterScriptApi(L_);
    nlohmann::json uiStateSnapshot = nlohmann::json::object();
    if (owner_) {
        std::lock_guard<std::mutex> uiLock(owner_->uiStateMutex);
        uiStateSnapshot = owner_->uiState;
    }
    SyncLuaSettingsTable(L_, uiStateSnapshot);
    syncUiIdCache();
    return true;
}

bool ScriptInstance::loadFile(const std::filesystem::path& path)
{
    if (!L_ && !init()) {
        return false;
    }

    const std::string pathString = path.string();
    if (luaL_loadfilex(L_, pathString.c_str(), "t") != LUA_OK) {
        const char* message = lua_tostring(L_, -1);
        std::string errorText = message ? message : "Could not load script file.";
        if (errorText.find("binary") != std::string::npos) {
            errorText = "Precompiled Lua chunks are not supported. Please provide a plain-text Lua script.";
        }
        owner_->setLastError(std::move(errorText));
        lua_pop(L_, 1);
        return false;
    }

    const bool loaded = callProtected(0, "load script");
    hasSettingsCallback_ = loaded && HasSettingsCallback(L_);
    if (hasSettingsCallback_) {
        callOnSettings(false);
    }
    return loaded;
}

bool ScriptInstance::hasFunction(const char* name) const
{
    std::lock_guard<std::mutex> luaLock(luaMutex_);
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
    std::lock_guard<std::mutex> luaLock(luaMutex_);
    if (!L_) {
        owner_->setLastError("Script is not loaded.");
        releaseLagSwitchControls();
        return false;
    }

    lua_getglobal(L_, "onExecute");
    if (!lua_isfunction(L_, -1)) {
        lua_pop(L_, 1);
        owner_->setLastError("Script does not define onExecute().");
        releaseLagSwitchControls();
        return false;
    }

    owner_->clearLastWarning();
    beginTimedCall(kMaxScriptRuntime);
    const int status = lua_pcall(L_, 0, 0, 0);
    if (status != LUA_OK) {
        endTimedCall();
        const StopReason stopReason = stopReason_.load(std::memory_order_acquire);
        const char* message = stopReason != StopReason::None
            ? stopReasonMessage()
            : lua_tostring(L_, -1);
        if (stopReason == StopReason::Cancelled) {
            owner_->clearLastError();
        } else {
            owner_->setLastError(message ? message : "Lua call failed.");
        }
        lua_pop(L_, 1);
        releaseAllSleepingCoroutines();
        releaseLagSwitchControls();
        if (stopReason != StopReason::Cancelled) {
            LogWarning(std::string("Imported script failed during run onExecute: ") + owner_->lastErrorCopy());
        }
        return false;
    }

    if (!drainSleepingCoroutines()) {
        endTimedCall();
        releaseLagSwitchControls();
        return false;
    }

    endTimedCall();
    const StopReason stopReason = stopReason_.load(std::memory_order_acquire);
    if (stopReason != StopReason::None) {
        if (stopReason == StopReason::Cancelled) {
            owner_->clearLastError();
        } else {
            owner_->setLastError(stopReasonMessage());
            LogWarning(std::string("Imported script failed during run onExecute: ") + owner_->lastErrorCopy());
        }
        releaseAllSleepingCoroutines();
        releaseLagSwitchControls();
        return false;
    }
    owner_->clearLastError();
    releaseLagSwitchControls();
    return true;
}

void ScriptInstance::syncSettingsTable()
{
    std::lock_guard<std::mutex> luaLock(luaMutex_);
    if (L_) {
        nlohmann::json uiStateSnapshot = nlohmann::json::object();
        if (owner_) {
            std::lock_guard<std::mutex> uiLock(owner_->uiStateMutex);
            uiStateSnapshot = owner_->uiState;
        }
        SyncLuaSettingsTable(L_, uiStateSnapshot);
        syncUiIdCache();
    }
}

void ScriptInstance::syncUiIdCache()
{
    uiIdCache_.clear();
    if (!owner_) {
        return;
    }
    std::lock_guard<std::mutex> uiLock(owner_->uiStateMutex);
    if (!owner_->uiState.is_object()) {
        return;
    }
    for (const auto& [key, value] : owner_->uiState.items()) {
        (void)value;
        if (uiIdCache_.size() >= kMaxUiStateEntries) {
            break;
        }
        uiIdCache_.insert(key);
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
    if (!tryRegisterUiId(key)) {
        return;
    }
    if (value.size() > kMaxUiStateStringBytes) {
        value.resize(kMaxUiStateStringBytes);
    }
    transientUi_[key] = std::move(value);
}

bool ScriptInstance::tryConsumeSettingsUiControl()
{
    if (settingsUiControlCount_ >= kMaxUiControlsPerSettingsCall) {
        return false;
    }
    ++settingsUiControlCount_;
    return true;
}

bool ScriptInstance::tryRegisterUiId(const std::string& id)
{
    if (uiIdCache_.find(id) != uiIdCache_.end()) {
        return true;
    }
    if (uiIdCache_.size() >= kMaxUiStateEntries) {
        return false;
    }
    uiIdCache_.insert(id);
    return true;
}

ScriptInstance::UiStringBuffer& ScriptInstance::textboxBuffer(const std::string& id)
{
    return textboxBuffers_[id];
}

ScriptInstance::UiStringBuffer& ScriptInstance::dynamicTextboxBuffer(const std::string& id)
{
    return dynamicTextboxBuffers_[id];
}

unsigned int& ScriptInstance::keybindValue(const std::string& id, unsigned int defaultValue)
{
    auto [it, inserted] = keybindValues_.try_emplace(id, defaultValue);
    if (inserted) {
        return it->second;
    }
    return it->second;
}

void ScriptInstance::beginSettingsUiCapture()
{
    std::lock_guard<std::mutex> lock(settingsUiMutex_);
    settingsUiCaptureActive_ = true;
    pendingSettingsUiControls_.clear();
}

void ScriptInstance::finishSettingsUiCapture(bool commit)
{
    std::lock_guard<std::mutex> lock(settingsUiMutex_);
    if (commit) {
        settingsUiControls_ = std::move(pendingSettingsUiControls_);
    } else {
        pendingSettingsUiControls_.clear();
    }
    pendingSettingsUiControls_.clear();
    settingsUiCaptureActive_ = false;
}

void ScriptInstance::recordSettingsText(std::string text, float width)
{
    std::lock_guard<std::mutex> lock(settingsUiMutex_);
    if (!settingsUiCaptureActive_) {
        return;
    }
    SettingsUiControl control;
    control.kind = SettingsUiControl::Kind::Text;
    control.text = std::move(text);
    control.width = width;
    pendingSettingsUiControls_.push_back(std::move(control));
}

void ScriptInstance::recordSettingsSeparator(float height)
{
    std::lock_guard<std::mutex> lock(settingsUiMutex_);
    if (!settingsUiCaptureActive_) {
        return;
    }
    SettingsUiControl control;
    control.kind = SettingsUiControl::Kind::Separator;
    control.height = height;
    pendingSettingsUiControls_.push_back(std::move(control));
}

void ScriptInstance::recordSettingsCheckbox(std::string id, std::string label, bool defaultValue, float width)
{
    std::lock_guard<std::mutex> lock(settingsUiMutex_);
    if (!settingsUiCaptureActive_) {
        return;
    }
    SettingsUiControl control;
    control.kind = SettingsUiControl::Kind::Checkbox;
    control.id = std::move(id);
    control.label = std::move(label);
    control.defaultBool = defaultValue;
    control.width = width;
    pendingSettingsUiControls_.push_back(std::move(control));
}

void ScriptInstance::recordSettingsSliderInt(std::string id, std::string label, int defaultValue, int minValue, int maxValue, float width)
{
    std::lock_guard<std::mutex> lock(settingsUiMutex_);
    if (!settingsUiCaptureActive_) {
        return;
    }
    SettingsUiControl control;
    control.kind = SettingsUiControl::Kind::SliderInt;
    control.id = std::move(id);
    control.label = std::move(label);
    control.defaultInt = defaultValue;
    control.minInt = minValue;
    control.maxInt = maxValue;
    control.width = width;
    pendingSettingsUiControls_.push_back(std::move(control));
}

void ScriptInstance::recordSettingsSliderFloat(std::string id, std::string label, double defaultValue, double minValue, double maxValue, float width)
{
    std::lock_guard<std::mutex> lock(settingsUiMutex_);
    if (!settingsUiCaptureActive_) {
        return;
    }
    SettingsUiControl control;
    control.kind = SettingsUiControl::Kind::SliderFloat;
    control.id = std::move(id);
    control.label = std::move(label);
    control.defaultFloat = defaultValue;
    control.minFloat = minValue;
    control.maxFloat = maxValue;
    control.width = width;
    pendingSettingsUiControls_.push_back(std::move(control));
}

void ScriptInstance::recordSettingsTextbox(std::string id, std::string label, std::string defaultValue, float width, float height)
{
    std::lock_guard<std::mutex> lock(settingsUiMutex_);
    if (!settingsUiCaptureActive_) {
        return;
    }
    SettingsUiControl control;
    control.kind = SettingsUiControl::Kind::Textbox;
    control.id = std::move(id);
    control.label = std::move(label);
    control.defaultText = std::move(defaultValue);
    control.width = width;
    control.height = height;
    pendingSettingsUiControls_.push_back(std::move(control));
}

void ScriptInstance::recordSettingsDynamicTextbox(std::string id, std::string label, std::string defaultValue, float width, float height)
{
    std::lock_guard<std::mutex> lock(settingsUiMutex_);
    if (!settingsUiCaptureActive_) {
        return;
    }
    SettingsUiControl control;
    control.kind = SettingsUiControl::Kind::DynamicTextbox;
    control.id = std::move(id);
    control.label = std::move(label);
    control.defaultText = std::move(defaultValue);
    control.width = width;
    control.height = height;
    pendingSettingsUiControls_.push_back(std::move(control));
}

void ScriptInstance::recordSettingsKeybind(std::string id, std::string label, unsigned int defaultValue, float width)
{
    std::lock_guard<std::mutex> lock(settingsUiMutex_);
    if (!settingsUiCaptureActive_) {
        return;
    }
    SettingsUiControl control;
    control.kind = SettingsUiControl::Kind::Keybind;
    control.id = std::move(id);
    control.label = std::move(label);
    control.defaultKeybind = defaultValue;
    control.width = width;
    pendingSettingsUiControls_.push_back(std::move(control));
}

void ScriptInstance::recordSettingsButton(std::string id, std::string label, float width, float height)
{
    std::lock_guard<std::mutex> lock(settingsUiMutex_);
    if (!settingsUiCaptureActive_) {
        return;
    }
    SettingsUiControl control;
    control.kind = SettingsUiControl::Kind::Button;
    control.id = std::move(id);
    control.label = std::move(label);
    control.width = width;
    control.height = height;
    pendingSettingsUiControls_.push_back(std::move(control));
}

void ScriptInstance::renderCachedSettings(bool readOnly)
{
    std::vector<SettingsUiControl> controls;
    {
        std::lock_guard<std::mutex> lock(settingsUiMutex_);
        controls = settingsUiControls_;
    }

    auto copyUiValue = [this](const std::string& id) -> nlohmann::json {
        if (!owner_) {
            return nullptr;
        }
        std::lock_guard<std::mutex> uiLock(owner_->uiStateMutex);
        const auto it = owner_->uiState.find(id);
        if (it == owner_->uiState.end()) {
            return nullptr;
        }
        return *it;
    };

    for (std::size_t index = 0; index < controls.size(); ++index) {
        const SettingsUiControl& control = controls[index];
        ImGui::PushID(static_cast<int>(index));
        switch (control.kind) {
        case SettingsUiControl::Kind::Text:
            if (control.width > 0.0f) {
                ImGui::PushTextWrapPos(ImGui::GetCursorPosX() + control.width);
                ImGui::TextWrapped("%s", control.text.c_str());
                ImGui::PopTextWrapPos();
            } else {
                ImGui::TextWrapped("%s", control.text.c_str());
            }
            break;
        case SettingsUiControl::Kind::Separator:
            ImGui::Separator();
            if (control.height > 0.0f) {
                ImGui::Dummy(ImVec2(0.0f, control.height));
            }
            break;
        case SettingsUiControl::Kind::Checkbox: {
            bool value = control.defaultBool;
            const nlohmann::json stored = copyUiValue(control.id);
            if (stored.is_boolean()) {
                value = stored.get<bool>();
            }
            if (readOnly) {
                ImGui::BeginDisabled();
            }
            if (control.width > 0.0f) {
                ImGui::Checkbox("##checkbox", &value);
                ImGui::SameLine();
                ImGui::PushTextWrapPos(ImGui::GetCursorPosX() + control.width);
                ImGui::TextWrapped("%s", control.label.c_str());
                ImGui::PopTextWrapPos();
            } else {
                ImGui::Checkbox(control.label.c_str(), &value);
            }
            if (readOnly) {
                ImGui::EndDisabled();
            }
            break;
        }
        case SettingsUiControl::Kind::SliderInt: {
            int value = control.defaultInt;
            const nlohmann::json stored = copyUiValue(control.id);
            if (stored.is_number_integer()) {
                const auto raw = stored.get<std::int64_t>();
                value = static_cast<int>(std::clamp<std::int64_t>(raw, std::numeric_limits<int>::min(), std::numeric_limits<int>::max()));
            } else if (stored.is_number_unsigned()) {
                const auto raw = stored.get<std::uint64_t>();
                value = raw > static_cast<std::uint64_t>(std::numeric_limits<int>::max())
                    ? std::numeric_limits<int>::max()
                    : static_cast<int>(raw);
            }
            if (readOnly) {
                ImGui::BeginDisabled();
            }
            if (control.width > 0.0f) {
                ImGui::SetNextItemWidth(control.width);
            }
            ImGui::SliderInt(control.label.c_str(), &value, control.minInt, control.maxInt);
            if (readOnly) {
                ImGui::EndDisabled();
            }
            break;
        }
        case SettingsUiControl::Kind::SliderFloat: {
            double storedValue = control.defaultFloat;
            const nlohmann::json stored = copyUiValue(control.id);
            if (stored.is_number()) {
                const double raw = stored.get<double>();
                if (std::isfinite(raw)) {
                    storedValue = raw;
                }
            }
            storedValue = std::clamp(storedValue,
                static_cast<double>(-std::numeric_limits<float>::max()),
                static_cast<double>(std::numeric_limits<float>::max()));
            float value = static_cast<float>(storedValue);
            if (readOnly) {
                ImGui::BeginDisabled();
            }
            if (control.width > 0.0f) {
                ImGui::SetNextItemWidth(control.width);
            }
            ImGui::SliderFloat(control.label.c_str(), &value,
                static_cast<float>(control.minFloat),
                static_cast<float>(control.maxFloat));
            if (readOnly) {
                ImGui::EndDisabled();
            }
            break;
        }
        case SettingsUiControl::Kind::Textbox: {
            std::string stored = ClampStoredUiText(control.defaultText);
            const nlohmann::json value = copyUiValue(control.id);
            if (value.is_string()) {
                stored = ClampStoredUiText(value.get<std::string>());
            }
            auto& buffer = textboxBuffer(control.id);
            CopyToBuffer(buffer, stored);
            if (readOnly) {
                ImGui::BeginDisabled();
            }
            if (control.height > 0.0f) {
                ImGui::InputTextMultiline(control.label.c_str(), buffer.data(), buffer.size(),
                    ImVec2(control.width > 0.0f ? control.width : -1.0f, control.height));
            } else {
                if (control.width > 0.0f) {
                    ImGui::SetNextItemWidth(control.width);
                }
                ImGui::InputText(control.label.c_str(), buffer.data(), buffer.size());
            }
            if (readOnly) {
                ImGui::EndDisabled();
            }
            break;
        }
        case SettingsUiControl::Kind::DynamicTextbox: {
            std::string stored = ClampDynamicUiText(control.defaultText);
            const nlohmann::json value = copyUiValue(control.id);
            if (value.is_string()) {
                stored = ClampDynamicUiText(value.get<std::string>());
            }
            auto& buffer = dynamicTextboxBuffer(control.id);
            CopyToBuffer(buffer, stored);
            ImGui::TextWrapped("%s", control.label.c_str());
            const float resolvedHeight = control.height > 0.0f ? control.height : ImGui::GetTextLineHeight() * 4.0f;
            ImGui::InputTextMultiline("##dynamic", buffer.data(), buffer.size(),
                ImVec2(control.width > 0.0f ? control.width : -1.0f, resolvedHeight),
                ImGuiInputTextFlags_ReadOnly | ImGuiInputTextFlags_NoHorizontalScroll);
            break;
        }
        case SettingsUiControl::Kind::Keybind: {
            unsigned int value = control.defaultKeybind;
            const nlohmann::json stored = copyUiValue(control.id);
            if (stored.is_number_integer()) {
                const auto raw = stored.get<std::int64_t>();
                if (raw >= 0 && raw <= static_cast<std::int64_t>(std::numeric_limits<int>::max())) {
                    value = NormalizeCachedHotkey(static_cast<unsigned int>(raw));
                } else {
                    value = kScriptUnboundHotkey;
                }
            } else if (stored.is_number_unsigned()) {
                const auto raw = stored.get<std::uint64_t>();
                if (raw <= static_cast<std::uint64_t>(std::numeric_limits<int>::max())) {
                    value = NormalizeCachedHotkey(static_cast<unsigned int>(raw));
                } else {
                    value = kScriptUnboundHotkey;
                }
            }
            if (readOnly) {
                ImGui::BeginDisabled();
            }
            if (control.width > 0.0f) {
                ImGui::PushTextWrapPos(ImGui::GetCursorPosX() + control.width);
                ImGui::TextWrapped("%s", control.label.c_str());
                ImGui::PopTextWrapPos();
            } else {
                ImGui::TextWrapped("%s", control.label.c_str());
            }
            const float humanWidth = control.width > 0.0f ? control.width * 0.55f : 170.0f;
            const float hexWidth = control.width > 0.0f ? control.width * 0.35f : 130.0f;
            DrawKeyBindControlShared("##ScriptKeybind", value, -1, humanWidth, hexWidth, true);
            if (readOnly) {
                ImGui::EndDisabled();
            }
            break;
        }
        case SettingsUiControl::Kind::Button: {
            if (readOnly) {
                ImGui::BeginDisabled();
            }
            ImGui::Button(control.label.c_str(), ImVec2(control.width, control.height));
            if (readOnly) {
                ImGui::EndDisabled();
            }
            break;
        }
        }
        ImGui::PopID();
    }
}

bool ScriptInstance::callOnSettings(bool renderMode)
{
    std::unique_lock<std::mutex> luaLock(luaMutex_, std::defer_lock);
    if (renderMode) {
        if (!luaLock.try_lock()) {
            return true;
        }
    } else {
        luaLock.lock();
    }

    if (!L_) {
        owner_->setLastError("Script is not loaded.");
        return false;
    }

    lua_getglobal(L_, "onSettings");
    if (!lua_isfunction(L_, -1)) {
        lua_pop(L_, 1);
        return true;
    }

    beginSettingsUiCapture();
    resetSettingsUiControlCount();
    setSettingsRenderMode(renderMode);
    setSettingsCallbackActive(true);
    beginTimedCall(kMaxSettingsRuntime);
    const int status = lua_pcall(L_, 0, 0, 0);
    endTimedCall();
    setSettingsCallbackActive(false);
    setSettingsRenderMode(false);
    const bool committed = status == LUA_OK && stopReason_.load(std::memory_order_acquire) == StopReason::None;
    finishSettingsUiCapture(committed);
    if (status != LUA_OK) {
        const char* message = stopReason_.load(std::memory_order_acquire) != StopReason::None
            ? stopReasonMessage()
            : lua_tostring(L_, -1);
        owner_->setLastError(message ? message : "Lua settings callback failed.");
        lua_pop(L_, 1);
        LogWarning(std::string("Imported script failed during onSettings: ") + owner_->lastErrorCopy());
        return false;
    }

    if (stopReason_.load(std::memory_order_acquire) != StopReason::None) {
        owner_->setLastError(stopReasonMessage());
        LogWarning(std::string("Imported script failed during onSettings: ") + owner_->lastErrorCopy());
        releaseAllSleepingCoroutines();
        return false;
    }

    if (owner_) {
        SyncUiStateFromLuaSettings(L_, *owner_);
    }

    return true;
}

void ScriptInstance::cleanup()
{
    requestCancel();
    std::lock_guard<std::mutex> luaLock(luaMutex_);
    setFreeze(false);
    releaseLagSwitchControls();
    if (L_) {
        releaseAllSleepingCoroutines();
        lua_close(L_);
        L_ = nullptr;
    }
    memoryUsedBytes_ = 0;
    hasSettingsCallback_ = false;
    settingsUiCaptureActive_ = false;
    settingsRenderMode_ = false;
    settingsCallbackActive_ = false;
    mouseMotionMode_ = MouseMotionMode::Raw;
    settingsUiControlCount_ = 0;
    transientUi_.clear();
    textboxBuffers_.clear();
    dynamicTextboxBuffers_.clear();
    keybindValues_.clear();
    {
        std::lock_guard<std::mutex> settingsUiLock(settingsUiMutex_);
        settingsUiControls_.clear();
        pendingSettingsUiControls_.clear();
    }
    uiIdCache_.clear();
}

bool ScriptInstance::checkDeadline()
{
    std::lock_guard<std::mutex> lock(sleepMutex_);
    if (stopReason_.load(std::memory_order_acquire) != StopReason::None) {
        return false;
    }
    if (deadline_ == std::chrono::steady_clock::time_point{}) {
        return true;
    }
    if (std::chrono::steady_clock::now() <= deadline_) {
        return true;
    }
    setStopReason(StopReason::Timeout);
    return false;
}

const char* ScriptInstance::stopReasonMessage() const
{
    switch (stopReason_.load(std::memory_order_acquire)) {
    case StopReason::Timeout:
        return "script execution timed out";
    case StopReason::Cancelled:
        return "script execution cancelled";
    default:
        return "script execution failed";
    }
}

void ScriptInstance::setStopReason(StopReason reason)
{
    StopReason expected = StopReason::None;
    if (stopReason_.compare_exchange_strong(expected, reason, std::memory_order_acq_rel)) {
#if defined(_WIN32)
        if (cancelEvent_) {
            SetEvent(static_cast<HANDLE>(cancelEvent_));
        }
#endif
        sleepCv_.notify_all();
    }
}

void ScriptInstance::requestCancel()
{
    setStopReason(StopReason::Cancelled);
}

void ScriptInstance::throwStopIfRequested(lua_State* L)
{
    if (!checkDeadline()) {
        luaL_error(L, "%s", stopReasonMessage());
    }
}

void ScriptInstance::pauseExecutionBudget()
{
    std::lock_guard<std::mutex> lock(sleepMutex_);
    if (!budgetActive_) {
        return;
    }

    const auto now = std::chrono::steady_clock::now();
    if (deadline_ != std::chrono::steady_clock::time_point{} && now > deadline_) {
        setStopReason(StopReason::Timeout);
    }

    budgetActive_ = false;
    deadline_ = {};
}

bool ScriptInstance::resumeExecutionBudget()
{
    std::lock_guard<std::mutex> lock(sleepMutex_);
    if (budgetActive_) {
        return true;
    }
    if (stopReason_.load(std::memory_order_acquire) != StopReason::None) {
        return false;
    }

    const auto limit = activeBudgetLimit_ > std::chrono::steady_clock::duration::zero()
        ? activeBudgetLimit_
        : kMaxScriptRuntime;
    budgetActive_ = true;
    activeBudgetStart_ = std::chrono::steady_clock::now();
    deadline_ = activeBudgetStart_ + limit;
    return true;
}

std::chrono::steady_clock::time_point ScriptInstance::computeWakeTime(std::chrono::milliseconds duration) const
{
    const auto now = std::chrono::steady_clock::now();
    if (duration <= std::chrono::milliseconds::zero()) {
        return now;
    }

    const auto maxTime = std::chrono::steady_clock::time_point::max();
    const auto maxDuration = std::chrono::duration_cast<std::chrono::milliseconds>(maxTime - now);
    if (duration >= maxDuration) {
        return maxTime;
    }
    return now + duration;
}

std::chrono::steady_clock::time_point ScriptInstance::computeWakeTime(std::chrono::microseconds duration) const
{
    const auto now = std::chrono::steady_clock::now();
    if (duration <= std::chrono::microseconds::zero()) {
        return now;
    }

    const auto maxTime = std::chrono::steady_clock::time_point::max();
    const auto maxDuration = std::chrono::duration_cast<std::chrono::microseconds>(maxTime - now);
    if (duration >= maxDuration) {
        return maxTime;
    }
    return now + duration;
}

bool ScriptInstance::spinUntil(std::chrono::steady_clock::time_point wakeTime)
{
    while (std::chrono::steady_clock::now() < wakeTime) {
        if (stopReason_.load(std::memory_order_acquire) != StopReason::None) {
            return false;
        }
        CpuRelax();
    }
    return stopReason_.load(std::memory_order_acquire) == StopReason::None;
}

bool ScriptInstance::waitUntil(std::chrono::steady_clock::time_point wakeTime, WaitPrecision precision)
{
    if (stopReason_.load(std::memory_order_acquire) != StopReason::None) {
        return false;
    }

#if defined(_WIN32)
    if (cancelEvent_) {
        const bool ok = WaitUntilWithWindowsTimer(wakeTime, static_cast<HANDLE>(cancelEvent_), precision == WaitPrecision::Precise);
        return ok && stopReason_.load(std::memory_order_acquire) == StopReason::None;
    }
#endif

    if (precision == WaitPrecision::Precise && wakeTime != std::chrono::steady_clock::time_point::max()) {
        while (true) {
            if (stopReason_.load(std::memory_order_acquire) != StopReason::None) {
                return false;
            }

            const auto now = std::chrono::steady_clock::now();
            if (now >= wakeTime) {
                return true;
            }

            const auto remaining = wakeTime - now;
            if (remaining <= kPreciseSleepSpinThreshold) {
                return spinUntil(wakeTime);
            }

            const auto coarseWakeTime = wakeTime - kPreciseSleepSpinThreshold;
            std::unique_lock<std::mutex> lock(sleepMutex_);
            sleepCv_.wait_until(lock, coarseWakeTime, [this] {
                return stopReason_.load(std::memory_order_acquire) != StopReason::None;
            });
        }
    }

    std::unique_lock<std::mutex> lock(sleepMutex_);
    sleepCv_.wait_until(lock, wakeTime, [this] {
        return stopReason_.load(std::memory_order_acquire) != StopReason::None;
    });
    return stopReason_.load(std::memory_order_acquire) == StopReason::None;
}

bool ScriptInstance::waitFor(std::chrono::milliseconds duration)
{
    pauseExecutionBudget();
    const auto wakeTime = computeWakeTime(duration);
    const bool ok = waitUntil(wakeTime, WaitPrecision::Coarse);
    if (!ok) {
        return false;
    }
    return resumeExecutionBudget();
}

bool ScriptInstance::waitFor(std::chrono::microseconds duration, WaitPrecision precision)
{
    pauseExecutionBudget();
    const auto wakeTime = computeWakeTime(duration);
    const bool ok = waitUntil(wakeTime, precision);
    if (!ok) {
        return false;
    }
    return resumeExecutionBudget();
}

void ScriptInstance::sleepWithDeadline(std::int64_t ms)
{
    const std::int64_t clamped = std::clamp<std::int64_t>(ms, 0, kMaxSingleSleepMs);
    if (!waitFor(std::chrono::milliseconds(clamped))) {
        throwStopIfRequested(L_);
    }
}

void ScriptInstance::sleepMicrosWithDeadline(std::int64_t us)
{
    constexpr auto kMaxSingleSleepMicros = std::chrono::duration_cast<std::chrono::microseconds>(std::chrono::hours(24)).count();
    const std::int64_t clamped = std::clamp<std::int64_t>(us, 0, kMaxSingleSleepMicros);
    if (!waitFor(std::chrono::microseconds(clamped), WaitPrecision::Precise)) {
        throwStopIfRequested(L_);
    }
}

void ScriptInstance::scheduleCoroutineSleep(lua_State* thread, std::int64_t ms)
{
    const std::int64_t clampedMs = std::clamp<std::int64_t>(ms, 0, kMaxSingleSleepMs);
    const auto wakeTime = computeWakeTime(std::chrono::milliseconds(clampedMs));
    for (auto& coroutine : sleepingCoroutines_) {
        if (coroutine.thread == thread) {
            coroutine.wakeTime = wakeTime;
            coroutine.precision = WaitPrecision::Coarse;
            return;
        }
    }

    sleepingCoroutines_.reserve(sleepingCoroutines_.size() + 1);
    lua_pushthread(thread);
    const int registryRef = luaL_ref(thread, LUA_REGISTRYINDEX);
    sleepingCoroutines_.push_back(SleepingCoroutine{thread, registryRef, wakeTime, WaitPrecision::Coarse});
}

void ScriptInstance::scheduleCoroutineSleepMicros(lua_State* thread, std::int64_t us)
{
    constexpr auto kMaxSingleSleepMicros = std::chrono::duration_cast<std::chrono::microseconds>(std::chrono::hours(24)).count();
    const std::int64_t clampedUs = std::clamp<std::int64_t>(us, 0, kMaxSingleSleepMicros);
    const auto wakeTime = computeWakeTime(std::chrono::microseconds(clampedUs));
    for (auto& coroutine : sleepingCoroutines_) {
        if (coroutine.thread == thread) {
            coroutine.wakeTime = wakeTime;
            coroutine.precision = WaitPrecision::Precise;
            return;
        }
    }

    sleepingCoroutines_.reserve(sleepingCoroutines_.size() + 1);
    lua_pushthread(thread);
    const int registryRef = luaL_ref(thread, LUA_REGISTRYINDEX);
    sleepingCoroutines_.push_back(SleepingCoroutine{thread, registryRef, wakeTime, WaitPrecision::Precise});
}

void ScriptInstance::checkpoint(lua_State* L)
{
    throwStopIfRequested(L);
    pauseExecutionBudget();
    std::this_thread::yield();
    if (!resumeExecutionBudget()) {
        throwStopIfRequested(L);
    }
}

bool ScriptInstance::shouldYield(std::chrono::microseconds threshold)
{
    if (stopReason_.load(std::memory_order_acquire) != StopReason::None) {
        return true;
    }
    std::lock_guard<std::mutex> lock(sleepMutex_);
    if (!budgetActive_) {
        return false;
    }
    return std::chrono::steady_clock::now() - activeBudgetStart_ >= threshold;
}

void ScriptInstance::releaseSleepingCoroutine(SleepingCoroutine& coroutine)
{
    if (L_ && coroutine.registryRef != LUA_NOREF && coroutine.registryRef != LUA_REFNIL) {
        luaL_unref(L_, LUA_REGISTRYINDEX, coroutine.registryRef);
    }
    coroutine.thread = nullptr;
    coroutine.registryRef = LUA_NOREF;
}

void ScriptInstance::releaseAllSleepingCoroutines()
{
    for (auto& coroutine : sleepingCoroutines_) {
        releaseSleepingCoroutine(coroutine);
    }
    sleepingCoroutines_.clear();
}

bool ScriptInstance::drainSleepingCoroutines()
{
    while (!sleepingCoroutines_.empty()) {
        const auto now = std::chrono::steady_clock::now();
        auto nextWake = sleepingCoroutines_.front().wakeTime;
        auto nextWakePrecision = sleepingCoroutines_.front().precision;
        for (const auto& coroutine : sleepingCoroutines_) {
            if (coroutine.wakeTime < nextWake) {
                nextWake = coroutine.wakeTime;
                nextWakePrecision = coroutine.precision;
            } else if (coroutine.wakeTime == nextWake && coroutine.precision == WaitPrecision::Precise) {
                nextWakePrecision = WaitPrecision::Precise;
            }
        }

        pauseExecutionBudget();
        if (nextWake > now) {
            if (!waitUntil(nextWake, nextWakePrecision)) {
                owner_->setLastError(stopReasonMessage());
                LogWarning("Imported script failed during coroutine sleep resume: " + owner_->lastErrorCopy());
                releaseAllSleepingCoroutines();
                return false;
            }
        }

        if (!resumeExecutionBudget()) {
            owner_->setLastError(stopReasonMessage());
            LogWarning("Imported script failed during coroutine sleep resume: " + owner_->lastErrorCopy());
            releaseAllSleepingCoroutines();
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
                releaseSleepingCoroutine(*it);
                it = sleepingCoroutines_.erase(it);
                continue;
            }

            if (lua_status(thread) != LUA_YIELD) {
                releaseSleepingCoroutine(*it);
                it = sleepingCoroutines_.erase(it);
                continue;
            }

            const lua_Hook previousHook = lua_gethook(thread);
            const int previousMask = lua_gethookmask(thread);
            const int previousCount = lua_gethookcount(thread);
            InstallTimeoutHook(thread);
            int resultCount = 0;
            const int status = lua_resume(thread, L_, 0, &resultCount);
            RestoreHook(thread, previousHook, previousMask, previousCount);

            if (status == LUA_OK) {
                lua_pop(thread, resultCount);
                releaseSleepingCoroutine(*it);
                it = sleepingCoroutines_.erase(it);
                if (stopReason_.load(std::memory_order_acquire) != StopReason::None) {
                    owner_->setLastError(stopReasonMessage());
                    LogWarning("Imported script failed during coroutine sleep resume: " + owner_->lastErrorCopy());
                    releaseAllSleepingCoroutines();
                    return false;
                }
                continue;
            }

            if (status == LUA_YIELD) {
                lua_pop(thread, resultCount);
                ++it;
                if (stopReason_.load(std::memory_order_acquire) != StopReason::None) {
                    owner_->setLastError(stopReasonMessage());
                    LogWarning("Imported script failed during coroutine sleep resume: " + owner_->lastErrorCopy());
                    releaseAllSleepingCoroutines();
                    return false;
                }
                continue;
            }

            if (stopReason_.load(std::memory_order_acquire) != StopReason::None) {
                owner_->setLastError(stopReasonMessage());
            } else {
                const char* message = lua_tostring(thread, -1);
                owner_->setLastError(message ? message : "Lua coroutine failed.");
            }
            lua_pop(thread, 1);
            LogWarning(std::string("Imported script failed during coroutine sleep resume: ") + owner_->lastErrorCopy());
            releaseAllSleepingCoroutines();
            return false;
        }

        pauseExecutionBudget();
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

    if (enabled && !Globals::bWinDivertEnabled) {
        std::string error;
        if (backend->init(&error)) {
            Globals::bWinDivertEnabled = true;
            owner_->clearLastWarning();
        } else {
#if defined(_WIN32)
            if (!smu::platform::windows::IsRunAsAdmin()) {
                owner_->setLastWarning(kLagSwitchRequiresAdminWarning);
                LogWarning(kLagSwitchRequiresAdminWarning);
                return;
            }
#endif
            if (error.empty()) {
                error = "Lag switch backend could not be initialized.";
            }
            owner_->setLastError(error);
            LogWarning(error);
            return;
        }
    }

    touchedLagSwitch_ = true;
    backend->setScriptBlockingActive(lagSwitchOwnerToken(), enabled);
    Globals::g_windivert_blocking.store(backend->isBlockingActive(), std::memory_order_relaxed);
}

void ScriptInstance::setLagSwitchConfig(const smu::platform::LagSwitchConfig& config)
{
    auto backend = smu::platform::GetNetworkLagBackend();
    if (!backend) {
        return;
    }

    touchedLagSwitch_ = true;
    backend->setScriptConfigOverride(lagSwitchOwnerToken(), config);
    Globals::g_windivert_blocking.store(backend->isBlockingActive(), std::memory_order_relaxed);
}

void ScriptInstance::clearLagSwitchConfig()
{
    auto backend = smu::platform::GetNetworkLagBackend();
    if (!backend) {
        return;
    }

    backend->clearScriptConfigOverride(lagSwitchOwnerToken());
    Globals::g_windivert_blocking.store(backend->isBlockingActive(), std::memory_order_relaxed);
}

smu::platform::LagSwitchConfig ScriptInstance::lagSwitchConfig() const
{
    auto backend = smu::platform::GetNetworkLagBackend();
    if (!backend) {
        return {};
    }
    return backend->effectiveConfig();
}

std::uintptr_t ScriptInstance::lagSwitchOwnerToken() const
{
    return reinterpret_cast<std::uintptr_t>(this);
}

void ScriptInstance::releaseLagSwitchControls()
{
    if (!touchedLagSwitch_) {
        return;
    }

    if (auto backend = smu::platform::GetNetworkLagBackend()) {
        backend->clearScriptState(lagSwitchOwnerToken());
        Globals::g_windivert_blocking.store(backend->isBlockingActive(), std::memory_order_relaxed);
    }
    touchedLagSwitch_ = false;
}

bool ScriptInstance::callProtected(int argCount, const char* context)
{
    beginTimedCall();
    const int status = lua_pcall(L_, argCount, 0, 0);
    endTimedCall();

    if (status != LUA_OK) {
        const char* message = stopReason_.load(std::memory_order_acquire) != StopReason::None
            ? stopReasonMessage()
            : lua_tostring(L_, -1);
        owner_->setLastError(message ? message : "Lua call failed.");
        lua_pop(L_, 1);
        releaseAllSleepingCoroutines();
        LogWarning(std::string("Imported script failed during ") + context + ": " + owner_->lastErrorCopy());
        return false;
    }

    if (stopReason_.load(std::memory_order_acquire) != StopReason::None) {
        owner_->setLastError(stopReasonMessage());
        LogWarning(std::string("Imported script failed during ") + context + ": " + owner_->lastErrorCopy());
        releaseAllSleepingCoroutines();
        return false;
    }

    owner_->clearLastError();
    return true;
}

void ScriptInstance::beginTimedCall(std::chrono::steady_clock::duration maxRuntime)
{
    std::lock_guard<std::mutex> lock(sleepMutex_);
    stopReason_.store(StopReason::None, std::memory_order_release);
#if defined(_WIN32)
    if (cancelEvent_) {
        ResetEvent(static_cast<HANDLE>(cancelEvent_));
    }
#endif
    activeBudgetLimit_ = maxRuntime;
    budgetActive_ = true;
    activeBudgetStart_ = std::chrono::steady_clock::now();
    deadline_ = activeBudgetStart_ + activeBudgetLimit_;
    InstallTimeoutHook(L_);
}

void ScriptInstance::beginTimedCall()
{
    beginTimedCall(kMaxScriptRuntime);
}

void ScriptInstance::endTimedCall()
{
    pauseExecutionBudget();
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

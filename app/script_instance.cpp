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

extern "C" {
#include "lauxlib.h"
#include "lualib.h"
}

namespace smu::app {
namespace {

constexpr const char* kRegistryInstanceKey = "SMU.ScriptInstance";
constexpr auto kMaxScriptRuntime = std::chrono::seconds(30);
constexpr auto kMaxSettingsRuntime = std::chrono::seconds(5);
constexpr int kLuaHookInstructionCount = 10000;
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
}

ScriptInstance::~ScriptInstance()
{
    cleanup();
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
    SyncLuaSettingsTable(L_, owner_->uiState);
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
        owner_->setLastError("Script is not loaded.");
        return false;
    }

    lua_getglobal(L_, "onExecute");
    if (!lua_isfunction(L_, -1)) {
        lua_pop(L_, 1);
        owner_->setLastError("Script does not define onExecute().");
        return false;
    }

    beginTimedCall(kMaxScriptRuntime);
    const int status = lua_pcall(L_, 0, 0, 0);
    if (status != LUA_OK) {
        endTimedCall();
        const char* message = stopReason_.load(std::memory_order_acquire) != StopReason::None
            ? stopReasonMessage()
            : lua_tostring(L_, -1);
        owner_->setLastError(message ? message : "Lua call failed.");
        lua_pop(L_, 1);
        releaseAllSleepingCoroutines();
        LogWarning(std::string("Imported script failed during run onExecute: ") + owner_->lastErrorCopy());
        return false;
    }

    if (!drainSleepingCoroutines()) {
        endTimedCall();
        return false;
    }

    endTimedCall();
    if (stopReason_.load(std::memory_order_acquire) != StopReason::None) {
        owner_->setLastError(stopReasonMessage());
        LogWarning(std::string("Imported script failed during run onExecute: ") + owner_->lastErrorCopy());
        releaseAllSleepingCoroutines();
        return false;
    }
    owner_->clearLastError();
    return true;
}

void ScriptInstance::syncSettingsTable()
{
    if (L_) {
        SyncLuaSettingsTable(L_, owner_->uiState);
        syncUiIdCache();
    }
}

void ScriptInstance::syncUiIdCache()
{
    uiIdCache_.clear();
    if (!owner_ || !owner_->uiState.is_object()) {
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

bool ScriptInstance::callOnSettings(bool renderMode)
{
    if (!L_) {
        owner_->setLastError("Script is not loaded.");
        return false;
    }

    lua_getglobal(L_, "onSettings");
    if (!lua_isfunction(L_, -1)) {
        lua_pop(L_, 1);
        return true;
    }

    resetSettingsUiControlCount();
    setSettingsRenderMode(renderMode);
    beginTimedCall(kMaxSettingsRuntime);
    const int status = lua_pcall(L_, 0, 0, 0);
    endTimedCall();
    setSettingsRenderMode(false);
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

    return true;
}

void ScriptInstance::cleanup()
{
    requestCancel();
    setFreeze(false);
    if (L_) {
        releaseAllSleepingCoroutines();
        lua_close(L_);
        L_ = nullptr;
    }
    memoryUsedBytes_ = 0;
    settingsRenderMode_ = false;
    settingsUiControlCount_ = 0;
    transientUi_.clear();
    textboxBuffers_.clear();
    dynamicTextboxBuffers_.clear();
    keybindValues_.clear();
    uiIdCache_.clear();
}

bool ScriptInstance::checkDeadline()
{
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
    if (!budgetActive_) {
        return;
    }
    const auto now = std::chrono::steady_clock::now();
    const auto elapsed = now - activeBudgetStart_;
    if (elapsed >= remainingBudget_) {
        remainingBudget_ = std::chrono::steady_clock::duration::zero();
        setStopReason(StopReason::Timeout);
    } else {
        remainingBudget_ -= elapsed;
    }
    budgetActive_ = false;
    deadline_ = {};
}

bool ScriptInstance::resumeExecutionBudget()
{
    if (budgetActive_) {
        return true;
    }
    if (remainingBudget_ <= std::chrono::steady_clock::duration::zero()) {
        setStopReason(StopReason::Timeout);
        return false;
    }
    budgetActive_ = true;
    activeBudgetStart_ = std::chrono::steady_clock::now();
    deadline_ = activeBudgetStart_ + remainingBudget_;
    return true;
}

std::chrono::steady_clock::time_point ScriptInstance::computeWakeTime(std::int64_t ms) const
{
    const auto now = std::chrono::steady_clock::now();
    if (ms <= 0) {
        return now;
    }
    const auto duration = std::chrono::milliseconds(ms);
    const auto maxTime = std::chrono::steady_clock::time_point::max();
    if (duration >= (maxTime - now)) {
        return maxTime;
    }
    return now + duration;
}

bool ScriptInstance::waitUntil(std::chrono::steady_clock::time_point wakeTime)
{
    if (stopReason_.load(std::memory_order_acquire) != StopReason::None) {
        return false;
    }
    std::unique_lock<std::mutex> lock(sleepMutex_);
    if (sleepCv_.wait_until(lock, wakeTime, [this] {
            return stopReason_.load(std::memory_order_acquire) != StopReason::None;
        })) {
        return false;
    }
    return stopReason_.load(std::memory_order_acquire) == StopReason::None;
}

bool ScriptInstance::waitFor(std::chrono::milliseconds duration)
{
    pauseExecutionBudget();
    const auto wakeTime = computeWakeTime(duration.count());
    const bool ok = waitUntil(wakeTime);
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

void ScriptInstance::scheduleCoroutineSleep(lua_State* thread, std::int64_t ms)
{
    const std::int64_t clampedMs = std::clamp<std::int64_t>(ms, 0, kMaxSingleSleepMs);
    const auto wakeTime = computeWakeTime(clampedMs);
    for (auto& coroutine : sleepingCoroutines_) {
        if (coroutine.thread == thread) {
            coroutine.wakeTime = wakeTime;
            return;
        }
    }

    sleepingCoroutines_.reserve(sleepingCoroutines_.size() + 1);
    lua_pushthread(thread);
    const int registryRef = luaL_ref(thread, LUA_REGISTRYINDEX);
    sleepingCoroutines_.push_back(SleepingCoroutine{thread, registryRef, wakeTime});
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
        for (const auto& coroutine : sleepingCoroutines_) {
            nextWake = std::min(nextWake, coroutine.wakeTime);
        }

        pauseExecutionBudget();
        if (nextWake > now) {
            if (!waitUntil(nextWake)) {
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
            owner_->setLastError(error);
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
    stopReason_.store(StopReason::None, std::memory_order_release);
    remainingBudget_ = maxRuntime;
    budgetActive_ = true;
    activeBudgetStart_ = std::chrono::steady_clock::now();
    deadline_ = activeBudgetStart_ + remainingBudget_;
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

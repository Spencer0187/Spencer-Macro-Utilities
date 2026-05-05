#include "script_api.h"

#include "input_actions.h"
#include "app_ui_controls.h"
#include "profile_manager.h"
#include "script_instance.h"
#include "script_manager.h"
#include "script_metadata.h"
#include "../core/legacy_globals.h"
#include "../platform/input_backend.h"
#include "../platform/logging.h"
#include "../platform/text_input_backend.h"

#include <algorithm>
#include <cstdio>
#include <array>
#include <chrono>
#include <cstring>
#include <cstdint>
#include <cmath>
#include <limits>
#include <memory>
#include <type_traits>
#include <optional>
#include <unordered_map>
#include <variant>
#include <string>

#include "imgui.h"

extern "C" {
#include "lauxlib.h"
#include "lua.h"
}

namespace smu::app {
namespace {

ScriptInstance& RequireInstance(lua_State* L)
{
    if (ScriptInstance* instance = GetScriptInstance(L)) {
        return *instance;
    }
    luaL_error(L, "SMU script context is not available");
    throw 0;
}

constexpr int kMaxSingleSleepMs = 300000;
constexpr int kMaxInputDelayMs = 300000;
constexpr float kMaxUiDimension = 10000.0f;

int CheckLuaInt(lua_State* L, int index, int minValue, int maxValue, const char* name)
{
    int isInteger = 0;
    const lua_Integer value = lua_tointegerx(L, index, &isInteger);
    if (!isInteger) {
        return static_cast<int>(luaL_error(L, "%s must be an integer", name));
    }
    if (value < static_cast<lua_Integer>(minValue) || value > static_cast<lua_Integer>(maxValue)) {
        return static_cast<int>(luaL_error(L, "%s is outside the allowed range", name));
    }
    return static_cast<int>(value);
}

int CheckLuaIntClamped(lua_State* L, int index, int minValue, int maxValue, const char* name)
{
    int isInteger = 0;
    const lua_Integer value = lua_tointegerx(L, index, &isInteger);
    if (!isInteger) {
        return static_cast<int>(luaL_error(L, "%s must be an integer", name));
    }
    if (value < static_cast<lua_Integer>(minValue)) {
        return minValue;
    }
    if (value > static_cast<lua_Integer>(maxValue)) {
        return maxValue;
    }
    return static_cast<int>(value);
}

float CheckOptionalNonNegativeFloat(lua_State* L, int index, float defaultValue, const char* name)
{
    if (lua_gettop(L) < index || lua_isnil(L, index)) {
        return defaultValue;
    }

    int isNumber = 0;
    const lua_Number number = lua_tonumberx(L, index, &isNumber);
    if (!isNumber) {
        return static_cast<float>(luaL_error(L, "%s must be a number", name));
    }
    if (!std::isfinite(static_cast<double>(number))) {
        return static_cast<float>(luaL_error(L, "%s must be a finite number", name));
    }
    if (number <= 0.0) {
        return 0.0f;
    }
    if (number > static_cast<lua_Number>(kMaxUiDimension)) {
        return kMaxUiDimension;
    }
    return static_cast<float>(number);
}

double CheckOptionalFiniteNumber(lua_State* L, int index, double defaultValue, const char* name)
{
    if (lua_gettop(L) < index || lua_isnil(L, index)) {
        return defaultValue;
    }

    int isNumber = 0;
    const lua_Number number = lua_tonumberx(L, index, &isNumber);
    if (!isNumber) {
        return static_cast<double>(luaL_error(L, "%s must be a number", name));
    }
    if (!std::isfinite(static_cast<double>(number))) {
        return static_cast<double>(luaL_error(L, "%s must be a finite number", name));
    }
    return static_cast<double>(number);
}

const char* CheckUiId(lua_State* L, int index, std::size_t& length)
{
    const char* id = luaL_checklstring(L, index, &length);
    if (length == 0) {
        luaL_argerror(L, index, "UI id must not be empty");
    }
    if (length > kMaxUiIdBytes) {
        luaL_argerror(L, index, "UI id is too long");
    }
    if (std::memchr(id, '\0', length)) {
        luaL_argerror(L, index, "UI id must not contain embedded NUL bytes");
    }
    return id;
}

void CheckUiControlBudget(lua_State* L, ScriptInstance& instance)
{
    if (!instance.tryConsumeSettingsUiControl()) {
        luaL_error(L, "script settings created too many UI controls");
    }
}

std::string ClampedString(const char* text, std::size_t length)
{
    const std::size_t clampedLength = std::min(length, kMaxUiStateStringBytes);
    return std::string(text ? text : "", clampedLength);
}

smu::core::KeyCode CheckKey(lua_State* L, int index)
{
    if (lua_isinteger(L, index)) {
        const int keyValue = CheckLuaInt(L, index, 0, std::numeric_limits<int>::max(), "key");
        const auto key = static_cast<smu::core::KeyCode>(keyValue);
        if ((key & smu::core::HOTKEY_KEY_MASK) != smu::core::SMU_VK_NONE) {
            return key;
        }
    }

    const char* keyName = luaL_checkstring(L, index);
    if (auto key = ParseScriptKeyName(keyName)) {
        return *key;
    }

    return static_cast<smu::core::KeyCode>(luaL_error(L, "invalid key: %s", keyName ? keyName : "<null>"));
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

unsigned int NormalizeScriptHotkey(unsigned int hotkey)
{
    if (!IsScriptHotkeyBound(hotkey)) {
        return kScriptUnboundHotkey;
    }
    return hotkey;
}

bool IsModifierPressed(const smu::platform::InputBackend& input, smu::core::KeyCode key)
{
    switch (key) {
    case smu::core::SMU_VK_SHIFT:
        return input.isKeyPressed(smu::core::SMU_VK_SHIFT) || input.isKeyPressed(smu::core::SMU_VK_LSHIFT) ||
            input.isKeyPressed(smu::core::SMU_VK_RSHIFT);
    case smu::core::SMU_VK_CONTROL:
        return input.isKeyPressed(smu::core::SMU_VK_CONTROL) || input.isKeyPressed(smu::core::SMU_VK_LCONTROL) ||
            input.isKeyPressed(smu::core::SMU_VK_RCONTROL);
    case smu::core::SMU_VK_MENU:
        return input.isKeyPressed(smu::core::SMU_VK_MENU) || input.isKeyPressed(smu::core::SMU_VK_LMENU) ||
            input.isKeyPressed(smu::core::SMU_VK_RMENU);
    case smu::core::SMU_VK_LWIN:
        return input.isKeyPressed(smu::core::SMU_VK_LWIN) || input.isKeyPressed(smu::core::SMU_VK_RWIN);
    default:
        return input.isKeyPressed(key);
    }
}

bool AreHotkeyModifiersPressed(const smu::platform::InputBackend& input, unsigned int combinedKey)
{
    if ((combinedKey & smu::core::HOTKEY_MASK_WIN) && !IsModifierPressed(input, smu::core::SMU_VK_LWIN)) {
        return false;
    }
    if ((combinedKey & smu::core::HOTKEY_MASK_CTRL) && !IsModifierPressed(input, smu::core::SMU_VK_CONTROL)) {
        return false;
    }
    if ((combinedKey & smu::core::HOTKEY_MASK_ALT) && !IsModifierPressed(input, smu::core::SMU_VK_MENU)) {
        return false;
    }
    if ((combinedKey & smu::core::HOTKEY_MASK_SHIFT) && !IsModifierPressed(input, smu::core::SMU_VK_SHIFT)) {
        return false;
    }
    return true;
}

bool IsHotkeyPressed(unsigned int combinedKey)
{
    if (!IsScriptHotkeyBound(combinedKey)) {
        return false;
    }

    auto backend = smu::platform::GetInputBackend();
    if (!backend) {
        return false;
    }

    if (!AreHotkeyModifiersPressed(*backend, combinedKey)) {
        return false;
    }

    const auto key = static_cast<smu::core::KeyCode>(combinedKey & smu::core::HOTKEY_KEY_MASK);
    return IsModifierPressed(*backend, key);
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

void SyncLuaSetting(lua_State* L, const std::string& name, const SavedSettingValue& value)
{
    lua_getglobal(L, "settings");
    if (!lua_istable(L, -1)) {
        lua_pop(L, 1);
        lua_newtable(L);
        lua_pushvalue(L, -1);
        lua_setglobal(L, "settings");
    }

    PushLuaSettingValue(L, value);
    lua_setfield(L, -2, name.c_str());
    lua_pop(L, 1);
}

void UpdateScriptSetting(ScriptInstance& instance, const std::string& name, const SavedSettingValue& value)
{
    ImportedScriptRecord& record = instance.owner();
    std::visit([&record, &name](const auto& storedValue) {
        record.uiState[name] = storedValue;
    }, value);
    SyncLuaSetting(instance.luaState(), name, value);
}

void UpdateScriptStringSetting(ScriptInstance& instance, const std::string& name, std::string value)
{
    if (value.size() > kMaxUiStateStringBytes) {
        value.resize(kMaxUiStateStringBytes);
    }
    UpdateScriptSetting(instance, name, SavedSettingValue{std::move(value)});
}

template <std::size_t N>
void CopyToBuffer(std::array<char, N>& buffer, const std::string& text)
{
    std::snprintf(buffer.data(), buffer.size(), "%s", text.c_str());
}

std::string ClampDynamicText(std::string text)
{
    if (text.size() > kMaxUiStateStringBytes) {
        text = text.substr(text.size() - kMaxUiStateStringBytes);
    }
    return text;
}

bool IsSettingsRenderMode(lua_State* L)
{
    return RequireInstance(L).isSettingsRenderMode();
}

int LuaLog(lua_State* L)
{
    const char* message = luaL_checkstring(L, 1);
    LogInfo(std::string("[script] ") + (message ? message : ""));
    return 0;
}

int LuaNowMicros(lua_State* L)
{
    using clock = std::chrono::steady_clock;

    const auto now = clock::now().time_since_epoch();
    const auto micros = std::chrono::duration_cast<std::chrono::microseconds>(now).count();

    lua_pushinteger(L, static_cast<lua_Integer>(micros));
    return 1;
}

int LuaSleep(lua_State* L)
{
    if (IsSettingsRenderMode(L)) {
        return luaL_error(L, "sleep is not available while rendering script settings");
    }
    ScriptInstance& instance = RequireInstance(L);
    const int ms = CheckLuaIntClamped(L, 1, 0, kMaxSingleSleepMs, "sleep duration");
    if (lua_isyieldable(L)) {
        instance.scheduleCoroutineSleep(L, ms);
        return lua_yield(L, 0);
    }
    instance.sleepWithDeadline(ms);
    return 0;
}

int LuaPressKey(lua_State* L)
{
    if (IsSettingsRenderMode(L)) {
        return luaL_error(L, "pressKey is not available while rendering script settings");
    }
    const smu::core::KeyCode key = CheckKey(L, 1);
    const int delay = lua_gettop(L) >= 2 ? CheckLuaIntClamped(L, 2, 0, kMaxInputDelayMs, "delay") : 50;
    PressKey(key, delay);
    return 0;
}

int LuaHoldKey(lua_State* L)
{
    if (IsSettingsRenderMode(L)) {
        return luaL_error(L, "holdKey is not available while rendering script settings");
    }
    HoldKey(CheckKey(L, 1));
    return 0;
}

int LuaReleaseKey(lua_State* L)
{
    if (IsSettingsRenderMode(L)) {
        return luaL_error(L, "releaseKey is not available while rendering script settings");
    }
    ReleaseKey(CheckKey(L, 1));
    return 0;
}

int LuaIsKeyPressed(lua_State* L)
{
    if (IsSettingsRenderMode(L)) {
        return luaL_error(L, "isKeyPressed is not available while rendering script settings");
    }
    lua_pushboolean(L, IsKeyPressed(CheckKey(L, 1)));
    return 1;
}

int LuaIsHotkeyPressed(lua_State* L)
{
    if (IsSettingsRenderMode(L)) {
        return luaL_error(L, "isHotkeyPressed is not available while rendering script settings");
    }

    unsigned int combinedKey = 0;
    if (lua_isinteger(L, 1)) {
        combinedKey = static_cast<unsigned int>(CheckLuaInt(L, 1, 0, std::numeric_limits<int>::max(), "hotkey"));
    } else {
        const char* text = luaL_checkstring(L, 1);
        if (auto parsed = ParseScriptHotkeyString(text ? text : "")) {
            combinedKey = *parsed;
        } else {
            return luaL_error(L, "invalid hotkey: %s", text ? text : "<null>");
        }
    }

    combinedKey = NormalizeScriptHotkey(combinedKey);
    lua_pushboolean(L, IsHotkeyPressed(combinedKey));
    return 1;
}

int LuaTypeText(lua_State* L)
{
    if (IsSettingsRenderMode(L)) {
        return luaL_error(L, "typeText is not available while rendering script settings");
    }
    std::size_t textLength = 0;
    const char* text = luaL_checklstring(L, 1, &textLength);
    const int delay = lua_gettop(L) >= 2 ? CheckLuaIntClamped(L, 2, 0, kMaxInputDelayMs, "delay") : 30;
    auto backend = smu::platform::GetInputBackend();
    if (!backend) {
        LogWarning("Imported script tried to type text, but the input backend is not available.");
        return 0;
    }
    smu::platform::typeText(*backend, std::string(text ? text : "", textLength), delay);
    return 0;
}

int LuaMoveMouse(lua_State* L)
{
    if (IsSettingsRenderMode(L)) {
        return luaL_error(L, "moveMouse is not available while rendering script settings");
    }
    const int dx = CheckLuaInt(L, 1, std::numeric_limits<int>::min(), std::numeric_limits<int>::max(), "dx");
    const int dy = CheckLuaInt(L, 2, std::numeric_limits<int>::min(), std::numeric_limits<int>::max(), "dy");
    MoveMouse(dx, dy);
    return 0;
}

int LuaMouseWheel(lua_State* L)
{
    if (IsSettingsRenderMode(L)) {
        return luaL_error(L, "mouseWheel is not available while rendering script settings");
    }
    const int delta = CheckLuaInt(L, 1, std::numeric_limits<int>::min(), std::numeric_limits<int>::max(), "delta");
    MouseWheel(delta);
    return 0;
}

int LuaFreeze(lua_State* L)
{
    if (IsSettingsRenderMode(L)) {
        return luaL_error(L, "freeze is not available while rendering script settings");
    }
    ScriptInstance& instance = RequireInstance(L);
    instance.setFreeze(lua_toboolean(L, 1) != 0);
    return 0;
}

int LuaLagSwitch(lua_State* L)
{
    if (IsSettingsRenderMode(L)) {
        return luaL_error(L, "lagSwitch is not available while rendering script settings");
    }
    ScriptInstance& instance = RequireInstance(L);
    instance.setLagSwitch(lua_toboolean(L, 1) != 0);
    return 0;
}

int LuaGetPlatform(lua_State* L)
{
#if defined(_WIN32)
    lua_pushliteral(L, "windows");
#elif defined(__linux__)
    lua_pushliteral(L, "linux");
#else
    lua_pushliteral(L, "unknown");
#endif
    return 1;
}

int LuaGetSMUVersion(lua_State* L)
{
    lua_pushstring(L, Globals::localVersion.c_str());
    return 1;
}

int LuaGetSavedValue(lua_State* L)
{
    const char* name = luaL_checkstring(L, 1);
    if (!name) {
        lua_pushnil(L);
        return 1;
    }

    const auto value = TryGetSavedSettingValue(name);
    if (!value) {
        lua_pushnil(L);
        return 1;
    }

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
    }, *value);

    return 1;
}

int LuaUiText(lua_State* L)
{
    ScriptInstance& instance = RequireInstance(L);
    CheckUiControlBudget(L, instance);
    std::size_t textLength = 0;
    const char* text = luaL_checklstring(L, 1, &textLength);
    const float width = CheckOptionalNonNegativeFloat(L, 2, 0.0f, "width");
    if (IsSettingsRenderMode(L) && text) {
        const int displayLength = static_cast<int>(std::min(textLength, kMaxUiStateStringBytes));
        if (width > 0.0f) {
            ImGui::PushTextWrapPos(ImGui::GetCursorPosX() + width);
            ImGui::TextWrapped("%.*s", displayLength, text);
            ImGui::PopTextWrapPos();
        } else {
            ImGui::TextWrapped("%.*s", displayLength, text);
        }
    }
    return 0;
}

int LuaUiSeparator(lua_State* L)
{
    ScriptInstance& instance = RequireInstance(L);
    CheckUiControlBudget(L, instance);
    if (IsSettingsRenderMode(L)) {
        ImGui::Separator();
        const float height = CheckOptionalNonNegativeFloat(L, 2, 0.0f, "height");
        if (height > 0.0f) {
            ImGui::Dummy(ImVec2(0.0f, height));
        }
    }
    return 0;
}

int LuaUiCheckbox(lua_State* L)
{
    ScriptInstance& instance = RequireInstance(L);
    CheckUiControlBudget(L, instance);
    std::size_t idLength = 0;
    const char* idText = CheckUiId(L, 1, idLength);
    const char* label = luaL_checkstring(L, 2);
    const bool defaultValue = lua_gettop(L) >= 3 ? (lua_toboolean(L, 3) != 0) : false;
    const float width = CheckOptionalNonNegativeFloat(L, 4, 0.0f, "width");

    const std::string id(idText, idLength);
    ImportedScriptRecord& record = instance.owner();
    const auto storedIt = record.uiState.find(id);
    const bool stored = storedIt != record.uiState.end() && storedIt->is_boolean()
        ? storedIt->get<bool>()
        : defaultValue;
    if (storedIt == record.uiState.end()) {
        UpdateScriptSetting(instance, id, SavedSettingValue{stored});
    }

    bool value = stored;
    if (instance.isSettingsRenderMode()) {
        ImGui::PushID(id.c_str());
        if (width > 0.0f) {
            if (ImGui::Checkbox("##checkbox", &value)) {
                UpdateScriptSetting(instance, id, SavedSettingValue{value});
            }
            ImGui::SameLine();
            ImGui::PushTextWrapPos(ImGui::GetCursorPosX() + width);
            ImGui::TextWrapped("%s", label);
            ImGui::PopTextWrapPos();
        } else {
            if (ImGui::Checkbox(label, &value)) {
                UpdateScriptSetting(instance, id, SavedSettingValue{value});
            }
        }
        ImGui::PopID();
    }

    lua_pushboolean(L, value);
    return 1;
}

int LuaUiSliderInt(lua_State* L)
{
    ScriptInstance& instance = RequireInstance(L);
    CheckUiControlBudget(L, instance);
    std::size_t idLength = 0;
    const char* idText = CheckUiId(L, 1, idLength);
    const char* label = luaL_checkstring(L, 2);
    const int defaultValue = lua_gettop(L) >= 3 && !lua_isnil(L, 3)
        ? CheckLuaInt(L, 3, std::numeric_limits<int>::min(), std::numeric_limits<int>::max(), "default value")
        : 0;
    int minValue = lua_gettop(L) >= 4 && !lua_isnil(L, 4)
        ? CheckLuaInt(L, 4, std::numeric_limits<int>::min(), std::numeric_limits<int>::max(), "min value")
        : defaultValue;
    int maxValue = lua_gettop(L) >= 5 && !lua_isnil(L, 5)
        ? CheckLuaInt(L, 5, std::numeric_limits<int>::min(), std::numeric_limits<int>::max(), "max value")
        : defaultValue;
    const float width = CheckOptionalNonNegativeFloat(L, 6, 0.0f, "width");
    if (minValue > maxValue) {
        std::swap(minValue, maxValue);
    }

    const std::string id(idText, idLength);
    ImportedScriptRecord& record = instance.owner();
    const auto storedIt = record.uiState.find(id);
    int stored = defaultValue;
    if (storedIt != record.uiState.end()) {
        if (storedIt->is_number_integer()) {
            const auto value = storedIt->get<std::int64_t>();
            stored = static_cast<int>(std::clamp<std::int64_t>(value, std::numeric_limits<int>::min(), std::numeric_limits<int>::max()));
        } else if (storedIt->is_number_unsigned()) {
            const auto value = storedIt->get<std::uint64_t>();
            stored = value > static_cast<std::uint64_t>(std::numeric_limits<int>::max())
                ? std::numeric_limits<int>::max()
                : static_cast<int>(value);
        }
    }
    if (storedIt == record.uiState.end()) {
        UpdateScriptSetting(instance, id, SavedSettingValue{static_cast<std::int64_t>(stored)});
    }

    int value = stored;
    if (instance.isSettingsRenderMode()) {
        ImGui::PushID(id.c_str());
        if (width > 0.0f) {
            ImGui::SetNextItemWidth(width);
        }
        if (ImGui::SliderInt(label, &value, minValue, maxValue)) {
            UpdateScriptSetting(instance, id, SavedSettingValue{static_cast<std::int64_t>(value)});
        }
        ImGui::PopID();
    }

    lua_pushinteger(L, value);
    return 1;
}

int LuaUiSliderFloat(lua_State* L)
{
    ScriptInstance& instance = RequireInstance(L);
    CheckUiControlBudget(L, instance);
    std::size_t idLength = 0;
    const char* idText = CheckUiId(L, 1, idLength);
    const char* label = luaL_checkstring(L, 2);
    const double defaultValue = CheckOptionalFiniteNumber(L, 3, 0.0, "default value");
    double minValue = CheckOptionalFiniteNumber(L, 4, defaultValue, "min value");
    double maxValue = CheckOptionalFiniteNumber(L, 5, defaultValue, "max value");
    const float width = CheckOptionalNonNegativeFloat(L, 6, 0.0f, "width");
    if (minValue > maxValue) {
        std::swap(minValue, maxValue);
    }
    minValue = std::clamp(minValue, static_cast<double>(-std::numeric_limits<float>::max()), static_cast<double>(std::numeric_limits<float>::max()));
    maxValue = std::clamp(maxValue, static_cast<double>(-std::numeric_limits<float>::max()), static_cast<double>(std::numeric_limits<float>::max()));

    const std::string id(idText, idLength);
    ImportedScriptRecord& record = instance.owner();
    const auto storedIt = record.uiState.find(id);
    double stored = defaultValue;
    if (storedIt != record.uiState.end() && storedIt->is_number()) {
        const double storedNumber = storedIt->get<double>();
        if (std::isfinite(storedNumber)) {
            stored = storedNumber;
        }
    }
    stored = std::clamp(stored, static_cast<double>(-std::numeric_limits<float>::max()), static_cast<double>(std::numeric_limits<float>::max()));
    if (storedIt == record.uiState.end()) {
        UpdateScriptSetting(instance, id, SavedSettingValue{stored});
    }

    float value = static_cast<float>(stored);
    if (instance.isSettingsRenderMode()) {
        ImGui::PushID(id.c_str());
        if (width > 0.0f) {
            ImGui::SetNextItemWidth(width);
        }
        if (ImGui::SliderFloat(label, &value, static_cast<float>(minValue), static_cast<float>(maxValue))) {
            UpdateScriptSetting(instance, id, SavedSettingValue{static_cast<double>(value)});
        }
        ImGui::PopID();
    }

    lua_pushnumber(L, static_cast<lua_Number>(value));
    return 1;
}

int LuaUiTextbox(lua_State* L)
{
    ScriptInstance& instance = RequireInstance(L);
    CheckUiControlBudget(L, instance);
    std::size_t idLength = 0;
    const char* idText = CheckUiId(L, 1, idLength);
    const char* label = luaL_checkstring(L, 2);
    std::size_t defaultLength = 0;
    const char* defaultValue = lua_gettop(L) >= 3 && !lua_isnil(L, 3) ? luaL_checklstring(L, 3, &defaultLength) : "";
    const float width = CheckOptionalNonNegativeFloat(L, 4, 0.0f, "width");
    const float height = CheckOptionalNonNegativeFloat(L, 5, 0.0f, "height");

    const std::string id(idText, idLength);
    ImportedScriptRecord& record = instance.owner();
    const auto storedIt = record.uiState.find(id);
    std::string stored = storedIt != record.uiState.end() && storedIt->is_string()
        ? storedIt->get<std::string>()
        : ClampedString(defaultValue, defaultLength);
    if (stored.size() > kMaxUiStateStringBytes) {
        stored.resize(kMaxUiStateStringBytes);
    }
    if (storedIt == record.uiState.end()) {
        UpdateScriptStringSetting(instance, id, stored);
    }

    auto& buffer = instance.textboxBuffer(id);
    if (buffer[0] == '\0') {
        CopyToBuffer(buffer, stored);
    }

    if (instance.isSettingsRenderMode()) {
        ImGui::PushID(id.c_str());
        if (height > 0.0f) {
            ImVec2 size(width > 0.0f ? width : -1.0f, height);
            if (ImGui::InputTextMultiline(label, buffer.data(), buffer.size(), size)) {
                UpdateScriptStringSetting(instance, id, std::string(buffer.data()));
            }
        } else {
            if (width > 0.0f) {
                ImGui::SetNextItemWidth(width);
            }
            if (ImGui::InputText(label, buffer.data(), buffer.size())) {
                UpdateScriptStringSetting(instance, id, std::string(buffer.data()));
            }
        }
        ImGui::PopID();
    }

    lua_pushstring(L, buffer.data());
    return 1;
}

int LuaUiDynamicTextbox(lua_State* L)
{
    ScriptInstance& instance = RequireInstance(L);
    CheckUiControlBudget(L, instance);
    std::size_t idLength = 0;
    const char* idText = CheckUiId(L, 1, idLength);
    const char* label = luaL_checkstring(L, 2);
    std::size_t defaultLength = 0;
    const char* defaultValue = lua_gettop(L) >= 3 && !lua_isnil(L, 3) ? luaL_checklstring(L, 3, &defaultLength) : "";
    const float width = CheckOptionalNonNegativeFloat(L, 4, 0.0f, "width");
    const float height = CheckOptionalNonNegativeFloat(L, 5, 0.0f, "height");

    const std::string id(idText, idLength);
    ImportedScriptRecord& record = instance.owner();
    const auto storedIt = record.uiState.find(id);
    std::string stored = storedIt != record.uiState.end() && storedIt->is_string()
        ? storedIt->get<std::string>()
        : ClampedString(defaultValue, defaultLength);
    stored = ClampDynamicText(std::move(stored));

    if (storedIt == record.uiState.end()) {
        UpdateScriptStringSetting(instance, id, stored);
    }

    auto& buffer = instance.dynamicTextboxBuffer(id);
    CopyToBuffer(buffer, stored);

    if (instance.isSettingsRenderMode()) {
        ImGui::PushID(id.c_str());
        ImGui::TextWrapped("%s", label);
        const float resolvedHeight = height > 0.0f ? height : ImGui::GetTextLineHeight() * 4.0f;
        ImGui::InputTextMultiline("##dynamic", buffer.data(), buffer.size(),
            ImVec2(width > 0.0f ? width : -1.0f, resolvedHeight),
            ImGuiInputTextFlags_ReadOnly | ImGuiInputTextFlags_NoHorizontalScroll);
        ImGui::PopID();
    }

    lua_pushstring(L, stored.c_str());
    return 1;
}

int LuaUiSetDynamicText(lua_State* L)
{
    ScriptInstance& instance = RequireInstance(L);
    CheckUiControlBudget(L, instance);
    std::size_t idLength = 0;
    const char* idText = CheckUiId(L, 1, idLength);
    std::size_t textLength = 0;
    const char* text = luaL_checklstring(L, 2, &textLength);

    const std::string id(idText, idLength);
    std::string value = ClampDynamicText(ClampedString(text, textLength));
    UpdateScriptStringSetting(instance, id, value);
    lua_pushboolean(L, 1);
    return 1;
}

int LuaUiKeybind(lua_State* L)
{
    ScriptInstance& instance = RequireInstance(L);
    CheckUiControlBudget(L, instance);
    std::size_t idLength = 0;
    const char* idText = CheckUiId(L, 1, idLength);
    const char* label = luaL_checkstring(L, 2);
    const float width = CheckOptionalNonNegativeFloat(L, 4, 0.0f, "width");

    unsigned int defaultValue = kScriptUnboundHotkey;
    if (lua_gettop(L) >= 3) {
        if (lua_isinteger(L, 3)) {
            defaultValue = NormalizeScriptHotkey(static_cast<unsigned int>(CheckLuaInt(L, 3, 0, std::numeric_limits<int>::max(), "default keybind")));
        } else {
            const char* defaultText = lua_tostring(L, 3);
            if (defaultText) {
                if (auto parsed = ParseScriptHotkeyString(defaultText)) {
                    defaultValue = NormalizeScriptHotkey(*parsed);
                } else {
                    defaultValue = kScriptUnboundHotkey;
                }
            }
        }
    }

    const std::string id(idText, idLength);
    ImportedScriptRecord& record = instance.owner();
    const auto storedIt = record.uiState.find(id);
    unsigned int stored = defaultValue;
    if (storedIt != record.uiState.end()) {
        if (storedIt->is_number_integer()) {
            const auto value = storedIt->get<std::int64_t>();
            stored = value >= 0 && value <= static_cast<std::int64_t>(std::numeric_limits<int>::max())
                ? NormalizeScriptHotkey(static_cast<unsigned int>(value))
                : kScriptUnboundHotkey;
        } else if (storedIt->is_number_unsigned()) {
            const auto value = storedIt->get<std::uint64_t>();
            stored = value <= static_cast<std::uint64_t>(std::numeric_limits<int>::max())
                ? NormalizeScriptHotkey(static_cast<unsigned int>(value))
                : kScriptUnboundHotkey;
        }
    }
    if (storedIt == record.uiState.end()) {
        UpdateScriptSetting(instance, id, SavedSettingValue{static_cast<std::int64_t>(stored)});
    }

    unsigned int& keyValue = instance.keybindValue(id, stored);
    if (keyValue != stored) {
        keyValue = stored;
    }

    if (instance.isSettingsRenderMode()) {
        ImGui::PushID(id.c_str());
        if (width > 0.0f) {
            ImGui::PushTextWrapPos(ImGui::GetCursorPosX() + width);
            ImGui::TextWrapped("%s", label);
            ImGui::PopTextWrapPos();
        } else {
            ImGui::TextWrapped("%s", label);
        }
        const float humanWidth = width > 0.0f ? width * 0.55f : 170.0f;
        const float hexWidth = width > 0.0f ? width * 0.35f : 130.0f;
        DrawKeyBindControlShared("##ScriptKeybind", keyValue, -1, humanWidth, hexWidth, true);
        ImGui::PopID();
    }

    const unsigned int normalized = NormalizeScriptHotkey(keyValue);
    if (normalized != stored) {
        UpdateScriptSetting(instance, id, SavedSettingValue{static_cast<std::int64_t>(normalized)});
        keyValue = normalized;
    }

    lua_pushinteger(L, static_cast<lua_Integer>(keyValue));
    return 1;
}

void RegisterUiTable(lua_State* L)
{
    lua_newtable(L);
    lua_pushcfunction(L, LuaUiText);
    lua_setfield(L, -2, "text");
    lua_pushcfunction(L, LuaUiSeparator);
    lua_setfield(L, -2, "separator");
    lua_pushcfunction(L, LuaUiCheckbox);
    lua_setfield(L, -2, "checkbox");
    lua_pushcfunction(L, LuaUiSliderInt);
    lua_setfield(L, -2, "sliderInt");
    lua_pushcfunction(L, LuaUiSliderFloat);
    lua_setfield(L, -2, "sliderFloat");
    lua_pushcfunction(L, LuaUiTextbox);
    lua_setfield(L, -2, "textbox");
    lua_pushcfunction(L, LuaUiDynamicTextbox);
    lua_setfield(L, -2, "dynamicTextbox");
    lua_pushcfunction(L, LuaUiSetDynamicText);
    lua_setfield(L, -2, "setDynamicText");
    lua_pushcfunction(L, LuaUiKeybind);
    lua_setfield(L, -2, "keybind");
    lua_setglobal(L, "ui");
}

void Register(lua_State* L, const char* name, lua_CFunction function)
{
    lua_pushcfunction(L, function);
    lua_setglobal(L, name);
}

} // namespace

void RegisterScriptApi(lua_State* L)
{
    Register(L, "log", LuaLog);
    Register(L, "nowMicros", LuaNowMicros);
    Register(L, "sleep", LuaSleep);
    Register(L, "pressKey", LuaPressKey);
    Register(L, "holdKey", LuaHoldKey);
    Register(L, "releaseKey", LuaReleaseKey);
    Register(L, "isKeyPressed", LuaIsKeyPressed);
    Register(L, "isHotkeyPressed", LuaIsHotkeyPressed);
    Register(L, "typeText", LuaTypeText);
    Register(L, "moveMouse", LuaMoveMouse);
    Register(L, "mouseWheel", LuaMouseWheel);
    Register(L, "freeze", LuaFreeze);
    Register(L, "lagSwitch", LuaLagSwitch);
    Register(L, "getPlatform", LuaGetPlatform);
    Register(L, "getSMUVersion", LuaGetSMUVersion);
    Register(L, "getSavedValue", LuaGetSavedValue);
    RegisterUiTable(L);
    Register(L, "robloxFreeze", LuaFreeze);
    Register(L, "roblox_freeze", LuaFreeze);
    Register(L, "lagswitch", LuaLagSwitch);
}

} // namespace smu::app

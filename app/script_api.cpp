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
#include "../platform/network_backend.h"
#include "../platform/text_input_backend.h"

#include <algorithm>
#include <cstdio>
#include <array>
#include <chrono>
#include <cctype>
#include <cstring>
#include <cstdint>
#include <cstdlib>
#include <cmath>
#include <limits>
#include <memory>
#include <type_traits>
#include <optional>
#include <unordered_map>
#include <unordered_set>
#include <variant>
#include <vector>
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

// Allow effectively infinite sleeps; the scheduler clamps to time_point::max().
constexpr std::int64_t kMaxSingleSleepMs = std::numeric_limits<std::int64_t>::max();
constexpr int kMaxInputDelayMs = 300000;
constexpr float kMaxUiDimension = 10000.0f;
constexpr std::size_t kMaxLogMessageBytes = 4096;
constexpr std::size_t kMaxTypeTextBytes = 4096;
constexpr const char* kLogTruncateSuffix = "...(truncated)";
constexpr const char* kRegistryMoveDegreesSettingsKey = "smu.moveDegreesSettings";
constexpr const char* kMouseMotionModeRaw = "raw";
constexpr const char* kMouseMotionModeAbsolute = "absolute";

const char* MouseMotionModeToString(ScriptInstance::MouseMotionMode mode)
{
    switch (mode) {
    case ScriptInstance::MouseMotionMode::Absolute:
        return kMouseMotionModeAbsolute;
    case ScriptInstance::MouseMotionMode::Raw:
    default:
        return kMouseMotionModeRaw;
    }
}

bool ParseMouseMotionMode(const char* value, ScriptInstance::MouseMotionMode& mode)
{
    if (!value) {
        return false;
    }

    if (std::strcmp(value, kMouseMotionModeRaw) == 0) {
        mode = ScriptInstance::MouseMotionMode::Raw;
        return true;
    }
    if (std::strcmp(value, kMouseMotionModeAbsolute) == 0 ||
        std::strcmp(value, "abs") == 0) {
        mode = ScriptInstance::MouseMotionMode::Absolute;
        return true;
    }
    return false;
}

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

double CheckLuaFiniteNumber(lua_State* L, int index, const char* name)
{
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

std::int64_t CheckLuaInt64Clamped(lua_State* L, int index, std::int64_t minValue, std::int64_t maxValue, const char* name)
{
    int isInteger = 0;
    const lua_Integer value = lua_tointegerx(L, index, &isInteger);
    if (!isInteger) {
        return static_cast<std::int64_t>(luaL_error(L, "%s must be an integer", name));
    }
    if (value < static_cast<lua_Integer>(minValue)) {
        return minValue;
    }
    if (value > static_cast<lua_Integer>(maxValue)) {
        return maxValue;
    }
    return static_cast<std::int64_t>(value);
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

void CheckUiIdBudget(lua_State* L, ScriptInstance& instance, const std::string& id)
{
    if (!instance.tryRegisterUiId(id)) {
        luaL_error(L, "script settings created too many unique UI IDs");
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

bool IsMouseRelatedKey(smu::core::KeyCode key)
{
    switch (key) {
    case smu::core::SMU_VK_LBUTTON:
    case smu::core::SMU_VK_RBUTTON:
    case smu::core::SMU_VK_MBUTTON:
    case smu::core::SMU_VK_XBUTTON1:
    case smu::core::SMU_VK_XBUTTON2:
    case smu::core::SMU_VK_MOUSE_WHEEL_UP:
    case smu::core::SMU_VK_MOUSE_WHEEL_DOWN:
        return true;
    default:
        return false;
    }
}

enum class PixelOutputFormat {
    Hex,
    Rgb,
};

PixelOutputFormat CheckPixelOutputFormat(lua_State* L, int index)
{
    if (lua_gettop(L) < index || lua_isnil(L, index)) {
        return PixelOutputFormat::Hex;
    }

    const char* format = luaL_checkstring(L, index);
    if (format && std::strcmp(format, "hex") == 0) {
        return PixelOutputFormat::Hex;
    }
    if (format && std::strcmp(format, "rgb") == 0) {
        return PixelOutputFormat::Rgb;
    }
    return static_cast<PixelOutputFormat>(luaL_error(L, "pixel format must be \"hex\" or \"rgb\""));
}

void PushPixelColor(lua_State* L, const smu::platform::PixelColor& color, PixelOutputFormat format)
{
    if (format == PixelOutputFormat::Rgb) {
        lua_createtable(L, 0, 3);
        lua_pushinteger(L, color.r);
        lua_setfield(L, -2, "r");
        lua_pushinteger(L, color.g);
        lua_setfield(L, -2, "g");
        lua_pushinteger(L, color.b);
        lua_setfield(L, -2, "b");
        return;
    }

    char buffer[8] = {};
    std::snprintf(buffer, sizeof(buffer), "#%02X%02X%02X",
        static_cast<unsigned int>(color.r),
        static_cast<unsigned int>(color.g),
        static_cast<unsigned int>(color.b));
    lua_pushstring(L, buffer);
}

void PushPixelRect(lua_State* L, const std::vector<std::vector<smu::platform::PixelColor>>& pixels, PixelOutputFormat format)
{
    lua_createtable(L, static_cast<int>(pixels.size()), 0);
    int rowIndex = 1;
    for (const auto& row : pixels) {
        lua_createtable(L, static_cast<int>(row.size()), 0);
        int colIndex = 1;
        for (const auto& color : row) {
            PushPixelColor(L, color, format);
            lua_rawseti(L, -2, colIndex++);
        }
        lua_rawseti(L, -2, rowIndex++);
    }
}

int LuaPressKeyImpl(lua_State* L, bool mouseOnly, const char* functionName)
{
    ScriptInstance& instance = RequireInstance(L);
    if (instance.isSettingsRenderMode()) {
        return luaL_error(L, "%s is not available while rendering script settings", functionName);
    }
    instance.throwStopIfRequested(L);
    const smu::core::KeyCode key = CheckKey(L, 1);
    if (mouseOnly && !IsMouseRelatedKey(key)) {
        return luaL_error(L, "%s only accepts mouse buttons or mouse wheel actions", functionName);
    }
    const int delay = lua_gettop(L) >= 2 ? CheckLuaIntClamped(L, 2, 0, kMaxInputDelayMs, "delay") : 50;
    auto backend = smu::platform::GetInputBackend();
    if (!backend) {
        LogWarning("Imported script tried to press a key, but the input backend is not available.");
        return 0;
    }
    backend->holdKey(key);
    if (!instance.waitFor(std::chrono::milliseconds(delay))) {
        backend->releaseKey(key);
        instance.throwStopIfRequested(L);
    }
    backend->releaseKey(key);
    return 0;
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

bool IsSettingsRenderMode(lua_State* L);

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

const char* LagSwitchTargetModeToString(smu::platform::LagSwitchTargetMode mode)
{
    switch (mode) {
    case smu::platform::LagSwitchTargetMode::All:
        return "all";
    case smu::platform::LagSwitchTargetMode::Custom:
        return "custom";
    case smu::platform::LagSwitchTargetMode::Roblox:
    default:
        return "roblox";
    }
}

bool IsValidIpv4Address(const char* text, std::size_t length)
{
    if (!text || length == 0 || length > 15 || std::memchr(text, '\0', length)) {
        return false;
    }

    int partCount = 0;
    std::size_t index = 0;
    while (index < length) {
        if (partCount >= 4 || !std::isdigit(static_cast<unsigned char>(text[index]))) {
            return false;
        }

        int value = 0;
        int digitCount = 0;
        while (index < length && std::isdigit(static_cast<unsigned char>(text[index]))) {
            value = value * 10 + (text[index] - '0');
            if (value > 255 || ++digitCount > 3) {
                return false;
            }
            ++index;
        }

        ++partCount;
        if (partCount == 4) {
            return index == length;
        }
        if (index >= length || text[index] != '.') {
            return false;
        }
        ++index;
    }

    return false;
}

void ValidateLagSwitchConfigKeys(lua_State* L, int index)
{
    static const std::unordered_set<std::string> kAllowedKeys = {
        "hardBlockInbound",
        "hardBlockOutbound",
        "fakeLag",
        "fakeLagInbound",
        "fakeLagOutbound",
        "fakeLagDelayMs",
        "targetMode",
        "useUdp",
        "useTcp",
        "preventDisconnect",
        "autoUnblock",
        "maxDurationSeconds",
        "unblockDurationMs",
        "remoteIps",
        "remotePorts",
        "includeRobloxDynamicIps"
    };

    const int tableIndex = lua_absindex(L, index);
    lua_pushnil(L);
    while (lua_next(L, tableIndex) != 0) {
        if (!lua_isstring(L, -2)) {
            luaL_error(L, "lag-switch config keys must be strings");
        }
        const char* key = lua_tostring(L, -2);
        if (!key || kAllowedKeys.find(key) == kAllowedKeys.end()) {
            luaL_error(L, "unknown lag-switch config key: %s", key ? key : "<null>");
        }
        lua_pop(L, 1);
    }
}

bool CheckOptionalLagSwitchBool(lua_State* L, int tableIndex, const char* name, bool currentValue)
{
    lua_getfield(L, tableIndex, name);
    if (lua_isnil(L, -1)) {
        lua_pop(L, 1);
        return currentValue;
    }
    if (!lua_isboolean(L, -1)) {
        luaL_error(L, "%s must be a boolean", name);
    }
    const bool value = lua_toboolean(L, -1) != 0;
    lua_pop(L, 1);
    return value;
}

int CheckOptionalLagSwitchInt(lua_State* L, int tableIndex, const char* name, int currentValue, int minValue, int maxValue)
{
    lua_getfield(L, tableIndex, name);
    if (lua_isnil(L, -1)) {
        lua_pop(L, 1);
        return currentValue;
    }
    const int value = CheckLuaInt(L, -1, minValue, maxValue, name);
    lua_pop(L, 1);
    return value;
}

float CheckOptionalLagSwitchFloat(lua_State* L, int tableIndex, const char* name, float currentValue, float minValue, float maxValue)
{
    lua_getfield(L, tableIndex, name);
    if (lua_isnil(L, -1)) {
        lua_pop(L, 1);
        return currentValue;
    }
    const double value = CheckLuaFiniteNumber(L, -1, name);
    if (value < minValue || value > maxValue) {
        luaL_error(L, "%s is outside the allowed range", name);
    }
    lua_pop(L, 1);
    return static_cast<float>(value);
}

std::vector<std::string> CheckOptionalLagSwitchIpList(lua_State* L, int tableIndex, const char* name, const std::vector<std::string>& currentValue)
{
    lua_getfield(L, tableIndex, name);
    if (lua_isnil(L, -1)) {
        lua_pop(L, 1);
        return currentValue;
    }
    luaL_checktype(L, -1, LUA_TTABLE);

    std::vector<std::string> values;
    const int listIndex = lua_absindex(L, -1);
    lua_pushnil(L);
    while (lua_next(L, listIndex) != 0) {
        if (values.size() >= 64) {
            luaL_error(L, "%s may contain at most 64 entries", name);
        }
        std::size_t length = 0;
        const char* value = luaL_checklstring(L, -1, &length);
        if (!IsValidIpv4Address(value, length)) {
            luaL_error(L, "%s contains an invalid IPv4 address", name);
        }
        values.emplace_back(value, length);
        lua_pop(L, 1);
    }

    lua_pop(L, 1);
    return values;
}

std::vector<int> CheckOptionalLagSwitchPortList(lua_State* L, int tableIndex, const char* name, const std::vector<int>& currentValue)
{
    lua_getfield(L, tableIndex, name);
    if (lua_isnil(L, -1)) {
        lua_pop(L, 1);
        return currentValue;
    }
    luaL_checktype(L, -1, LUA_TTABLE);

    std::vector<int> values;
    const int listIndex = lua_absindex(L, -1);
    lua_pushnil(L);
    while (lua_next(L, listIndex) != 0) {
        if (values.size() >= 64) {
            luaL_error(L, "%s may contain at most 64 entries", name);
        }
        values.push_back(CheckLuaInt(L, -1, 1, 65535, name));
        lua_pop(L, 1);
    }

    lua_pop(L, 1);
    return values;
}

smu::platform::LagSwitchTargetMode CheckOptionalLagSwitchTargetMode(
    lua_State* L,
    int tableIndex,
    const char* name,
    smu::platform::LagSwitchTargetMode currentValue)
{
    lua_getfield(L, tableIndex, name);
    if (lua_isnil(L, -1)) {
        lua_pop(L, 1);
        return currentValue;
    }

    const char* text = luaL_checkstring(L, -1);
    smu::platform::LagSwitchTargetMode mode = currentValue;
    if (std::strcmp(text, "roblox") == 0) {
        mode = smu::platform::LagSwitchTargetMode::Roblox;
    } else if (std::strcmp(text, "all") == 0) {
        mode = smu::platform::LagSwitchTargetMode::All;
    } else if (std::strcmp(text, "custom") == 0) {
        mode = smu::platform::LagSwitchTargetMode::Custom;
    } else {
        luaL_error(L, "%s must be \"roblox\", \"all\", or \"custom\"", name);
    }

    lua_pop(L, 1);
    return mode;
}

smu::platform::LagSwitchConfig CheckLagSwitchConfigTable(lua_State* L, int index, smu::platform::LagSwitchConfig config)
{
    luaL_checktype(L, index, LUA_TTABLE);
    const int tableIndex = lua_absindex(L, index);
    ValidateLagSwitchConfigKeys(L, tableIndex);

    config.inboundHardBlock = CheckOptionalLagSwitchBool(L, tableIndex, "hardBlockInbound", config.inboundHardBlock);
    config.outboundHardBlock = CheckOptionalLagSwitchBool(L, tableIndex, "hardBlockOutbound", config.outboundHardBlock);
    config.fakeLagEnabled = CheckOptionalLagSwitchBool(L, tableIndex, "fakeLag", config.fakeLagEnabled);
    config.inboundFakeLag = CheckOptionalLagSwitchBool(L, tableIndex, "fakeLagInbound", config.inboundFakeLag);
    config.outboundFakeLag = CheckOptionalLagSwitchBool(L, tableIndex, "fakeLagOutbound", config.outboundFakeLag);
    config.fakeLagDelayMs = CheckOptionalLagSwitchInt(L, tableIndex, "fakeLagDelayMs", config.fakeLagDelayMs, 0, 300000);
    config.targetMode = CheckOptionalLagSwitchTargetMode(L, tableIndex, "targetMode", config.targetMode);
    config.useUdp = CheckOptionalLagSwitchBool(L, tableIndex, "useUdp", config.useUdp);
    config.useTcp = CheckOptionalLagSwitchBool(L, tableIndex, "useTcp", config.useTcp);
    config.preventDisconnect = CheckOptionalLagSwitchBool(L, tableIndex, "preventDisconnect", config.preventDisconnect);
    config.autoUnblock = CheckOptionalLagSwitchBool(L, tableIndex, "autoUnblock", config.autoUnblock);
    config.maxDurationSeconds = CheckOptionalLagSwitchFloat(L, tableIndex, "maxDurationSeconds", config.maxDurationSeconds, 0.0f, 3600.0f);
    config.unblockDurationMs = CheckOptionalLagSwitchInt(L, tableIndex, "unblockDurationMs", config.unblockDurationMs, 0, 300000);
    config.remoteIps = CheckOptionalLagSwitchIpList(L, tableIndex, "remoteIps", config.remoteIps);
    config.remotePorts = CheckOptionalLagSwitchPortList(L, tableIndex, "remotePorts", config.remotePorts);
    config.includeRobloxDynamicIps = CheckOptionalLagSwitchBool(L, tableIndex, "includeRobloxDynamicIps", config.includeRobloxDynamicIps);
    config.targetRobloxOnly = config.targetMode == smu::platform::LagSwitchTargetMode::Roblox;
    return config;
}

void PushStringList(lua_State* L, const std::vector<std::string>& values)
{
    lua_createtable(L, static_cast<int>(values.size()), 0);
    int index = 1;
    for (const std::string& value : values) {
        lua_pushlstring(L, value.c_str(), value.size());
        lua_rawseti(L, -2, index++);
    }
}

void PushIntList(lua_State* L, const std::vector<int>& values)
{
    lua_createtable(L, static_cast<int>(values.size()), 0);
    int index = 1;
    for (int value : values) {
        lua_pushinteger(L, value);
        lua_rawseti(L, -2, index++);
    }
}

void PushLagSwitchConfig(lua_State* L, const smu::platform::LagSwitchConfig& config)
{
    lua_createtable(L, 0, 16);
    lua_pushboolean(L, config.inboundHardBlock);
    lua_setfield(L, -2, "hardBlockInbound");
    lua_pushboolean(L, config.outboundHardBlock);
    lua_setfield(L, -2, "hardBlockOutbound");
    lua_pushboolean(L, config.fakeLagEnabled);
    lua_setfield(L, -2, "fakeLag");
    lua_pushboolean(L, config.inboundFakeLag);
    lua_setfield(L, -2, "fakeLagInbound");
    lua_pushboolean(L, config.outboundFakeLag);
    lua_setfield(L, -2, "fakeLagOutbound");
    lua_pushinteger(L, config.fakeLagDelayMs);
    lua_setfield(L, -2, "fakeLagDelayMs");
    lua_pushstring(L, LagSwitchTargetModeToString(config.targetMode));
    lua_setfield(L, -2, "targetMode");
    lua_pushboolean(L, config.useUdp);
    lua_setfield(L, -2, "useUdp");
    lua_pushboolean(L, config.useTcp);
    lua_setfield(L, -2, "useTcp");
    lua_pushboolean(L, config.preventDisconnect);
    lua_setfield(L, -2, "preventDisconnect");
    lua_pushboolean(L, config.autoUnblock);
    lua_setfield(L, -2, "autoUnblock");
    lua_pushnumber(L, config.maxDurationSeconds);
    lua_setfield(L, -2, "maxDurationSeconds");
    lua_pushinteger(L, config.unblockDurationMs);
    lua_setfield(L, -2, "unblockDurationMs");
    lua_pushboolean(L, config.includeRobloxDynamicIps);
    lua_setfield(L, -2, "includeRobloxDynamicIps");
    PushStringList(L, config.remoteIps);
    lua_setfield(L, -2, "remoteIps");
    PushIntList(L, config.remotePorts);
    lua_setfield(L, -2, "remotePorts");
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

std::optional<bool> TryGetSavedBool(const char* name)
{
    const auto value = TryGetSavedSettingValue(name);
    if (!value) {
        return std::nullopt;
    }
    if (const bool* stored = std::get_if<bool>(&*value)) {
        return *stored;
    }
    return std::nullopt;
}

std::optional<std::string> TryGetSavedString(const char* name)
{
    const auto value = TryGetSavedSettingValue(name);
    if (!value) {
        return std::nullopt;
    }
    if (const std::string* stored = std::get_if<std::string>(&*value)) {
        return *stored;
    }
    return std::nullopt;
}

std::optional<double> ParseFiniteDouble(std::string text)
{
    const auto begin = text.find_first_not_of(" \t\r\n");
    if (begin == std::string::npos) {
        return std::nullopt;
    }
    const auto end = text.find_last_not_of(" \t\r\n");
    text = text.substr(begin, end - begin + 1);

    char* parseEnd = nullptr;
    const double value = std::strtod(text.c_str(), &parseEnd);
    if (parseEnd == text.c_str()) {
        return std::nullopt;
    }
    while (*parseEnd != '\0' && std::isspace(static_cast<unsigned char>(*parseEnd))) {
        ++parseEnd;
    }
    if (*parseEnd != '\0' || !std::isfinite(value)) {
        return std::nullopt;
    }
    return value;
}

void CacheMoveDegreesSettings(lua_State* L)
{
    std::string error;
    double pixelsPerDegree = 0.0;

    const auto camfixEnabled = TryGetSavedBool("camfixtoggle");
    if (!camfixEnabled) {
        error = "moveDegrees requires the saved camfixtoggle setting.";
    }

    const auto savedSensitivity = TryGetSavedString("RobloxSensValue");
    if (error.empty() && !savedSensitivity) {
        error = "moveDegrees requires the saved RobloxSensValue setting.";
    }

    const auto sensitivity = savedSensitivity ? ParseFiniteDouble(*savedSensitivity) : std::nullopt;
    if (error.empty() && (!sensitivity || *sensitivity <= 0.0)) {
        error = "moveDegrees requires RobloxSensValue to be a finite number greater than 0.";
    }

    if (error.empty()) {
        const double baseValue = *camfixEnabled ? 1000.0 : 720.0;
        pixelsPerDegree = baseValue / (360.0 * *sensitivity);
    }

    lua_newtable(L);
    lua_pushboolean(L, error.empty());
    lua_setfield(L, -2, "valid");
    lua_pushnumber(L, static_cast<lua_Number>(pixelsPerDegree));
    lua_setfield(L, -2, "pixelsPerDegree");
    lua_pushlstring(L, error.c_str(), error.size());
    lua_setfield(L, -2, "error");
    lua_setfield(L, LUA_REGISTRYINDEX, kRegistryMoveDegreesSettingsKey);
}

bool GetMoveDegreesSettings(lua_State* L, double& pixelsPerDegree, std::string& error)
{
    lua_getfield(L, LUA_REGISTRYINDEX, kRegistryMoveDegreesSettingsKey);
    if (!lua_istable(L, -1)) {
        lua_pop(L, 1);
        CacheMoveDegreesSettings(L);
        lua_getfield(L, LUA_REGISTRYINDEX, kRegistryMoveDegreesSettingsKey);
    }

    lua_getfield(L, -1, "valid");
    const bool valid = lua_toboolean(L, -1) != 0;
    lua_pop(L, 1);

    lua_getfield(L, -1, "pixelsPerDegree");
    pixelsPerDegree = lua_tonumber(L, -1);
    lua_pop(L, 1);

    lua_getfield(L, -1, "error");
    std::size_t errorLength = 0;
    const char* errorText = lua_tolstring(L, -1, &errorLength);
    error.assign(errorText ? errorText : "", errorLength);
    lua_pop(L, 1);

    lua_pop(L, 1);
    return valid;
}

int RoundMouseDelta(lua_State* L, double pixels, const char* name)
{
    if (!std::isfinite(pixels)) {
        return static_cast<int>(luaL_error(L, "%s conversion overflowed", name));
    }

    const double rounded = std::round(pixels);
    if (rounded < static_cast<double>(std::numeric_limits<int>::min()) ||
        rounded > static_cast<double>(std::numeric_limits<int>::max())) {
        return static_cast<int>(luaL_error(L, "%s conversion is outside the allowed range", name));
    }
    return static_cast<int>(rounded);
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
    std::lock_guard<std::mutex> uiLock(record.uiStateMutex);
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
    std::size_t messageLength = 0;
    const char* message = luaL_checklstring(L, 1, &messageLength);
    if (!message) {
        LogInfo("[script] ");
        return 0;
    }
    if (messageLength > kMaxLogMessageBytes) {
        const std::size_t suffixLength = std::strlen(kLogTruncateSuffix);
        const std::size_t prefixLength = kMaxLogMessageBytes > suffixLength ? kMaxLogMessageBytes - suffixLength : 0;
        std::string truncated(message, prefixLength);
        truncated += kLogTruncateSuffix;
        LogInfo(std::string("[script] ") + truncated);
        return 0;
    }
    LogInfo(std::string("[script] ") + std::string(message, messageLength));
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
    instance.throwStopIfRequested(L);
    const std::int64_t ms = CheckLuaInt64Clamped(L, 1, 0, kMaxSingleSleepMs, "sleep duration");
    if (lua_isyieldable(L)) {
        instance.pauseExecutionBudget();
        instance.scheduleCoroutineSleep(L, ms);
        return lua_yield(L, 0);
    }
    instance.sleepWithDeadline(ms);
    return 0;
}

int LuaPressKey(lua_State* L)
{
    return LuaPressKeyImpl(L, false, "pressKey");
}

int LuaClickMouse(lua_State* L)
{
    return LuaPressKeyImpl(L, true, "clickMouse");
}

int LuaIsCancelled(lua_State* L)
{
    lua_pushboolean(L, RequireInstance(L).isStopRequested());
    return 1;
}

int LuaSleepUntilCancelled(lua_State* L)
{
    if (IsSettingsRenderMode(L)) {
        return luaL_error(L, "sleepUntilCancelled is not available while rendering script settings");
    }
    ScriptInstance& instance = RequireInstance(L);
    const std::int64_t ms = CheckLuaInt64Clamped(L, 1, 0, kMaxSingleSleepMs, "sleep duration");
    if (instance.isStopRequested()) {
        lua_pushboolean(L, 1);
        return 1;
    }
    const bool completed = instance.waitFor(std::chrono::milliseconds(ms));
    lua_pushboolean(L, completed ? 0 : 1);
    return 1;
}

int LuaThrowIfCancelled(lua_State* L)
{
    ScriptInstance& instance = RequireInstance(L);
    instance.throwStopIfRequested(L);
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

int LuaGetScriptHotkey(lua_State* L)
{
    const unsigned int hotkey = RequireInstance(L).owner().hotkey.load(std::memory_order_acquire);
    if (!IsScriptHotkeyBound(hotkey)) {
        lua_pushnil(L);
        return 1;
    }

    lua_pushinteger(L, static_cast<lua_Integer>(hotkey));
    return 1;
}

int LuaTypeText(lua_State* L)
{
    if (IsSettingsRenderMode(L)) {
        return luaL_error(L, "typeText is not available while rendering script settings");
    }
    ScriptInstance& instance = RequireInstance(L);
    instance.throwStopIfRequested(L);
    std::size_t textLength = 0;
    const char* text = luaL_checklstring(L, 1, &textLength);
    if (textLength > kMaxTypeTextBytes) {
        return luaL_error(L, "typeText text is too long (max %zu bytes)", kMaxTypeTextBytes);
    }
    const int delay = lua_gettop(L) >= 2 ? CheckLuaIntClamped(L, 2, 0, kMaxInputDelayMs, "delay") : 30;
    auto backend = smu::platform::GetInputBackend();
    if (!backend) {
        LogWarning("Imported script tried to type text, but the input backend is not available.");
        return 0;
    }
    const int safeDelayMs = std::max(0, delay);
    for (std::size_t i = 0; i < textLength; ++i) {
        instance.throwStopIfRequested(L);
        const smu::platform::KeyAction action = smu::platform::charToKeyAction(text[i]);
        if (!action.valid) {
            continue;
        }
        if (action.needsShift) {
            // holdKeyChord handles modifier combinations; we use it for single modifiers too.
            backend->holdKeyChord(smu::core::SMU_VK_SHIFT);
        }
        backend->holdKeyChord(action.key);
        if (!instance.waitFor(std::chrono::milliseconds(safeDelayMs))) {
            backend->releaseKeyChord(action.key);
            if (action.needsShift) {
                backend->releaseKeyChord(smu::core::SMU_VK_SHIFT);
            }
            instance.throwStopIfRequested(L);
        }
        backend->releaseKeyChord(action.key);
        if (action.needsShift) {
            backend->releaseKeyChord(smu::core::SMU_VK_SHIFT);
        }
    }
    return 0;
}

int LuaMoveMouse(lua_State* L)
{
    if (IsSettingsRenderMode(L)) {
        return luaL_error(L, "moveMouse is not available while rendering script settings");
    }
    const int dx = CheckLuaInt(L, 1, std::numeric_limits<int>::min(), std::numeric_limits<int>::max(), "dx");
    const int dy = CheckLuaInt(L, 2, std::numeric_limits<int>::min(), std::numeric_limits<int>::max(), "dy");

    ScriptInstance& instance = RequireInstance(L);
    const ScriptInstance::MouseMotionMode motionMode = instance.mouseMotionMode();
    if (motionMode == ScriptInstance::MouseMotionMode::Raw) {
        MoveMouse(dx, dy);
    } else {
        std::string error;
        if (!MoveMouseAbsoluteDelta(dx, dy, &error)) {
            if (error.empty()) {
                error = "unknown failure";
            }
            return luaL_error(L, "moveMouse failed: %s", error.c_str());
        }
    }
    return 0;
}

int LuaMoveMouseAbs(lua_State* L)
{
    if (IsSettingsRenderMode(L)) {
        return luaL_error(L, "moveMouseAbs is not available while rendering script settings");
    }

    const double x = CheckLuaFiniteNumber(L, 1, "x");
    const double y = CheckLuaFiniteNumber(L, 2, "y");
    const char* modeText = "pixels";
    if (lua_gettop(L) >= 3 && !lua_isnil(L, 3)) {
        modeText = luaL_checkstring(L, 3);
    }

    std::string error;
    ScriptInstance& instance = RequireInstance(L);
    if (!MoveMouseAbs(x, y, modeText ? modeText : "pixels",
            instance.mouseMotionMode() != ScriptInstance::MouseMotionMode::Raw,
            &error)) {
        if (error.empty()) {
            error = "unknown failure";
        }
        return luaL_error(L, "moveMouseAbs failed: %s", error.c_str());
    }
    return 0;
}


int LuaGetPixelColor(lua_State* L)
{
    if (IsSettingsRenderMode(L)) {
        return luaL_error(L, "getPixelColor is not available while rendering script settings");
    }

    const double x = CheckLuaFiniteNumber(L, 1, "x");
    const double y = CheckLuaFiniteNumber(L, 2, "y");
    const char* modeText = "pixels";
    if (lua_gettop(L) >= 3 && !lua_isnil(L, 3)) {
        modeText = luaL_checkstring(L, 3);
    }
    const PixelOutputFormat format = CheckPixelOutputFormat(L, 4);

    std::string error;
    const auto color = GetPixelColor(x, y, modeText ? modeText : "pixels", &error);
    if (!color) {
        if (error.empty()) {
            error = "unknown failure";
        }
        return luaL_error(L, "getPixelColor failed: %s", error.c_str());
    }

    PushPixelColor(L, *color, format);
    return 1;
}

int LuaSetMouseMotionMode(lua_State* L)
{
    if (IsSettingsRenderMode(L)) {
        return luaL_error(L, "setMouseMotionMode is not available while rendering script settings");
    }

    ScriptInstance& instance = RequireInstance(L);
    const char* modeText = luaL_checkstring(L, 1);
    ScriptInstance::MouseMotionMode mode = ScriptInstance::MouseMotionMode::Raw;
    if (!ParseMouseMotionMode(modeText, mode)) {
        return luaL_error(L, "mouse motion mode must be one of: raw, absolute");
    }

    instance.setMouseMotionMode(mode);
    lua_pushstring(L, MouseMotionModeToString(mode));
    return 1;
}

int LuaGetMouseMotionMode(lua_State* L)
{
    ScriptInstance& instance = RequireInstance(L);
    lua_pushstring(L, MouseMotionModeToString(instance.mouseMotionMode()));
    return 1;
}

int LuaGetPixelRect(lua_State* L)
{
    if (IsSettingsRenderMode(L)) {
        return luaL_error(L, "getPixelRect is not available while rendering script settings");
    }

    const double x1 = CheckLuaFiniteNumber(L, 1, "x1");
    const double y1 = CheckLuaFiniteNumber(L, 2, "y1");
    const double x2 = CheckLuaFiniteNumber(L, 3, "x2");
    const double y2 = CheckLuaFiniteNumber(L, 4, "y2");
    const char* modeText = "pixels";
    if (lua_gettop(L) >= 5 && !lua_isnil(L, 5)) {
        modeText = luaL_checkstring(L, 5);
    }
    const PixelOutputFormat format = CheckPixelOutputFormat(L, 6);

    std::string error;
    const auto pixels = GetPixelRect(x1, y1, x2, y2, modeText ? modeText : "pixels", &error);
    if (!pixels) {
        if (error.empty()) {
            error = "unknown failure";
        }
        return luaL_error(L, "getPixelRect failed: %s", error.c_str());
    }

    PushPixelRect(L, *pixels, format);
    return 1;
}

int LuaMoveDegrees(lua_State* L)
{
    if (IsSettingsRenderMode(L)) {
        return luaL_error(L, "moveDegrees is not available while rendering script settings");
    }

    const double dxDegrees = CheckLuaFiniteNumber(L, 1, "dx");
    const double dyDegrees = CheckLuaFiniteNumber(L, 2, "dy");

    double pixelsPerDegree = 0.0;
    std::string error;
    if (!GetMoveDegreesSettings(L, pixelsPerDegree, error)) {
        return luaL_error(L, "%s", error.c_str());
    }

    const int dxPixels = RoundMouseDelta(L, dxDegrees * pixelsPerDegree, "dx");
    // Positive degree values move upward, which is negative screen-space Y.
    const int dyPixels = RoundMouseDelta(L, -dyDegrees * pixelsPerDegree, "dy");
    ScriptInstance& instance = RequireInstance(L);
    const ScriptInstance::MouseMotionMode motionMode = instance.mouseMotionMode();
    if (motionMode == ScriptInstance::MouseMotionMode::Raw) {
        MoveMouse(dxPixels, dyPixels);
    } else {
        std::string absoluteError;
        if (!MoveMouseAbsoluteDelta(dxPixels, dyPixels, &absoluteError)) {
            if (absoluteError.empty()) {
                absoluteError = "unknown failure";
            }
            return luaL_error(L, "moveDegrees failed: %s", absoluteError.c_str());
        }
    }
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
    if (lua_gettop(L) >= 2 && !lua_isnil(L, 2)) {
        const smu::platform::LagSwitchConfig config = CheckLagSwitchConfigTable(L, 2, instance.lagSwitchConfig());
        instance.setLagSwitchConfig(config);
    }
    instance.setLagSwitch(lua_toboolean(L, 1) != 0);
    return 0;
}

int LuaGetLagSwitchConfig(lua_State* L)
{
    if (IsSettingsRenderMode(L)) {
        return luaL_error(L, "getLagSwitchConfig is not available while rendering script settings");
    }
    ScriptInstance& instance = RequireInstance(L);
    PushLagSwitchConfig(L, instance.lagSwitchConfig());
    return 1;
}

int LuaSetLagSwitchConfig(lua_State* L)
{
    if (IsSettingsRenderMode(L)) {
        return luaL_error(L, "setLagSwitchConfig is not available while rendering script settings");
    }
    ScriptInstance& instance = RequireInstance(L);
    const smu::platform::LagSwitchConfig config = CheckLagSwitchConfigTable(L, 1, instance.lagSwitchConfig());
    instance.setLagSwitchConfig(config);
    return 0;
}

int LuaClearLagSwitchConfig(lua_State* L)
{
    if (IsSettingsRenderMode(L)) {
        return luaL_error(L, "clearLagSwitchConfig is not available while rendering script settings");
    }
    ScriptInstance& instance = RequireInstance(L);
    instance.clearLagSwitchConfig();
    return 0;
}

int LuaGetLagSwitchStatus(lua_State* L)
{
    if (IsSettingsRenderMode(L)) {
        return luaL_error(L, "getLagSwitchStatus is not available while rendering script settings");
    }

    auto backend = smu::platform::GetNetworkLagBackend();
    const bool available = backend && backend->isAvailable();
    const smu::platform::LagSwitchConfig config = backend ? backend->effectiveConfig() : smu::platform::LagSwitchConfig{};

    lua_createtable(L, 0, 7);
    lua_pushboolean(L, available);
    lua_setfield(L, -2, "available");
    lua_pushboolean(L, config.enabled);
    lua_setfield(L, -2, "enabled");
    lua_pushboolean(L, backend ? backend->isBlockingActive() : false);
    lua_setfield(L, -2, "active");
    lua_pushboolean(L, backend ? backend->isBaseBlockingActive() : false);
    lua_setfield(L, -2, "baseActive");
    lua_pushstring(L, available ? "available" : "unsupported");
    lua_setfield(L, -2, "platformSupport");
    lua_pushstring(L, backend ? backend->unsupportedReason().c_str() : "Network lagswitch backend is unavailable.");
    lua_setfield(L, -2, "unsupportedReason");
    lua_pushstring(L, LagSwitchTargetModeToString(config.targetMode));
    lua_setfield(L, -2, "targetMode");
    return 1;
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
    instance.recordSettingsText(text ? ClampedString(text, textLength) : std::string(), width);
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
    const float height = CheckOptionalNonNegativeFloat(L, 1, 0.0f, "height");
    instance.recordSettingsSeparator(height);
    if (IsSettingsRenderMode(L)) {
        ImGui::Separator();
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
    instance.recordSettingsCheckbox(id, label, defaultValue, width);
    CheckUiIdBudget(L, instance, id);
    ImportedScriptRecord& record = instance.owner();
    bool hasStored = false;
    bool stored = defaultValue;
    {
        std::lock_guard<std::mutex> uiLock(record.uiStateMutex);
        const auto storedIt = record.uiState.find(id);
        hasStored = storedIt != record.uiState.end();
        if (hasStored && storedIt->is_boolean()) {
            stored = storedIt->get<bool>();
        }
    }
    if (!hasStored) {
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
    instance.recordSettingsSliderInt(id, label, defaultValue, minValue, maxValue, width);
    CheckUiIdBudget(L, instance, id);
    ImportedScriptRecord& record = instance.owner();
    int stored = defaultValue;
    bool hasStored = false;
    {
        std::lock_guard<std::mutex> uiLock(record.uiStateMutex);
        const auto storedIt = record.uiState.find(id);
        hasStored = storedIt != record.uiState.end();
        if (hasStored) {
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
    }
    if (!hasStored) {
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
    instance.recordSettingsSliderFloat(id, label, defaultValue, minValue, maxValue, width);
    CheckUiIdBudget(L, instance, id);
    ImportedScriptRecord& record = instance.owner();
    double stored = defaultValue;
    bool hasStored = false;
    {
        std::lock_guard<std::mutex> uiLock(record.uiStateMutex);
        const auto storedIt = record.uiState.find(id);
        hasStored = storedIt != record.uiState.end();
        if (hasStored && storedIt->is_number()) {
            const double storedNumber = storedIt->get<double>();
            if (std::isfinite(storedNumber)) {
                stored = storedNumber;
            }
        }
    }
    stored = std::clamp(stored, static_cast<double>(-std::numeric_limits<float>::max()), static_cast<double>(std::numeric_limits<float>::max()));
    if (!hasStored) {
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
    instance.recordSettingsTextbox(id, label, ClampedString(defaultValue, defaultLength), width, height);
    CheckUiIdBudget(L, instance, id);
    ImportedScriptRecord& record = instance.owner();
    bool hasStored = false;
    std::string stored = ClampedString(defaultValue, defaultLength);
    {
        std::lock_guard<std::mutex> uiLock(record.uiStateMutex);
        const auto storedIt = record.uiState.find(id);
        hasStored = storedIt != record.uiState.end();
        if (hasStored && storedIt->is_string()) {
            stored = storedIt->get<std::string>();
        }
    }
    if (stored.size() > kMaxUiStateStringBytes) {
        stored.resize(kMaxUiStateStringBytes);
    }
    if (!hasStored) {
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
    instance.recordSettingsDynamicTextbox(id, label, ClampedString(defaultValue, defaultLength), width, height);
    CheckUiIdBudget(L, instance, id);
    ImportedScriptRecord& record = instance.owner();
    bool hasStored = false;
    std::string stored = ClampedString(defaultValue, defaultLength);
    {
        std::lock_guard<std::mutex> uiLock(record.uiStateMutex);
        const auto storedIt = record.uiState.find(id);
        hasStored = storedIt != record.uiState.end();
        if (hasStored && storedIt->is_string()) {
            stored = storedIt->get<std::string>();
        }
    }
    stored = ClampDynamicText(std::move(stored));

    if (!hasStored) {
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
    std::size_t idLength = 0;
    const char* idText = CheckUiId(L, 1, idLength);
    std::size_t textLength = 0;
    const char* text = luaL_checklstring(L, 2, &textLength);

    const std::string id(idText, idLength);
    CheckUiIdBudget(L, instance, id);
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
    instance.recordSettingsKeybind(id, label, defaultValue, width);
    CheckUiIdBudget(L, instance, id);
    ImportedScriptRecord& record = instance.owner();
    unsigned int stored = defaultValue;
    bool hasStored = false;
    {
        std::lock_guard<std::mutex> uiLock(record.uiStateMutex);
        const auto storedIt = record.uiState.find(id);
        hasStored = storedIt != record.uiState.end();
        if (hasStored) {
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
    }
    if (!hasStored) {
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
    CacheMoveDegreesSettings(L);
    Register(L, "log", LuaLog);
    Register(L, "nowMicros", LuaNowMicros);
    Register(L, "sleep", LuaSleep);
    Register(L, "pressKey", LuaPressKey);
    Register(L, "clickMouse", LuaClickMouse);
    Register(L, "holdKey", LuaHoldKey);
    Register(L, "releaseKey", LuaReleaseKey);
    Register(L, "isKeyPressed", LuaIsKeyPressed);
    Register(L, "isHotkeyPressed", LuaIsHotkeyPressed);
    Register(L, "getScriptHotkey", LuaGetScriptHotkey);
    Register(L, "isCancelled", LuaIsCancelled);
    Register(L, "sleepUntilCancelled", LuaSleepUntilCancelled);
    Register(L, "throwIfCancelled", LuaThrowIfCancelled);
    Register(L, "typeText", LuaTypeText);
    Register(L, "moveMouse", LuaMoveMouse);
    Register(L, "moveMouseAbs", LuaMoveMouseAbs);
    Register(L, "setMouseMotionMode", LuaSetMouseMotionMode);
    Register(L, "getMouseMotionMode", LuaGetMouseMotionMode);
    Register(L, "getPixelColor", LuaGetPixelColor);
    Register(L, "getPixelRect", LuaGetPixelRect);
    Register(L, "moveDegrees", LuaMoveDegrees);
    Register(L, "mouseWheel", LuaMouseWheel);
    Register(L, "freeze", LuaFreeze);
    Register(L, "lagSwitch", LuaLagSwitch);
    Register(L, "getLagSwitchConfig", LuaGetLagSwitchConfig);
    Register(L, "setLagSwitchConfig", LuaSetLagSwitchConfig);
    Register(L, "getLagSwitchStatus", LuaGetLagSwitchStatus);
    Register(L, "clearLagSwitchConfig", LuaClearLagSwitchConfig);
    Register(L, "getPlatform", LuaGetPlatform);
    Register(L, "getSMUVersion", LuaGetSMUVersion);
    Register(L, "getSavedValue", LuaGetSavedValue);
    RegisterUiTable(L);
    Register(L, "robloxFreeze", LuaFreeze);
    Register(L, "roblox_freeze", LuaFreeze);
    Register(L, "lagswitch", LuaLagSwitch);
}

} // namespace smu::app

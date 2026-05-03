#include "script_api.h"

#include "input_actions.h"
#include "script_instance.h"
#include "script_metadata.h"
#include "../core/legacy_globals.h"
#include "../platform/input_backend.h"
#include "../platform/logging.h"
#include "../platform/text_input_backend.h"

#include <algorithm>
#include <string>

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

smu::core::KeyCode CheckKey(lua_State* L, int index)
{
    if (lua_isinteger(L, index)) {
        const auto key = static_cast<smu::core::KeyCode>(lua_tointeger(L, index));
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

int LuaLog(lua_State* L)
{
    const char* message = luaL_checkstring(L, 1);
    LogInfo(std::string("[script] ") + (message ? message : ""));
    return 0;
}

int LuaSleep(lua_State* L)
{
    ScriptInstance& instance = RequireInstance(L);
    const int ms = static_cast<int>(luaL_checkinteger(L, 1));
    instance.sleepWithDeadline(ms);
    return 0;
}

int LuaPressKey(lua_State* L)
{
    const smu::core::KeyCode key = CheckKey(L, 1);
    const int delay = lua_gettop(L) >= 2 ? static_cast<int>(luaL_checkinteger(L, 2)) : 50;
    PressKey(key, std::max(0, delay));
    return 0;
}

int LuaHoldKey(lua_State* L)
{
    HoldKey(CheckKey(L, 1));
    return 0;
}

int LuaReleaseKey(lua_State* L)
{
    ReleaseKey(CheckKey(L, 1));
    return 0;
}

int LuaIsKeyPressed(lua_State* L)
{
    lua_pushboolean(L, IsKeyPressed(CheckKey(L, 1)));
    return 1;
}

int LuaTypeText(lua_State* L)
{
    const char* text = luaL_checkstring(L, 1);
    const int delay = lua_gettop(L) >= 2 ? static_cast<int>(luaL_checkinteger(L, 2)) : 30;
    auto backend = smu::platform::GetInputBackend();
    if (!backend) {
        return luaL_error(L, "input backend is not available");
    }
    smu::platform::typeText(*backend, text ? std::string(text) : std::string(), std::max(0, delay));
    return 0;
}

int LuaMoveMouse(lua_State* L)
{
    const int dx = static_cast<int>(luaL_checkinteger(L, 1));
    const int dy = static_cast<int>(luaL_checkinteger(L, 2));
    MoveMouse(dx, dy);
    return 0;
}

int LuaMouseWheel(lua_State* L)
{
    const int delta = static_cast<int>(luaL_checkinteger(L, 1));
    MouseWheel(delta);
    return 0;
}

int LuaFreeze(lua_State* L)
{
    ScriptInstance& instance = RequireInstance(L);
    instance.setFreeze(lua_toboolean(L, 1) != 0);
    return 0;
}

int LuaLagSwitch(lua_State* L)
{
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

void Register(lua_State* L, const char* name, lua_CFunction function)
{
    lua_pushcfunction(L, function);
    lua_setglobal(L, name);
}

} // namespace

void RegisterScriptApi(lua_State* L)
{
    Register(L, "log", LuaLog);
    Register(L, "sleep", LuaSleep);
    Register(L, "pressKey", LuaPressKey);
    Register(L, "holdKey", LuaHoldKey);
    Register(L, "releaseKey", LuaReleaseKey);
    Register(L, "isKeyPressed", LuaIsKeyPressed);
    Register(L, "typeText", LuaTypeText);
    Register(L, "moveMouse", LuaMoveMouse);
    Register(L, "mouseWheel", LuaMouseWheel);
    Register(L, "freeze", LuaFreeze);
    Register(L, "lagSwitch", LuaLagSwitch);
    Register(L, "getPlatform", LuaGetPlatform);
    Register(L, "getSMUVersion", LuaGetSMUVersion);
    Register(L, "robloxFreeze", LuaFreeze);
    Register(L, "roblox_freeze", LuaFreeze);
    Register(L, "lagswitch", LuaLagSwitch);
}

} // namespace smu::app

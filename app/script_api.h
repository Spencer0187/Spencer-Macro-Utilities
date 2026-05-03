#pragma once

struct lua_State;

namespace smu::app {

void RegisterScriptApi(lua_State* L);

} // namespace smu::app

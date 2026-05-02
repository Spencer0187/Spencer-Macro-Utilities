#include "app_state.h"

namespace smu::core {
namespace {
AppState g_appState;
}

AppState& GetAppState()
{
    return g_appState;
}

void ResetRuntimeAppFlags()
{
    g_appState.running.store(true, std::memory_order_release);
    g_appState.done.store(false, std::memory_order_release);
}

} // namespace smu::core

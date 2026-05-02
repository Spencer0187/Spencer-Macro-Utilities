#pragma once

#include "app_context.h"

namespace smu::app {

struct AppMainConfig {
    const char* title = "Spencer Macro Client";
    int defaultWidth = 1280;
    int defaultHeight = 800;
};

int RunSharedApp(AppContext& context, const AppMainConfig& config = {});

} // namespace smu::app

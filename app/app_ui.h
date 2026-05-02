#pragma once

#include "app_context.h"

namespace smu::app {

void RenderAppUi(AppContext& context);
void RenderPlatformCriticalNotifications();
void RenderPlatformWarningNotifications();
void RenderForegroundDependentCheckbox(AppContext& context, const char* label, const char* id, bool* value);

} // namespace smu::app

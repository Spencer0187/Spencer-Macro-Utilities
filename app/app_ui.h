#pragma once

#include "app_context.h"

#include <filesystem>

namespace smu::app {

void RenderAppUi(AppContext& context);
void RenderPlatformCriticalNotifications();
void RenderPlatformWarningNotifications();
void RenderForegroundDependentCheckbox(AppContext& context, const char* label, const char* id, bool* value);
bool QueueDroppedScriptImport(const std::filesystem::path& path);
void ResetFloatingUiWindowState();

} // namespace smu::app

#pragma once

struct ImGuiIO;

namespace smu::app {

void InitializeSharedThemeSystem();
void ApplySharedTheme();
void RenderSharedThemeEditor(bool* open);
void SetupSharedFontsAndStyle(ImGuiIO& io);

} // namespace smu::app
